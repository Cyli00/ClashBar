import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Connection, Proxy, Rule, Snapshot, Status, Tab } from './types';
import { formatBytes, LOG_LIMIT, matchesQuery, ReadEpoch, safeError, validatePorts, validateSubscription } from './utils';
import { button, el, emptyState, field, icon, iconButton, replacePreservingFocus, searchField, setFieldError } from './ui';
import { dismissMenu, initializeMenus, menuIsOpen, scheduleMenuClose, scheduleMenuOpen, showMenu, updateMenuValue, type MenuChoice } from './menu';
import logoUrl from './assets/logo.png';

const desktop = isTauri();
const epoch = new ReadEpoch();
const tabs: [Tab, string][] = [['proxies', '节点'], ['rules', '分流'], ['connections', '连接'], ['logs', '日志'], ['settings', '设置']];
let status: Status | null = null, snapshot: Snapshot | null = null;
let activeTab: Tab = 'proxies', busy = false, refreshing = false, popupVisible = true, pinned = false;
let actionError = '', refreshError = '', pendingLabel = '';
type LogRow = { timestamp: number; source: string; line: string };
let logs: LogRow[] = [], appLogs: LogRow[] = [];
let ruleQuery = '', ruleType = '全部', rulePolicy = '', connectionQuery = '', logQuery = '';
const sourceFilter = new Set<string>(), levelFilter = new Set<string>();
let combinedLogs: LogRow[] = [];
let providerCollapsed = preference('providers-collapsed', false), sortLatency = preference('sort-latency', false), showHistory = preference('show-history', false), hideHidden = preference('hide-hidden', true), groupRules = preference('group-rules', false);
let transport = localStorage.getItem('clashbar-transport') || '', connectionSort = localStorage.getItem('clashbar-connection-sort') || '默认顺序';
const expandedPolicies = new Set<string>();
const delays = new Map<string, number>();
const gated = new Map<HTMLButtonElement | HTMLInputElement, () => boolean>();
let lastSample: { time: number; up: number; down: number } | null = null;
const trafficSamples: { up: number; down: number }[] = [];
let rateUp: number | null = null, rateDown: number | null = null;
let portsDirty = false;
let portSaveTimer: ReturnType<typeof setTimeout> | undefined;
let resizeTimer: ReturnType<typeof setTimeout> | undefined, lastRequestedHeight = 0;
const unlisten: (() => void)[] = [];
const scrollPositions = new Map<Tab, number>();

function preference(key: string, fallback: boolean) { const value = localStorage.getItem(`clashbar-${key}`); return value === null ? fallback : value === 'true'; }
function savePreference(key: string, value: boolean) { localStorage.setItem(`clashbar-${key}`, String(value)); }
function gate<T extends HTMLButtonElement | HTMLInputElement>(control: T, enabled: () => boolean): T { gated.set(control, enabled); return control; }
function canRun() { return Boolean(status?.running); }
function unsupported(control: HTMLButtonElement | HTMLInputElement, explanation: string) { control.disabled = true; control.title = explanation; control.setAttribute('aria-description', explanation); return control; }
function setIcon(control: HTMLButtonElement, symbol: string, label: string) { control.replaceChildren(icon(symbol)); control.setAttribute('aria-label', label); control.title = label; }
function choose(anchor: HTMLElement, title: string, choices: MenuChoice[], focus = true) { void showMenu(anchor, title, choices, focus).catch(error => { actionError = `无法打开菜单。${safeError(error)}`; updateChrome(); }); }
function settingSelect(label: string, items: string[] | (() => string[]), value: () => string, change: (next: string) => void | Promise<unknown>) {
  const choices = () => (typeof items === 'function' ? items() : items).map(name => ({ label: name, checked: value() === name, action: () => change(name) }));
  const control = button('', () => choose(control, label, choices()), 'value-select');
  control.addEventListener('pointerenter', () => { if (!control.disabled) scheduleMenuOpen(() => choose(control, label, choices(), false)); });
  control.addEventListener('pointerleave', scheduleMenuClose);
  const update = () => { control.replaceChildren(el('span', '', value()), icon('down')); control.setAttribute('aria-label', label); control.title = value(); };
  update(); return { control, update };
}

