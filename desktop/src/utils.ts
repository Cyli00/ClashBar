export const PAGE_SIZE = 50;
export const LOG_LIMIT = 500;

export function pageSlice<T>(items: T[], requestedPage: number, size = PAGE_SIZE) {
  const total = items.length;
  const pages = Math.max(1, Math.ceil(total / size));
  const page = Math.max(1, Math.min(pages, Math.floor(requestedPage) || 1));
  const start = (page - 1) * size;
  return { items: items.slice(start, start + size), page, pages, total, start: total ? start + 1 : 0, end: Math.min(start + size, total) };
}

export function matchesQuery(values: unknown[], query: string): boolean {
  const needle = query.trim().toLocaleLowerCase('zh-CN');
  return !needle || values.some(value => String(value ?? '').toLocaleLowerCase('zh-CN').includes(needle));
}

export function formatBytes(value?: number): string {
  if (value == null || !Number.isFinite(value) || value < 0) return '—';
  if (value < 1024) return `${Math.round(value)} B`;
  const exponent = Math.min(4, Math.floor(Math.log(value) / Math.log(1024)));
  return `${new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 1 }).format(value / 1024 ** exponent)} ${['B', 'KiB', 'MiB', 'GiB', 'TiB'][exponent]}`;
}

export function validatePorts(mixed: string, controller: string): string | null {
  const valid = (value: string) => /^\d+$/.test(value) && Number(value) >= 1024 && Number(value) <= 65535;
  if (!valid(mixed) || !valid(controller)) return '端口需为 1024–65535 之间的整数。';
  if (Number(mixed) === Number(controller)) return '代理端口和控制端口不能相同。';
  return null;
}

export function validateSubscription(value: string): string | null {
  try {
    const url = new URL(value.trim());
    if (url.protocol !== 'https:' || !url.hostname || url.port || url.username || url.password || url.hash) {
      return '请输入使用默认 443 端口的 HTTPS 链接，且不包含用户名、密码或片段标识。';
    }
    return null;
  } catch {
    return '请输入有效的 HTTPS 订阅链接。';
  }
}

// Backend errors may contain a subscription URL; never echo its credentials.
export function safeError(error: unknown): string {
  const raw = error instanceof Error ? error.message : String(error);
  return raw.replace(/https?:\/\/[^\s"'<>]+/gi, '[链接已隐藏]').slice(0, 500);
}

/** A mutation or newer read invalidates all previously issued reads. */
export class ReadEpoch {
  private value = 0;
  next(): number { return ++this.value; }
  current(ticket: number): boolean { return ticket === this.value; }
}
