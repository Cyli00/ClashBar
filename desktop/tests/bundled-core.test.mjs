import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, writeFile, rm, stat } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { gzipSync } from 'node:zlib';
import { prepareBundledCore, validateExecutable } from '../scripts/prepare-core.mjs';

function executable() {
  const bytes = Buffer.alloc(128);
  bytes.write('MZ'); bytes.writeUInt32LE(64, 60); bytes.write('PE\0\0', 64);
  bytes.writeUInt16LE(0x8664, 68); bytes.writeUInt16LE(2, 86); bytes.writeUInt16LE(0x20b, 88);
  return bytes;
}

test('可信exe与gzip准备资源和校验清单，但不执行文件', async () => {
  const directory = await mkdtemp(path.join(tmpdir(), 'clashbar-bundle-test-'));
  try {
    for (const suffix of ['.exe', '.gz']) {
      const bytes = executable();
      const input = path.join(directory, `source${suffix}`);
      const output = path.join(directory, `output-${suffix.slice(1)}`);
      const source = suffix === '.gz' ? gzipSync(bytes) : bytes;
      await writeFile(input, source);
      const manifest = await prepareBundledCore(input, output);
      assert.equal(manifest.executableSize, bytes.length);
      assert.equal(manifest.executableSha256.length, 64);
      assert.deepEqual(await readFile(path.join(output, manifest.fileName)), source);
      assert.deepEqual(JSON.parse(await readFile(path.join(output, 'core.json'), 'utf8')), manifest);
      assert.deepEqual(await readFile(input), source);
    }
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('损坏gzip或非x64可执行文件不会生成随包资源', async () => {
  const directory = await mkdtemp(path.join(tmpdir(), 'clashbar-bundle-test-'));
  try {
    const input = path.join(directory, 'broken.gz');
    const output = path.join(directory, 'output');
    await writeFile(input, Buffer.from('not gzip'));
    await assert.rejects(prepareBundledCore(input, output), /gzip/);
    await assert.rejects(stat(output), {code:'ENOENT'});
    const bytes = executable(); bytes.writeUInt16LE(0x14c, 68);
    assert.throws(() => validateExecutable(bytes), /x64/);
    assert.throws(() => validateExecutable(Buffer.from('not an exe')), /x64/);
  } finally { await rm(directory, { recursive: true, force: true }); }
});
