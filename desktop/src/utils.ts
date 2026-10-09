import { t, locale } from './i18n.ts';
import { localizeBackendError } from './errors.ts';
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
    const needle = query.trim().toLocaleLowerCase(locale);
    return !needle || values.some(value => String(value ?? '').toLocaleLowerCase(locale).includes(needle));
}
export function formatBytes(value?: number): string {
    if (value == null || !Number.isFinite(value) || value < 0)
        return '—';
    if (value < 1024)
        return `${Math.round(value)} B`;
    const exponent = Math.min(4, Math.floor(Math.log(value) / Math.log(1024)));
    return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(value / 1024 ** exponent)} ${['B', 'KiB', 'MiB', 'GiB', 'TiB'][exponent]}`;
}
export function validatePorts(mixed: string, controller: string): string | null {
    const valid = (value: string) => /^\d+$/.test(value) && Number(value) >= 1024 && Number(value) <= 65535;
    if (!valid(mixed) || !valid(controller))
        return t("端口需为 1024–65535 之间的整数。");
    if (Number(mixed) === Number(controller))
        return t("代理端口和控制端口不能相同。");
    return null;
}
export function validateRemotePorts(values: string[]): string | null {
    if (values.some(value => !/^\d+$/.test(value) || Number(value) > 65535))
        return t("端口需为 0–65535 之间的整数，0 表示关闭。");
    const enabled = values.map(Number).filter(value => value > 0);
    if (new Set(enabled).size !== enabled.length)
        return t("已启用的代理端口不能重复。");
    return null;
}
export function validateLocalPorts(values: string[], controller: string): string | null {
    const proxyError = validateRemotePorts(values);
    if (proxyError) return proxyError;
    if (!/^\d+$/.test(controller) || Number(controller) < 1024 || Number(controller) > 65535) return t('端口需为 1024–65535 之间的整数。');
    return values.map(Number).includes(Number(controller)) ? t('代理端口和控制端口不能相同。') : null;
}
export function terminalProxyCommand(host: string, http?: number, socks?: number): string {
    if (!host || /[^a-zA-Z0-9._:\[\]-]/.test(host))
        return '';
    const valid = (value?: number) => value != null && Number.isInteger(value) && value > 0 && value <= 65535;
    if (!valid(http) && !valid(socks))
        return '';
    const authority = host.includes(':') && !host.startsWith('[') ? `[${host}]` : host;
    try {
        new URL(`http://${authority}:1`);
    }
    catch {
        return '';
    }
    return `$env:HTTP_PROXY="${valid(http) ? `http://${authority}:${http}` : ''}"; $env:HTTPS_PROXY=$env:HTTP_PROXY; $env:ALL_PROXY="${valid(socks) ? `socks5://${authority}:${socks}` : ''}"`;
}
export function validateSubscription(value: string): string | null {
    try {
        const url = new URL(value.trim());
        if (url.protocol !== 'https:' || !url.hostname || url.port || url.username || url.password || url.hash) {
            return t("请输入使用默认 443 端口的 HTTPS 链接，且不包含用户名、密码或片段标识。");
        }
        return null;
    }
    catch {
        return t("请输入有效的 HTTPS 订阅链接。");
    }
}
// 后端错误可能包含订阅链接，输出前隐藏完整 URL。
export function safeError(error: unknown): string {
    const raw = error instanceof Error ? error.message : String(error);
    return localizeBackendError(raw.replace(/https?:\/\/[^\s"'<>]+/gi, t("[链接已隐藏]"))).slice(0, 500);
}
/** 写操作或后续读取开始后，旧读取结果不再有效。 */
export class ReadEpoch {
    private value = 0;
    next(): number { return ++this.value; }
    current(ticket: number): boolean { return ticket === this.value; }
}