const app = document.querySelector<HTMLDivElement>('#app')!;
const shell = el('main', 'menu-panel');
const header = el('header', 'app-header');
const logo = el('img', 'brand-logo'); logo.src = logoUrl; logo.alt = 'ClashBar'; logo.width = logo.height = 40;
const identity = el('div', 'identity'); identity.append(el('h1', '', 'ClashBar'));
const endpointRow = el('div', 'endpoint-row');
const endpoint = button('', () => choose(endpoint, '控制器', [
  { label: '本机', detail: `127.0.0.1:${status?.controllerPort ?? 9090}`, checked: true, action: () => {} },
  { label: '管理远程机器', detail: 'Windows 版本尚未提供', disabled: true, action: () => {} },
]), 'endpoint-button');
const statusDot = el('span', 'status-dot'); const endpointText = el('span', '', '127.0.0.1:9090'); endpoint.append(statusDot, endpointText, icon('down'));
const webUI = unsupported(iconButton('globe', '打开 WebUI', () => {}), '此版本未捆绑 WebUI；请使用下方节点与分流页面。');
endpointRow.append(endpoint, webUI); identity.append(endpointRow);
endpoint.addEventListener('pointerenter', () => scheduleMenuOpen(() => choose(endpoint, '控制器', [{ label: '本机', detail: `127.0.0.1:${status?.controllerPort ?? 9090}`, checked: true, action: () => {} }, { label: '管理远程机器', detail: 'Windows 版本尚未提供', disabled: true, action: () => {} }], false))); endpoint.addEventListener('pointerleave', scheduleMenuClose);
const headerActions = el('div', 'header-actions');
const pinButton = gate(iconButton('pin', '固定面板', async () => { const next = !pinned; if (await mutate(next ? '固定面板' : '取消固定', () => invoke('set_popup_pinned', { pinned: next }), false)) { pinned = next; updateChrome(); } }), () => true);
const restartButton = gate(iconButton('restart', '重启内核', () => mutate(status?.running ? '重启内核' : '启动内核', () => invoke<Status>(status?.running ? 'restart_core' : 'start_core'))), () => Boolean(status?.corePath && status.configName));
const runButton = gate(iconButton('play', '启动内核', () => mutate(status?.running ? '停止内核' : '启动内核', () => invoke<Status>(status?.running ? 'stop_core' : 'start_core'))), () => Boolean(status && (status.running || (status.corePath && status.configName))));
const quitButton = gate(iconButton('power', '退出 ClashBar', () => invoke('quit_app')), () => true);
headerActions.append(pinButton, restartButton, runButton, quitButton); header.append(logo, identity, headerActions);
const modes = el('div', 'mode-segments'); modes.setAttribute('role', 'group'); modes.setAttribute('aria-label', '代理模式');
const modeButtons = new Map<string, HTMLButtonElement>();
for (const [value, label, symbol] of [['rule', '规则', 'shield'], ['global', '全局', 'globe'], ['direct', '直连', 'bolt']]) {
  const mode = gate(button('', () => { if (snapshot?.configs.mode !== value) return mutate('切换代理模式', () => invoke('set_mode', { mode: value })); }, 'mode-button'), () => Boolean(status?.running && snapshot));
  mode.append(icon(symbol), el('span', '', label)); mode.setAttribute('aria-label', `${label}模式`); modes.append(mode); modeButtons.set(value, mode);
}
const navigation = el('nav', 'tabs'); navigation.setAttribute('role', 'tablist'); navigation.setAttribute('aria-label', '功能页面');
const tabButtons = new Map<Tab, HTMLButtonElement>(), panels = new Map<Tab, HTMLElement>();
for (const [key, title] of tabs) {
  const tab = button(title, () => activateTab(key), 'tab'); tab.id = `tab-${key}`; tab.setAttribute('role', 'tab'); tab.setAttribute('aria-controls', `panel-${key}`);
  tab.addEventListener('keydown', event => {
    if (event.isComposing || !['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault(); const index = tabs.findIndex(([id]) => id === key);
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? tabs.length - 1 : (index + (event.key === 'ArrowRight' ? 1 : -1) + tabs.length) % tabs.length;
    activateTab(tabs[next][0]); tabButtons.get(tabs[next][0])?.focus();
  });
  tabButtons.set(key, tab); navigation.append(tab);
  const panel = el('section', 'tab-panel'); panel.id = `panel-${key}`; panel.setAttribute('role', 'tabpanel'); panel.setAttribute('aria-labelledby', tab.id); panels.set(key, panel);
}
const content = el('div', 'content-scroll');
const notice = el('p', 'sr-only'); notice.setAttribute('role', 'status'); notice.setAttribute('aria-live', 'polite');
const banner = el('div', 'error-banner'); banner.setAttribute('role', 'alert'); banner.hidden = true;
const errorCopy = el('span'); const retryButton = iconButton('refresh', '重新读取', () => refresh()); banner.append(errorCopy, retryButton);
const browserNotice = el('p', 'browser-notice', '此页面需要 ClashBar 桌面应用。浏览器无法管理本机内核和系统代理。'); browserNotice.hidden = desktop;
content.append(browserNotice, banner, ...panels.values());
const footer = el('footer', 'app-footer');
const coreVersion = gate(button('', () => { activateTab('settings'); coreButton.focus(); }, 'footer-core'), () => Boolean(status)); coreVersion.append(icon('chip'), el('span', '', 'MIHOMO —'));
const corePicker = gate(iconButton('arrowDown', '选择 mihomo 内核', () => mutate('选择内核', () => invoke<Status>('choose_core'))), () => Boolean(status && !status.running));
footer.append(coreVersion, corePicker, el('span', 'app-version', 'v0.1.0'));
shell.append(header, modes, navigation, content, footer, notice); app.append(shell);

const proxyPanel = panels.get('proxies')!;
const traffic = el('section', 'traffic'); traffic.setAttribute('aria-label', '代理流量');
const trafficTop = el('div', 'traffic-top'), trafficBottom = el('div', 'traffic-bottom');
const connectionCount = el('span', 'metric connection-count'), upMetric = el('span', 'metric'), memoryMetric = el('span', 'metric memory-count'), downMetric = el('span', 'metric');
trafficTop.append(connectionCount, upMetric); trafficBottom.append(memoryMetric, downMetric);
const chart = document.createElementNS('http://www.w3.org/2000/svg', 'svg'); chart.setAttribute('viewBox', '0 0 336 60'); chart.setAttribute('preserveAspectRatio', 'none'); chart.setAttribute('class', 'traffic-chart'); chart.setAttribute('aria-hidden', 'true');
traffic.append(chart, trafficTop, trafficBottom);
const configButton = gate(button('', () => openConfigMenu(), 'quick-row'), () => Boolean(status));
configButton.addEventListener('pointerenter', () => { if (!configButton.disabled) scheduleMenuOpen(() => openConfigMenu(false)); }); configButton.addEventListener('pointerleave', scheduleMenuClose);
const configName = el('span', 'quick-value', '未导入'); configButton.append(icon('document', 'purple'), el('strong', '', '切换配置'), configName, icon('chevron', 'tertiary'));
const systemRow = el('label', 'quick-row'); systemRow.htmlFor = 'system-proxy';
const systemProxy = gate(el('input', 'switch'), () => Boolean(status?.running || status?.systemProxy)); systemProxy.id = 'system-proxy'; systemProxy.type = 'checkbox';
systemRow.append(icon('globe', 'green'), el('strong', '', '系统代理'), el('span', 'flex-space'), systemProxy);
systemProxy.addEventListener('change', () => { const enabled = systemProxy.checked; systemProxy.checked = status?.systemProxy ?? false; void mutate(enabled ? '开启系统代理' : '关闭系统代理', () => invoke<Status>('set_system_proxy', { enabled })); });
const tunRow = el('div', 'quick-row unsupported-row'); tunRow.title = '此 Windows 版本尚未提供 TUN 控制。';
const tunModes = el('span', 'tun-modes'); for (const name of ['system', 'gvisor', 'mixed', 'mips']) tunModes.append(el('span', 'capsule', name));
const tunSwitch = unsupported(el('input', 'switch'), tunRow.title); tunSwitch.type = 'checkbox'; tunSwitch.setAttribute('aria-label', 'TUN 模式，此版本未提供');
tunRow.append(icon('shield', 'green'), el('strong', '', 'TUN 模式'), tunModes, tunSwitch);
const terminalRow = el('div', 'quick-row');
const copyTerminal: HTMLButtonElement = gate(button('', async () => { if (await copyText(`$env:HTTP_PROXY="http://127.0.0.1:${status?.mixedPort}"; $env:HTTPS_PROXY=$env:HTTP_PROXY; $env:ALL_PROXY="socks5://127.0.0.1:${status?.mixedPort}"`, '已复制 PowerShell 代理命令')) { copyTerminal.replaceChildren(el('span', '', '已复制'), icon('check', 'green')); setTimeout(() => copyTerminal.replaceChildren(el('span', '', '127.0.0.1'), icon('copy')), 1600); } }, 'capsule copy-command'), () => Boolean(status)); copyTerminal.append(el('span', '', '127.0.0.1'), icon('copy')); copyTerminal.setAttribute('aria-label', '复制 PowerShell 代理命令');
terminalRow.append(icon('terminal', 'orange'), el('strong', '', '复制终端命令'), el('span', 'flex-space'), copyTerminal);
const providerHeader = sectionHeading('代理提供者', 'drive');
const providerCounter = el('span', 'count-badge', '0'); providerHeader.insertBefore(providerCounter, providerHeader.lastElementChild);
const collapseProviders = iconButton('down', '折叠代理提供者', () => { providerCollapsed = !providerCollapsed; savePreference('providers-collapsed', providerCollapsed); renderProxies(); }); providerHeader.append(collapseProviders);
const providers = el('div', 'providers');
const groupHeader = sectionHeading('代理组', 'network');
const groupCounter = el('span', 'count-badge', '0'); groupHeader.insertBefore(groupCounter, groupHeader.lastElementChild);
const sortButton = iconButton('list', '按延迟排序节点', () => { sortLatency = !sortLatency; savePreference('sort-latency', sortLatency); updateChrome(); });
const historyButton = iconButton('chart', '显示延迟历史', () => { showHistory = !showHistory; savePreference('show-history', showHistory); renderProxies(); });
const hiddenButton = iconButton('eye', '显示隐藏代理组', () => { hideHidden = !hideHidden; savePreference('hide-hidden', hideHidden); renderProxies(); });
const allDelay = gate(iconButton('gauge', '测试全部代理组延迟', async () => {
  await mutate('测试全部代理组延迟', async () => { for (const [name] of currentGroups()) { const result = await invoke<Record<string, number>>('test_group_delay', { name }); for (const [node, delay] of Object.entries(result)) delays.set(node, delay); } });
}), canRun);
groupHeader.append(sortButton, historyButton, hiddenButton, allDelay);
const proxyGroups = el('div', 'proxy-groups');
proxyPanel.append(traffic, configButton, systemRow, tunRow, terminalRow, providerHeader, providers, groupHeader, proxyGroups);

const rulePanel = panels.get('rules')!;
const ruleToolbar = el('div', 'compact-toolbar');
const ruleStats = el('span', 'muted');
const groupRulesButton = iconButton('list', '按策略分组', () => { groupRules = !groupRules; savePreference('group-rules', groupRules); renderRules(); });
const refreshRulesButton = gate(iconButton('refresh', '刷新规则', () => refresh()), canRun);
ruleToolbar.append(ruleStats, el('span', 'flex-space'), groupRulesButton, refreshRulesButton);
const ruleChips = el('div', 'filter-chips');
const ruleSearch = searchField('搜索目标、类型、策略...', 'rule-search', value => { ruleQuery = value; renderRules(); });
const policySelect = settingSelect('筛选策略', () => ['全部策略', ...new Set(snapshot?.rules.rules.map(rule => rule.proxy).sort() ?? [])], () => rulePolicy || '全部策略', value => { rulePolicy = value === '全部策略' ? '' : value; renderRules(); });
const ruleSearchRow = el('div', 'search-row'); ruleSearchRow.append(ruleSearch.wrap, policySelect.control);
const ruleContent = el('div', 'rule-content');
rulePanel.append(ruleToolbar, ruleChips, ruleSearchRow, ruleContent);

const connectionPanel = panels.get('connections')!;
const connectionToolbar = el('div', 'compact-toolbar');
const transportSelect = settingSelect('连接协议', ['全部协议', '仅 TCP', '仅 UDP', '其他协议'], () => transport || '全部协议', value => { transport = value === '全部协议' ? '' : value; localStorage.setItem('clashbar-transport', transport); transportSelect.update(); renderConnections(); });
const sortSelect = settingSelect('连接排序', ['默认顺序', '最新连接', '最早连接', '上传流量高到低', '下载流量高到低', '总流量高到低'], () => connectionSort, value => { connectionSort = value; localStorage.setItem('clashbar-connection-sort', value); sortSelect.update(); renderConnections(); });
const connectionFraction = el('span', 'count-badge');
const closeAll = gate(iconButton('x', '关闭全部连接', () => mutate('关闭全部连接', () => invoke('close_all_connections')), 'danger-text'), () => Boolean(status?.running && snapshot?.connections.connections.length));
connectionToolbar.append(transportSelect.control, sortSelect.control, el('span', 'flex-space'), connectionFraction, closeAll);
const connectionSearch = searchField('过滤域名、IP...', 'connection-search', value => { connectionQuery = value; renderConnections(); });
const connectionContent = el('div', 'connections-list'); connectionPanel.append(connectionToolbar, connectionSearch.wrap, connectionContent);

const logsPanel = panels.get('logs')!;
const sourceChips = el('div', 'filter-chips');
const logLevel = settingSelect('内核日志级别', ['silent', 'error', 'warning', 'info', 'debug'], () => snapshot?.configs['log-level'] || 'info', level => mutate('更改日志级别', () => invoke('set_log_level', { level })));
gate(logLevel.control, canRun);
const logFraction = el('span', 'count-badge');
const logTop = el('div', 'compact-toolbar'); logTop.append(sourceChips, el('span', 'flex-space'), logLevel.control, logFraction);
const levelChips = el('div', 'filter-chips');
const copyLogs = iconButton('copy', '复制全部日志', () => copyText(combinedLogs.map(item => item.line).join('\n'), '已复制全部日志'));
const clearLogs = gate(iconButton('trash', '清理全部日志', () => mutate('清理全部日志', async () => { await invoke('clear_logs'); logs = []; appLogs = []; combinedLogs = []; })), () => true);
const logActions = el('div', 'compact-toolbar'); logActions.append(levelChips, el('span', 'flex-space'), copyLogs, clearLogs);
const logSearch = searchField('搜索日志...', 'log-search', value => { logQuery = value; renderLogs(); });
const logContent = el('div', 'logs-list'); logContent.setAttribute('aria-label', '内核日志'); logsPanel.append(logTop, logActions, logSearch.wrap, logContent);

const settings = panels.get('settings')!;
settings.append(sectionHeading('基础设置', 'settings'));
for (const [label, symbol, message] of [['开机自启', 'power', '此版本尚未接入 Windows 登录启动。'], ['内核自启', 'play', '此版本需要手动启动内核。']]) settings.append(disabledSetting(label, symbol, message));
settings.append(disabledSetting('状态栏样式', 'chart', 'Windows 托盘不支持 macOS 状态栏速度文字。', '仅图标'));
settings.append(disabledSetting('界面语言', 'globe', '当前 Windows 界面为简体中文。', '中文'));
let appearance = localStorage.getItem('clashbar-appearance') || '跟随系统';
const appearanceSelect = settingSelect('外观模式', ['跟随系统', '浅色', '深色'], () => appearance, value => { appearance = value; localStorage.setItem('clashbar-appearance', value); applyAppearance(); appearanceSelect.update(); });
settings.append(settingRow('外观模式', 'sun', appearanceSelect.control));
const settingsLogLevel = settingSelect('设置内核日志级别', ['silent', 'error', 'warning', 'info', 'debug'], () => snapshot?.configs['log-level'] || 'info', level => mutate('更改日志级别', () => invoke('set_log_level', { level })));
gate(settingsLogLevel.control, canRun); settings.append(settingRow('日志级别', 'document', settingsLogLevel.control));
settings.append(disabledSetting('代理绕过', 'network', '此版本使用系统代理恢复机制，尚未提供绕过规则编辑。', '系统默认'));
settings.append(sectionHeading('内核设置', 'chip'));
for (const [label, symbol] of [['允许局域网', 'globe'], ['IPv6', 'network'], ['TCP 并发', 'link']]) settings.append(disabledSetting(label, symbol, '此版本尚未提供此项内核设置。'));
const coreButton = gate(button('选择内核', () => mutate('选择内核', () => invoke<Status>('choose_core')), 'value-select'), () => Boolean(status && !status.running));
settings.append(settingRow('mihomo 内核', 'chip', coreButton));
const corePath = el('p', 'settings-path'); settings.append(corePath);
const portForm = el('form', 'port-form'); portForm.noValidate = true; portForm.append(sectionHeading('代理端口', 'network'));
for (const label of ['HTTP 端口', 'SOCKS 端口']) portForm.append(disabledSetting(label, 'network', '通过混合端口同时提供 HTTP 和 SOCKS 服务。', '—'));
const mixed = portField('混合端口', 'mixed-port'), controller = portField('控制端口', 'controller-port');
portForm.append(mixed.row, controller.row);
for (const label of ['重定向端口', 'TProxy 端口']) portForm.append(disabledSetting(label, 'network', '此 Windows 版本未提供透明代理端口。', '—'));
const portError = el('p', 'field-message error-text'); portError.id = 'port-error';
for (const input of [mixed.input, controller.input]) { input.setAttribute('aria-describedby', portError.id); input.addEventListener('input', () => { portsDirty = true; portError.textContent = ''; input.setAttribute('aria-invalid', 'false'); clearTimeout(portSaveTimer); portSaveTimer = setTimeout(() => { void savePortSettings(false); }, 750); }); }
portForm.append(portError); portForm.addEventListener('submit', event => { event.preventDefault(); clearTimeout(portSaveTimer); void savePortSettings(true); });
async function savePortSettings(focusInvalid: boolean) {
  if (busy || !desktop || !status || status.running || !portsDirty) return;
  const message = validatePorts(mixed.input.value, controller.input.value); portError.textContent = message || ''; portError.classList.toggle('error-text', Boolean(message));
  for (const input of [mixed.input, controller.input]) input.setAttribute('aria-invalid', String(Boolean(message)));
  if (message) { if (focusInvalid) mixed.input.focus(); return; }
  const success = await mutate('保存端口', () => invoke<Status>('save_settings', { mixedPort: Number(mixed.input.value), controllerPort: Number(controller.input.value) }));
  if (success) { portsDirty = false; portError.textContent = '已保存'; portError.classList.remove('error-text'); }
}
settings.append(portForm, sectionHeading('系统维护', 'settings'));
const maintenance = el('div', 'maintenance-grid');
for (const label of ['清理 FakeIP 缓存', '清理 DNS 缓存', '更新 Geo 数据库', '打开内核目录']) maintenance.append(unsupported(button(label, () => {}, 'maintenance-action'), '此版本尚未提供该维护操作。'));
settings.append(maintenance);
const settingsHint = el('p', 'settings-hint', '灰色项目尚未接入 Windows。更换内核或端口前请先停止内核。'); settings.append(settingsHint);

function sectionHeading(label: string, symbol: string) { const row = el('div', 'section-heading'); row.append(icon(symbol), el('h2', '', label), el('span', 'flex-space')); return row; }
function settingRow(label: string, symbol: string, control: HTMLElement) { const row = el('div', 'setting-row'); row.append(icon(symbol), el('span', 'setting-label', label), control); return row; }
function disabledSetting(label: string, symbol: string, explanation: string, text?: string) {
  const control = text ? unsupported(button(text, () => {}, 'value-select'), explanation) : unsupported(el('input', 'switch'), explanation);
  if (control instanceof HTMLInputElement) control.type = 'checkbox'; control.setAttribute('aria-label', `${label}，尚未提供`);
  const row = settingRow(label, symbol, control); row.classList.add('unsupported-row'); row.title = explanation; return row;
}
function portField(label: string, id: string) {
  const input = gate(el('input', 'port-input'), () => Boolean(status && !status.running)); input.type = 'number'; input.id = id; input.min = '1024'; input.max = '65535'; input.step = '1';
  const row = el('div', 'setting-row'); const caption = el('label', 'setting-label', label); caption.htmlFor = id; row.append(icon('network'), caption, input); return { row, input };
}
function applyAppearance() { document.documentElement.dataset.appearance = appearance === '深色' ? 'dark' : appearance === '浅色' ? 'light' : 'system'; }

function openConfigMenu(focus = true) {
  const choices: MenuChoice[] = (status?.profiles ?? []).map(profile => ({ label: profile.name, checked: profile.id === status?.activeProfileId, action: () => mutate('切换配置', () => invoke<Status>('select_profile', { id: profile.id })) }));
  if (!choices.length && status?.configName) choices.push({ label: status.configName, checked: true, action: () => {} });
  choices.push({ label: '', kind: 'separator', action: () => {} }, { label: '导入配置文件...', action: () => mutate('导入配置', () => invoke<Status>('import_config')) }, { label: '导入订阅链接...', action: () => openSubscription() });
  choose(configButton, '配置文件', choices, focus);
}
function openSubscription() {
  const trigger = document.activeElement as HTMLElement | null;
  const dialog = el('dialog', 'dialog subscription-dialog'); dialog.setAttribute('aria-labelledby', 'subscription-title');
  const title = el('h2', '', '导入订阅链接'); title.id = 'subscription-title';
  const form = el('form'); form.noValidate = true;
  const url = field('订阅链接', 'subscription-url', 'password'); url.input.autocomplete = 'off'; url.input.spellcheck = false; url.input.placeholder = 'https://…';
  const reveal = button('显示链接', () => { const visible = url.input.type === 'password'; url.input.type = visible ? 'text' : 'password'; reveal.textContent = visible ? '隐藏链接' : '显示链接'; reveal.setAttribute('aria-pressed', String(visible)); }, 'small-action'); reveal.setAttribute('aria-pressed', 'false');
  const cancel = button('取消', () => dialog.close(), 'small-action'); const submit = button('导入订阅', () => {}, 'small-action primary'); submit.type = 'submit';
  const actions = el('div', 'dialog-actions'); actions.append(reveal, el('span', 'flex-space'), cancel, submit);
  form.append(url.wrapper, el('p', 'muted', '使用 HTTPS 链接导入；已有配置会保留，可随时切换。'), actions); dialog.append(title, form); document.body.append(dialog);
  form.addEventListener('submit', event => {
    event.preventDefault(); if (submit.disabled) return; const validation = validateSubscription(url.input.value); setFieldError(url.input, url.error, validation);
    if (validation) { url.input.focus(); return; }
    submit.disabled = cancel.disabled = true; submit.setAttribute('aria-busy', 'true');
    void mutate('导入订阅', () => invoke<Status>('import_subscription', { url: url.input.value.trim() })).then(success => { if (success) dialog.close(); else { setFieldError(url.input, url.error, actionError); url.input.focus(); } submit.disabled = cancel.disabled = false; submit.setAttribute('aria-busy', 'false'); });
  });
  dialog.addEventListener('cancel', event => { if (submit.disabled) event.preventDefault(); });
  dialog.addEventListener('keydown', event => {
    if (event.key !== 'Tab') return; const controls = [...dialog.querySelectorAll<HTMLElement>('input:not(:disabled),button:not(:disabled)')];
    if (event.shiftKey && document.activeElement === controls[0]) { event.preventDefault(); controls.at(-1)?.focus(); }
    else if (!event.shiftKey && document.activeElement === controls.at(-1)) { event.preventDefault(); controls[0]?.focus(); }
  });
  dialog.addEventListener('close', () => { dialog.remove(); (trigger?.isConnected ? trigger : configButton).focus(); }, { once: true }); dialog.showModal(); url.input.focus();
}

function updateChrome() {
  for (const [control, enabled] of gated) { if (!control.isConnected) { gated.delete(control); continue; } control.disabled = !desktop || busy || !enabled(); }
  retryButton.disabled = !desktop || busy || refreshing;
  setIcon(runButton, status?.running ? 'stop' : 'play', status?.running ? '停止内核' : '启动内核'); runButton.setAttribute('aria-busy', String(busy));
  runButton.classList.toggle('orange', Boolean(status?.running)); runButton.classList.toggle('green', !status?.running);
  restartButton.title = status?.running ? '重启内核' : '启动内核';
  pinButton.classList.toggle('selected', pinned); pinButton.setAttribute('aria-pressed', String(pinned)); pinButton.setAttribute('aria-label', pinned ? '取消固定' : '固定面板');
  endpointText.textContent = `127.0.0.1:${status?.controllerPort ?? 9090}`; endpoint.setAttribute('aria-label', `本机控制器，${status?.running ? '运行中' : '已停止'}`); statusDot.dataset.state = status?.running ? 'running' : 'stopped';
  configName.textContent = status?.configName || '未导入'; configName.title = configName.textContent; systemProxy.checked = status?.systemProxy ?? false;
  coreVersion.querySelector('span')!.textContent = `MIHOMO ${status?.version ? `v${status.version.replace(/^v/, '')}` : '—'}`;
  corePath.textContent = status?.corePath || '尚未选择 mihomo.exe';
  if (status && !portsDirty) { mixed.input.value = String(status.mixedPort); controller.input.value = String(status.controllerPort); }
  const error = actionError || refreshError || (status?.lastError ? safeError(status.lastError) : ''); banner.hidden = !error; errorCopy.textContent = error;
  if (busy) notice.textContent = `${pendingLabel}…`;
  for (const [mode, control] of modeButtons) { control.setAttribute('aria-pressed', String(snapshot?.configs.mode?.toLowerCase() === mode)); }
  sortButton.setAttribute('aria-pressed', String(sortLatency)); historyButton.setAttribute('aria-pressed', String(showHistory)); hiddenButton.setAttribute('aria-pressed', String(!hideHidden)); groupRulesButton.setAttribute('aria-pressed', String(groupRules));
  settingsLogLevel.update(); logLevel.update(); requestResize();
}
function activateTab(tab: Tab) {
  scrollPositions.set(activeTab, content.scrollTop); activeTab = tab; void dismissMenu();
  shell.dataset.layout = ['proxies', 'settings'].includes(tab) ? 'dynamic' : 'fixed';
  for (const [key, panel] of panels) panel.hidden = key !== tab;
  for (const [key, control] of tabButtons) { control.setAttribute('aria-selected', String(key === tab)); control.tabIndex = key === tab ? 0 : -1; }
  document.title = `ClashBar · ${tabs.find(([key]) => key === tab)![1]}`;
  renderActive(); content.scrollTop = scrollPositions.get(tab) ?? 0; if (desktop && !busy) void refresh();
}
function renderActive() { if (activeTab === 'proxies') renderProxies(); if (activeTab === 'rules') renderRules(); if (activeTab === 'connections') renderConnections(); if (activeTab === 'logs') renderLogs(); updateChrome(); }
function stopped() { return emptyState(status?.running ? '正在读取数据' : '内核未启动', status?.running ? '读取失败时可重新刷新。' : status?.corePath && status?.configName ? '点击顶部启动按钮。' : '选择 mihomo 内核并导入配置后启动。'); }
function currentGroups() { return Object.entries(snapshot?.proxies.proxies ?? {}).filter(([name, proxy]) => proxy.all && (!hideHidden || !proxy.hidden) && (name !== 'GLOBAL' || snapshot?.configs.mode === 'global')); }
function nodeDelay(name: string, proxy?: Proxy) { return delays.get(name) ?? snapshot?.proxies.proxies[name]?.history?.at(-1)?.delay ?? proxy?.history?.at(-1)?.delay; }
function delayText(delay?: number) { return delay == null ? '—' : delay === 0 ? '超时' : String(delay); }
async function testNode(name: string) { await mutate('测试延迟', async () => { const result = await invoke<{ delay: number }>('test_delay', { name }); delays.set(name, result.delay); await updateMenuValue(name, delayText(result.delay)); }); }
function groupMenu(anchor: HTMLElement, name: string, group: Proxy, focus = true) {
  let names = [...new Set(group.all ?? [])];
  if (sortLatency) names = names.sort((a, b) => (nodeDelay(a) || Infinity) - (nodeDelay(b) || Infinity));
  choose(anchor, `${name}  ${names.length}`, names.map(node => ({ label: node, detail: snapshot?.proxies.proxies[node]?.type, value: delayText(nodeDelay(node)), checked: group.now === node, action: () => mutate('切换节点', () => invoke('select_proxy', { group: name, name: node })), secondaryLabel: `测试 ${node} 延迟`, secondaryAction: () => testNode(node) })), focus);
}
function renderProxies() {
  renderTraffic(); const groups = currentGroups(); groupCounter.textContent = String(groups.length);
  const groupNodes = groups.map(([name, group], index) => {
    const row = el('div', 'proxy-group-row');
    const trigger: HTMLButtonElement = gate(button('', () => groupMenu(trigger, name, group), 'group-trigger'), canRun); trigger.id = `group-${index}`; trigger.setAttribute('aria-label', `代理组 ${name}`); trigger.setAttribute('aria-haspopup', 'dialog');
    trigger.addEventListener('pointerenter', () => { if (!trigger.disabled) scheduleMenuOpen(() => groupMenu(trigger, name, group, false)); });
    row.addEventListener('pointerleave', scheduleMenuClose);
    const title = el('span', 'group-name', name); title.title = name;
    const selected = el('span', 'selected-node capsule', group.now || '—'); selected.title = group.now || '';
    const delay = nodeDelay(group.now || name, group); const latency = el('span', 'latency', delayText(delay)); latency.dataset.tone = delay === 0 ? 'error' : delay == null ? 'unknown' : delay > 500 ? 'warning' : 'success';
    trigger.append(title, selected, latency);
    if (showHistory && group.history?.length) { const history = el('span', 'history-bars'); for (const sample of group.history.slice(-8)) { const bar = el('i'); bar.style.height = `${Math.min(14, Math.max(2, sample.delay / 40))}px`; history.append(bar); } latency.replaceChildren(history, document.createTextNode(delayText(delay))); }
    const test = gate(iconButton('gauge', `测试 ${name} 分组延迟`, () => mutate('测试分组延迟', async () => { const result = await invoke<Record<string, number>>('test_group_delay', { name }); for (const [node, value] of Object.entries(result)) delays.set(node, value); })), canRun); test.id = `delay-${index}`;
    row.append(trigger, test); return row;
  });
  if (!menuIsOpen()) replacePreservingFocus(proxyGroups, ...(status?.running && snapshot ? groupNodes.length ? groupNodes : [emptyState('暂无代理组', '当前配置没有可用的代理组。')] : [stopped()]));
  const entries = Object.entries(snapshot?.providers.providers ?? {}); providerCounter.textContent = String(entries.length); providers.hidden = providerCollapsed;
  setIcon(collapseProviders, providerCollapsed ? 'chevron' : 'down', providerCollapsed ? '展开代理提供者' : '折叠代理提供者'); collapseProviders.setAttribute('aria-expanded', String(!providerCollapsed));
  const providerNodes = entries.map(([name, provider], index) => {
    const row = el('div', 'provider-item'); const top = el('div', 'provider-row'); top.append(icon('drive', 'teal'), el('strong', 'provider-name', name), el('span', 'muted', String(provider.proxies?.length ?? 0)), el('span', 'flex-space'));
    const updated = provider.updatedAt ? relativeTime(provider.updatedAt) : '—'; top.append(el('span', 'provider-age', updated));
    const update = gate(iconButton('refresh', `更新代理提供者 ${name}`, () => mutate('更新代理提供者', () => invoke('update_provider', { name }))), canRun); update.id = `provider-${index}`; top.append(update); row.append(top);
    const info = provider.subscriptionInfo; if (info && info.total > 0) { const usage = el('div', 'provider-usage'); const labels = el('div', 'usage-labels'); const days = info.expire ? Math.ceil((info.expire * 1000 - Date.now()) / 86400000) : null; labels.append(el('span', 'orange', days == null ? '不限时' : days < 0 ? '已过期' : `剩 ${days} 天`), el('span', '', `${compactBytes(info.upload + info.download)} / ${compactBytes(info.total)}`)); const progress = el('progress'); progress.max = info.total; progress.value = info.upload + info.download; progress.setAttribute('aria-label', `${name} 订阅用量`); usage.append(labels, progress); row.append(usage); }
    return row;
  });
  replacePreservingFocus(providers, ...(providerNodes.length ? providerNodes : [el('p', 'compact-empty', '暂无代理提供者')])); updateChrome();
}
function compactBytes(bytes?: number) { return formatBytes(bytes).replace('KiB', 'KB').replace('MiB', 'MB').replace('GiB', 'GB').replace('TiB', 'TB'); }
function relativeTime(raw: string) { const seconds = Math.max(0, Math.floor((Date.now() - Date.parse(raw)) / 1000)); if (!Number.isFinite(seconds)) return '—'; return seconds < 60 ? '刚刚' : seconds < 3600 ? `${Math.floor(seconds / 60)}m前` : `${Math.floor(seconds / 3600)}h前`; }
function renderTraffic() {
  const data = status?.running ? snapshot : null;
  connectionCount.replaceChildren(icon('link', 'purple'), document.createTextNode(data ? String(data.connections.connections.length) : '—'));
  memoryMetric.replaceChildren(icon('chip', 'teal'), document.createTextNode(data ? compactBytes(data.memory ?? undefined) : '—'));
  upMetric.replaceChildren(document.createTextNode(`${rateUp == null ? '—' : compactBytes(rateUp) + '/s'} · ${data ? compactBytes(data.connections.uploadTotal) : '—'}`), icon('up', 'blue'));
  downMetric.replaceChildren(document.createTextNode(`${rateDown == null ? '—' : compactBytes(rateDown) + '/s'} · ${data ? compactBytes(data.connections.downloadTotal) : '—'}`), icon('arrowDown', 'green'));
  chart.replaceChildren(); const maximum = Math.max(1, ...trafficSamples.flatMap(sample => [sample.up, sample.down]));
  for (const direction of ['up', 'down'] as const) { const line = document.createElementNS(chart.namespaceURI, 'polyline'); const points = trafficSamples.map((sample, i) => `${336 - (trafficSamples.length - 1 - i) / 59 * 336},${30 + (direction === 'up' ? -1 : 1) * sample[direction] / maximum * 27}`).join(' '); line.setAttribute('points', points); line.setAttribute('class', `traffic-line ${direction}`); chart.append(line); }
}

function classifyRule(rule: Rule) { const type = rule.type.toLowerCase(); return type.includes('domain') ? '域名' : type.includes('ip') || type.includes('cidr') || type.includes('geoip') ? 'IP' : type.includes('ruleset') || type.includes('rule-set') ? '规则集' : '其他'; }
function renderRules() {
  const all = snapshot?.rules.rules ?? []; const base = all.filter(rule => matchesQuery([rule.type, rule.payload, rule.proxy], ruleQuery) && (!rulePolicy || rule.proxy === rulePolicy));
  const filtered = ruleType === '全部' ? base : base.filter(rule => classifyRule(rule) === ruleType);
  ruleStats.textContent = `规则 ${all.length}  规则集 ${all.filter(rule => classifyRule(rule) === '规则集').length}`; policySelect.update();
  ruleChips.replaceChildren(...['全部', '域名', 'IP', '规则集', '其他'].map(type => { const count = type === '全部' ? base.length : base.filter(rule => classifyRule(rule) === type).length; const chip = button(`${type} ${count}`, () => { ruleType = type; renderRules(); }, 'filter-chip'); chip.setAttribute('aria-pressed', String(ruleType === type)); return chip; }));
  if (!status?.running || !snapshot) { ruleContent.replaceChildren(stopped()); return; }
  if (!filtered.length) { ruleContent.replaceChildren(emptyState(all.length ? '无匹配规则' : '暂无规则数据', all.length ? '修改或清除筛选条件。' : '当前配置没有路由规则。')); return; }
  const head = el('div', 'rule-head'); head.append(el('span', '', '目标 / 类型'), el('span', '', '策略'), el('span', '', '统计'));
  if (groupRules) {
    const groups = new Map<string, Rule[]>(); for (const rule of filtered) groups.set(rule.proxy, [...(groups.get(rule.proxy) ?? []), rule]);
    const nodes = [...groups.entries()].sort((a, b) => b[1].length - a[1].length || a[0].localeCompare(b[0], 'zh-CN')).map(([policy, rules]) => {
      const group = el('section', 'policy-group'); const toggle = button('', () => { if (expandedPolicies.has(policy)) expandedPolicies.delete(policy); else expandedPolicies.add(policy); renderRules(); }, 'policy-heading'); toggle.append(icon(expandedPolicies.has(policy) ? 'down' : 'chevron'), el('strong', '', policy), el('span', 'count-badge', String(rules.length))); toggle.setAttribute('aria-expanded', String(expandedPolicies.has(policy))); group.append(toggle); if (expandedPolicies.has(policy)) group.append(ruleList(rules)); return group;
    }); ruleContent.replaceChildren(head, ...nodes);
  } else ruleContent.replaceChildren(head, ruleList(filtered));
  updateChrome();
}
function ruleList(rules: Rule[]) {
  const host = el('div', 'virtual-rules'); host.style.height = `${rules.length * 32}px`; host.setAttribute('role', 'table'); host.setAttribute('aria-label', '路由规则'); host.setAttribute('aria-rowcount', String(rules.length));
  let previousStart = -1;
  const render = () => {
    if (!host.isConnected) return; const offset = content.getBoundingClientRect().top - host.getBoundingClientRect().top;
    const start = Math.max(0, Math.floor(offset / 32) - 6); if (start === previousStart) return; previousStart = start;
    const rows = rules.slice(start, start + 50).map((rule, i) => { const row = el('div', 'rule-row'); row.style.top = `${(start + i) * 32}px`; row.setAttribute('role', 'row'); row.setAttribute('aria-rowindex', String(start + i + 1)); const target = el('span', 'rule-target'); target.setAttribute('role', 'cell'); target.append(icon(classifyRule(rule) === 'IP' ? 'network' : 'globe', 'muted-icon')); const text = el('span'); const name = el('span', 'truncate', rule.payload || '—'); name.title = rule.payload; text.append(name, el('small', '', rule.type)); target.append(text); const policy = el('span', 'rule-policy truncate', rule.proxy); policy.setAttribute('role', 'cell'); policy.title = rule.proxy; const count = el('span', 'rule-stat muted', '—'); count.setAttribute('role', 'cell'); row.append(target, policy, count); return row; }); host.replaceChildren(...rows);
  };
  const onScroll = () => { if (!host.isConnected) content.removeEventListener('scroll', onScroll); else render(); }; content.addEventListener('scroll', onScroll, { passive: true }); requestAnimationFrame(render); return host;
}

function connectionName(connection: Connection) { return connection.metadata.host || connection.metadata.destinationIP || connection.id; }
function renderConnections() {
  if (!status?.running || !snapshot) { connectionFraction.textContent = '0/0'; connectionContent.replaceChildren(stopped()); return; }
  const all = snapshot.connections.connections ?? []; let visible = all.slice(0, 120).filter(connection => {
    const net = connection.metadata.network?.toUpperCase(); const transportMatch = !transport || (transport === '仅 TCP' ? net === 'TCP' : transport === '仅 UDP' ? net === 'UDP' : !['TCP', 'UDP'].includes(net || ''));
    return transportMatch && matchesQuery([connectionName(connection), connection.id, connection.metadata.sourceIP, connection.metadata.destinationIP, connection.metadata.process, connection.rule, connection.rulePayload, ...(connection.chains ?? [])], connectionQuery);
  });
  if (connectionSort !== '默认顺序') visible = visible.sort((a, b) => connectionSort === '最新连接' ? Date.parse(b.start || '') - Date.parse(a.start || '') : connectionSort === '最早连接' ? Date.parse(a.start || '') - Date.parse(b.start || '') : connectionSort === '上传流量高到低' ? (b.upload ?? 0) - (a.upload ?? 0) : connectionSort === '下载流量高到低' ? (b.download ?? 0) - (a.download ?? 0) : (b.upload ?? 0) + (b.download ?? 0) - (a.upload ?? 0) - (a.download ?? 0));
  connectionFraction.textContent = `${visible.length}/${Math.min(all.length, 120)}`;
  const rows = visible.map(connection => {
    const row = el('article', 'connection-row');
    const leading = icon('globe', 'blue'); const body = el('div', 'connection-body'); const top = el('div', 'connection-top');
    const name = el('strong', 'truncate', connectionName(connection)); name.title = connectionName(connection); top.append(name, el('span', 'capsule rule-badge', connection.rule || '—'));
    const metrics = el('div', 'connection-metrics'); metrics.append(el('span', '', connection.start ? timeText(connection.start) : '—'), el('span', '', connection.metadata.network?.toUpperCase() || '—'), el('span', 'blue', `↑ ${compactBytes(connection.upload)}`), el('span', 'green', `↓ ${compactBytes(connection.download)}`));
    const chains = el('div', 'connection-chains truncate', [...(connection.chains ?? [])].reverse().join(' › ') || '—'); chains.title = chains.textContent || ''; body.append(top, metrics, chains);
    const close = gate(iconButton('x', `关闭连接 ${connectionName(connection)}`, () => closeConnection(connection), 'row-close danger-text'), canRun); close.id = `close-${connection.id}`;
    row.append(leading, body, close); row.addEventListener('contextmenu', event => { event.preventDefault(); choose(row, connectionName(connection), [{ label: '关闭连接', action: () => closeConnection(connection) }, { label: '复制 Host', action: () => copyText(connectionName(connection), '已复制 Host') }, { label: '复制连接 ID', action: () => copyText(connection.id, '已复制连接 ID') }]); }); return row;
  });
  replacePreservingFocus(connectionContent, ...(rows.length ? rows : [emptyState(all.length ? '无匹配连接' : '暂无活动连接', all.length ? '修改或清除筛选条件。' : '通过代理访问网络后，连接会显示在这里。')])); updateChrome();
}
function closeConnection(connection: Connection) { return mutate('关闭连接', () => invoke('close_connection', { id: connection.id })); }
function timeText(value: string) { const date = new Date(value); return Number.isFinite(date.valueOf()) ? date.toLocaleTimeString('zh-CN', { hour12: false }) : '—'; }
function logSeverity(line: string) { return /\b(error|fatal|panic)\b/i.test(line) ? '错误' : /\bwarn(ing)?\b/i.test(line) ? '警告' : '信息'; }
function renderLogs() {
  sourceChips.replaceChildren(...['全部', 'ClashBar', 'Mihomo'].map(source => { const chip = button(source, () => { if (source === '全部') sourceFilter.clear(); else if (sourceFilter.has(source)) sourceFilter.delete(source); else sourceFilter.add(source); renderLogs(); }, 'filter-chip'); chip.setAttribute('aria-pressed', String(source === '全部' ? !sourceFilter.size : sourceFilter.has(source))); return chip; }));
  levelChips.replaceChildren(...['全部', '信息', '警告', '错误'].map(level => { const chip = button(level, () => { if (level === '全部') levelFilter.clear(); else if (levelFilter.has(level)) levelFilter.delete(level); else levelFilter.add(level); renderLogs(); }, 'filter-chip'); chip.setAttribute('aria-pressed', String(level === '全部' ? !levelFilter.size : levelFilter.has(level))); return chip; }));
  const all = [...combinedLogs].reverse();
  const visible = all.slice(0, 120).filter(item => (!sourceFilter.size || sourceFilter.has(item.source)) && (!levelFilter.size || levelFilter.has(logSeverity(item.line))) && matchesQuery([item.line], logQuery));
  logFraction.textContent = `${visible.length}/${all.length}`;
  const rows = visible.map(item => { const row = el('article', 'log-row'); const level = logSeverity(item.line); row.append(icon(level === '错误' ? 'x' : level === '警告' ? 'warning' : 'info', level === '错误' ? 'danger-text' : level === '警告' ? 'orange' : 'blue')); const body = el('div', 'log-body'); const timestamp = timeText(new Date(item.timestamp).toISOString()); const protocol = item.line.match(/\[(TCP|UDP)\]/)?.[1]; body.append(el('div', 'log-meta', [item.source, protocol, timestamp].filter(Boolean).join(' • ')), el('p', 'log-message', item.line)); row.append(body); row.addEventListener('contextmenu', event => { event.preventDefault(); choose(row, '日志', [{ label: '复制日志', action: () => copyText(item.line, '已复制日志') }]); }); return row; });
  logContent.replaceChildren(...(rows.length ? rows : [emptyState(logQuery || sourceFilter.size || levelFilter.size ? '无匹配日志' : '暂无日志', logQuery || sourceFilter.size || levelFilter.size ? '修改或清除筛选条件。' : '启动内核后查看日志。')])); updateChrome();
}
async function copyText(text: string, feedback: string) { try { await navigator.clipboard.writeText(text); notice.textContent = feedback; return true; } catch { actionError = '复制失败，请检查剪贴板权限后重试。'; updateChrome(); return false; } }
function sampleTraffic(next: Snapshot) {
  const now = performance.now(); if (lastSample && now > lastSample.time) { const seconds = (now - lastSample.time) / 1000; rateUp = Math.max(0, (next.connections.uploadTotal - lastSample.up) / seconds); rateDown = Math.max(0, (next.connections.downloadTotal - lastSample.down) / seconds); trafficSamples.push({ up: rateUp, down: rateDown }); if (trafficSamples.length > 60) trafficSamples.shift(); }
  lastSample = { time: now, up: next.connections.uploadTotal, down: next.connections.downloadTotal };
}
async function refresh() {
  if (!desktop || busy || refreshing || document.hidden || !popupVisible) return;
  const ticket = epoch.next(); refreshing = true; updateChrome();
  try {
    const nextStatus = await invoke<Status>('get_status'); if (!epoch.current(ticket)) return; status = nextStatus;
    if (!status.running) { snapshot = null; lastSample = null; rateUp = rateDown = null; trafficSamples.length = 0; }
    else { const next = await invoke<Snapshot>('get_snapshot'); if (!epoch.current(ticket)) return; sampleTraffic(next); snapshot = next; }
    if (activeTab === 'logs') {
      const nextLogs = await invoke<{ timestamp: number; source: string; message: string }[]>('get_log_entries'); if (!epoch.current(ticket)) return;
      logs = nextLogs.slice(-LOG_LIMIT).map(item => ({ timestamp: item.timestamp, source: item.source, line: item.message }));
      combinedLogs = [...appLogs, ...logs].sort((a, b) => a.timestamp - b.timestamp).slice(-LOG_LIMIT);
    }
    refreshError = '';
  } catch (error) { if (epoch.current(ticket)) refreshError = `读取失败，显示上次数据。${safeError(error)}`; }
  finally { if (epoch.current(ticket)) { refreshing = false; renderActive(); } }
}
async function mutate(label: string, operation: () => Promise<unknown>, reload = true): Promise<boolean> {
  if (!desktop || busy) return false; epoch.next(); refreshing = false; busy = true; pendingLabel = label; actionError = ''; updateChrome();
  let success = false;
  try { const result = await operation(); if (result && typeof result === 'object' && 'running' in result) status = result as Status; success = true; }
  catch (error) { actionError = `${label}未完成。${safeError(error)}`; }
  finally { busy = false; pendingLabel = ''; if (label !== '清理全部日志' || !success) { const record = { timestamp: Date.now(), source: 'ClashBar', line: `${success ? 'info' : 'error'} ${label}${success ? '已完成' : '未完成'}` }; appLogs.push(record); appLogs = appLogs.slice(-LOG_LIMIT); combinedLogs.push(record); combinedLogs = combinedLogs.slice(-LOG_LIMIT); } renderActive(); }
  if (reload) await refresh(); if (success) notice.textContent = `${label}已完成。`; return success;
}
function requestResize() {
  if (!desktop || !['proxies', 'settings'].includes(activeTab)) return; clearTimeout(resizeTimer); resizeTimer = setTimeout(() => {
    if (!['proxies', 'settings'].includes(activeTab)) return;
    const desired = Math.min(window.screen.availHeight || 900, Math.ceil(header.offsetHeight + modes.offsetHeight + navigation.offsetHeight + footer.offsetHeight + panels.get(activeTab)!.scrollHeight + (banner.hidden ? 0 : banner.offsetHeight) + 3));
    if (Math.abs(desired - lastRequestedHeight) < 2) return; lastRequestedHeight = desired;
    void invoke('resize_popup', { height: desired }).catch(() => { lastRequestedHeight = 0; });
  }, 30);
}
function resetEphemeralState() { ruleQuery = connectionQuery = logQuery = ''; ruleSearch.input.value = connectionSearch.input.value = logSearch.input.value = ''; rulePolicy = ''; ruleType = '全部'; sourceFilter.clear(); levelFilter.clear(); expandedPolicies.clear(); scrollPositions.clear(); content.scrollTop = 0; void dismissMenu(); }
async function initializeNative() {
  if (!desktop) return;
  await initializeMenus();
  pinned = await invoke<boolean>('get_popup_pinned');
  unlisten.push(await listen<{ visible: boolean; pinned: boolean }>('popup-visibility', event => { const reopening = !popupVisible && event.payload.visible; popupVisible = event.payload.visible; pinned = event.payload.pinned; if (reopening) { resetEphemeralState(); lastSample = null; void refresh(); } updateChrome(); }));
  unlisten.push(await listen<{ tab: string }>('popup-tab', event => { if (event.payload.tab === 'system') activateTab('settings'); }));
  updateChrome();
}
document.addEventListener('keydown', event => { if (event.key === 'Escape' && !event.isComposing && !event.defaultPrevented && !document.querySelector('dialog[open]')) { event.preventDefault(); if (menuIsOpen()) void dismissMenu(); else if (desktop) void invoke('hide_popup'); } });
window.addEventListener('beforeunload', event => { if (portsDirty) { event.preventDefault(); event.returnValue = ''; } });
document.addEventListener('visibilitychange', () => { if (!document.hidden) void refresh(); });
window.addEventListener('unload', () => { for (const cleanup of unlisten) cleanup(); });
new ResizeObserver(requestResize).observe(shell);
applyAppearance(); activateTab('proxies'); void initializeNative().catch(error => { actionError = `托盘连接失败。${safeError(error)}`; updateChrome(); });
setInterval(() => { void refresh(); }, 3000);
