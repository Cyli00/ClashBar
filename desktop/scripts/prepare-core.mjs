import { createHash, randomUUID } from 'node:crypto';
import { lstat, mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { gunzipSync } from 'node:zlib';

export const MAX_SOURCE_BYTES = 128 * 1024 * 1024;
export const MAX_EXECUTABLE_BYTES = 256 * 1024 * 1024;
const defaultOutput = fileURLToPath(new URL('../src-tauri/resources/bin/', import.meta.url));

export function validateExecutable(bytes) {
  const error = () => new Error('内核必须为 Windows x64 PE 可执行文件。');
  if (bytes.length < 64 || bytes.toString('ascii', 0, 2) !== 'MZ') throw error();
  const offset = bytes.readUInt32LE(60);
  if (offset + 26 > bytes.length || !bytes.subarray(offset, offset + 4).equals(Buffer.from('PE\0\0')) || bytes.readUInt16LE(offset + 4) !== 0x8664 || (bytes.readUInt16LE(offset + 22) & 2) === 0 || bytes.readUInt16LE(offset + 24) !== 0x20b) throw error();
}

const sha256 = bytes => createHash('sha256').update(bytes).digest('hex');

export async function prepareBundledCore(inputPath, outputDirectory = defaultOutput) {
  const sourcePath = path.resolve(inputPath);
  const metadata = await lstat(sourcePath);
  if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.size === 0 || metadata.size > MAX_SOURCE_BYTES) throw new Error('内核来源必须是不超过 128 MiB 的普通文件。');
  const compressed = path.extname(sourcePath).toLowerCase() === '.gz';
  if (!compressed && path.extname(sourcePath).toLowerCase() !== '.exe') throw new Error('请选择可信的 .exe 或 gzip 内核文件。');
  const source = await readFile(sourcePath);
  if (source.length > MAX_SOURCE_BYTES) throw new Error('内核来源超过 128 MiB。');
  let executable;
  try { executable = compressed ? gunzipSync(source, { maxOutputLength: MAX_EXECUTABLE_BYTES }) : source; }
  catch { throw new Error('gzip 内核损坏或解压后超过 256 MiB。'); }
  validateExecutable(executable);
  const fileName = compressed ? 'mihomo.exe.gz' : 'mihomo.exe';
  const manifest = { fileName, sha256: sha256(source), executableSha256: sha256(executable), executableSize: executable.length };
  await mkdir(outputDirectory, { recursive: true });
  const temporary = path.join(outputDirectory, `.core-${randomUUID()}.tmp`);
  await writeFile(temporary, source, { flag: 'wx' });
  await rename(temporary, path.join(outputDirectory, fileName));
  const manifestTemporary = path.join(outputDirectory, `.manifest-${randomUUID()}.tmp`);
  await writeFile(manifestTemporary, `${JSON.stringify(manifest, null, 2)}\n`, { flag: 'wx' });
  await rename(manifestTemporary, path.join(outputDirectory, 'core.json'));
  return manifest;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  if (process.argv.length !== 3) {
    console.error('用法：npm run core:bundle -- <可信的 mihomo.exe 或 mihomo.gz 路径>');
    process.exitCode = 1;
  } else {
    try {
      const manifest = await prepareBundledCore(process.argv[2]);
      console.log(`已准备随包内核 ${manifest.fileName}（解压后 ${manifest.executableSize} 字节）。`);
      console.log(`SHA-256: ${manifest.executableSha256}`);
      console.log('此操作只准备构建资源，不下载、不执行内核。');
    } catch (error) {
      console.error(`随包内核准备失败：${error instanceof Error ? error.message : '未知错误'}`);
      process.exitCode = 1;
    }
  }
}
