import { t, locale, localizedPreference } from './i18n';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { AppReleaseInfo, Connection, Proxy, Rule, RuleProvider, Snapshot, Status, SubscriptionSummary, Tab } from './types';
import { formatBytes, LOG_LIMIT, matchesQuery, ReadEpoch, safeError, terminalProxyCommand, validateLocalPorts, validateRemotePorts, validateSubscription } from './utils';
import { button, el, emptyState, field, icon, iconButton, presentDialog, replacePreservingFocus, searchField, setFieldError } from './ui';
import { dismissMenu, initializeMenus, menuIsOpen, scheduleMenuClose, scheduleMenuOpen, showMenu, updateMenuValue, type MenuChoice } from './menu';
import logoUrl from './assets/logo.png';
import { openMachineManager } from './remote';
const desktop = isTauri();
const epoch = new ReadEpoch();
const tabs: [
    Tab,
    string
][] = [['proxies', t("节点")], ['rules', t("分流")], ['connections', t("连接")], ['logs', t("日志")], ['settings', t("设置")]];
let status: Status | null = null, snapshot: Snapshot | null = null;
let activeTab: Tab = 'proxies', busy = false, refreshing = false, popupVisible = true, pinned = false;
let actionError = '', refreshError = '', pendingLabel = '';
type LogRow = {
    timestamp: number;
    source: string;
    line: string;
};
let logs: LogRow[] = [], appLogs: LogRow[] = [];
let ruleQuery = '', ruleType = t("全部"), rulePolicy = '', connectionQuery = '', logQuery = '';
const sourceFilter = new Set<string>(), levelFilter = new Set<string>();
let combinedLogs: LogRow[] = [];
let providerCollapsed = preference('providers-collapsed', false), sortLatency = preference('sort-latency', false), showHistory = preference('show-history', false), hideHidden = preference('hide-hidden', true), groupRules = preference('group-rules', false);
let transport = localizedPreference(localStorage.getItem('clashbar-transport') || ''), connectionSort = localizedPreference(localStorage.getItem('clashbar-connection-sort') || '默认顺序');
const expandedPolicies = new Set<string>();
const delays = new Map<string, number>();
const groupIcons = new Map<string, Promise<string | null>>();
const gated = new Map<HTMLButtonElement | HTMLInputElement, () => boolean>();
let lastSample: {
    time: number;
    up: number;
    down: number;
} | null = null;
const trafficSamples: {
    up: number;
    down: number;
}[] = [];
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
function isLocal() { return !status?.activeRemoteId; }
function setIcon(control: HTMLButtonElement, symbol: string, label: string) { control.replaceChildren(icon(symbol)); control.setAttribute('aria-label', label); control.title = label; }
function choose(anchor: HTMLElement, title: string, choices: MenuChoice[], focus = true) { void showMenu(anchor, title, choices, focus).catch(error => { actionError = t("无法打开菜单。{0}", safeError(error)); updateChrome(); }); }
function settingSelect(label: string, items: string[] | (() => string[]), value: () => string, change: (next: string) => void | Promise<unknown>) {
    const choices = () => (typeof items === 'function' ? items() : items).map(name => ({ label: name, checked: value() === name, action: () => change(name) }));
    const control = button('', () => choose(control, label, choices()), 'value-select');
    control.addEventListener('pointerenter', () => { if (!control.disabled)
        scheduleMenuOpen(() => choose(control, label, choices(), false)); });
    control.addEventListener('pointerleave', scheduleMenuClose);
    const update = () => { control.replaceChildren(el('span', '', value()), icon('down')); control.setAttribute('aria-label', label); control.title = value(); };
    update();
    return { control, update };
}
const app = document.querySelector<HTMLDivElement>('#app')!;
const shell = el('main', 'menu-panel');
const header = el('header', 'app-header');
const logo = el('img', 'brand-logo');
logo.src = logoUrl;
logo.alt = 'ClashBar';
logo.width = logo.height = 40;
const identity = el('div', 'identity');
identity.append(el('h1', '', 'ClashBar'));
const endpointRow = el('div', 'endpoint-row');
const endpoint = gate(button('', () => openEndpointMenu(), 'endpoint-button'), () => Boolean(status));
const statusDot = el('span', 'status-dot');
const endpointText = el('span', '', '127.0.0.1:9090');
endpoint.append(statusDot, endpointText, icon('down'));
const webUI = gate(iconButton('globe', t("打开 WebUI"), () => openWebUI()), canRun);
endpointRow.append(endpoint, webUI);
identity.append(endpointRow);
endpoint.addEventListener('pointerenter', () => { if (!endpoint.disabled)
    scheduleMenuOpen(() => openEndpointMenu(false)); });
endpoint.addEventListener('pointerleave', scheduleMenuClose);
const headerActions = el('div', 'header-actions');
const pinButton = gate(iconButton('pin', t("固定面板"), async () => { const next = !pinned; if (await mutate(next ? t("固定面板") : t("取消固定"), () => invoke('set_popup_pinned', { pinned: next }), false)) {
    pinned = next;
    updateChrome();
} }), () => true);
const restartButton = gate(iconButton('restart', t("重启内核"), () => mutate(status?.running ? t("重启内核") : t("启动内核"), () => invoke<Status>(status?.running ? 'restart_core' : 'start_core'))), () => isLocal() && Boolean(status?.corePath && status.configName));
const runButton = gate(iconButton('play', t("启动内核"), () => mutate(status?.running ? t("停止内核") : t("启动内核"), () => invoke<Status>(status?.running ? 'stop_core' : 'start_core'))), () => isLocal() && Boolean(status && (status.running || (status.corePath && status.configName))));
const quitButton = gate(iconButton('power', t("退出 ClashBar"), () => invoke('quit_app')), () => true);
headerActions.append(pinButton, restartButton, runButton, quitButton);
header.append(logo, identity, headerActions);
const modes = el('div', 'mode-segments');
modes.setAttribute('role', 'group');
modes.setAttribute('aria-label', t("代理模式"));
const modeButtons = new Map<string, HTMLButtonElement>();
for (const [value, label, symbol] of [['rule', t("规则"), 'shield'], ['global', t("全局"), 'globe'], ['direct', t("直连"), 'bolt']]) {
    const mode = gate(button('', () => { if (snapshot?.configs.mode !== value)
        return mutate(t("切换代理模式"), () => invoke('set_mode', { mode: value })); }, 'mode-button'), () => Boolean(status?.running && snapshot));
    mode.append(icon(symbol), el('span', '', label));
    mode.setAttribute('aria-label', t("{0}模式", label));
    modes.append(mode);
    modeButtons.set(value, mode);
}
const navigation = el('nav', 'tabs');
navigation.setAttribute('role', 'tablist');
navigation.setAttribute('aria-label', t("功能页面"));
const tabButtons = new Map<Tab, HTMLButtonElement>(), panels = new Map<Tab, HTMLElement>();
for (const [key, title] of tabs) {
    const tab = button(title, () => activateTab(key), 'tab');
    tab.id = `tab-${key}`;
    tab.setAttribute('role', 'tab');
    tab.setAttribute('aria-controls', `panel-${key}`);
    tab.addEventListener('keydown', event => {
        if (event.isComposing || !['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key))
            return;
        event.preventDefault();
        const index = tabs.findIndex(([id]) => id === key);
        const next = event.key === 'Home' ? 0 : event.key === 'End' ? tabs.length - 1 : (index + (event.key === 'ArrowRight' ? 1 : -1) + tabs.length) % tabs.length;
        activateTab(tabs[next][0]);
        tabButtons.get(tabs[next][0])?.focus();
    });
    tabButtons.set(key, tab);
    navigation.append(tab);
    const panel = el('section', 'tab-panel');
    panel.id = `panel-${key}`;
    panel.setAttribute('role', 'tabpanel');
    panel.setAttribute('aria-labelledby', tab.id);
    panels.set(key, panel);
}
const content = el('div', 'content-scroll');
const notice = el('p', 'sr-only');
notice.setAttribute('role', 'status');
notice.setAttribute('aria-live', 'polite');
const banner = el('div', 'error-banner');
banner.setAttribute('role', 'alert');
banner.hidden = true;
const errorCopy = el('span');
const retryButton = iconButton('refresh', t("重新读取"), () => refresh());
banner.append(errorCopy, retryButton);
const browserNotice = el('p', 'browser-notice', t("此页面需要 ClashBar 桌面应用。浏览器无法管理本机内核和系统代理。"));
browserNotice.hidden = desktop;
content.append(browserNotice, banner, ...panels.values());
const footer = el('footer', 'app-footer');
const coreVersion = gate(button('', () => { activateTab('settings'); coreButton.focus(); }, 'footer-core'), () => Boolean(status));
coreVersion.append(icon('chip'), el('span', '', 'MIHOMO —'));
const corePicker: HTMLButtonElement = gate(iconButton('arrowDown', t("更新或选择 mihomo 内核"), () => openCoreMenu(corePicker)), () => isLocal() && Boolean(status));
const appVersion = gate(button('v0.1.0', () => checkAppUpdate(), 'app-version'), () => true);
appVersion.setAttribute('aria-label', t("检查 ClashBar 更新"));
footer.append(coreVersion, corePicker, appVersion);
shell.append(header, modes, navigation, content, footer, notice);
app.append(shell);
const proxyPanel = panels.get('proxies')!;
const traffic = el('section', 'traffic');
traffic.setAttribute('aria-label', t("代理流量"));
const trafficTop = el('div', 'traffic-top'), trafficBottom = el('div', 'traffic-bottom');
const connectionCount = el('span', 'metric connection-count'), upMetric = el('span', 'metric'), memoryMetric = el('span', 'metric memory-count'), downMetric = el('span', 'metric');
trafficTop.append(connectionCount, upMetric);
trafficBottom.append(memoryMetric, downMetric);
const chart = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
chart.setAttribute('viewBox', '0 0 336 60');
chart.setAttribute('preserveAspectRatio', 'none');
chart.setAttribute('class', 'traffic-chart');
chart.setAttribute('aria-hidden', 'true');
traffic.append(chart, trafficTop, trafficBottom);
const configButton = gate(button('', () => openConfigMenu(), 'quick-row'), () => isLocal() && Boolean(status));
configButton.addEventListener('pointerenter', () => { if (!configButton.disabled)
    scheduleMenuOpen(() => openConfigMenu(false)); });
configButton.addEventListener('pointerleave', scheduleMenuClose);
const configName = el('span', 'quick-value', t("未导入"));
configButton.append(icon('document', 'purple'), el('strong', '', t("切换配置")), configName, icon('chevron', 'tertiary'));
const systemRow = el('label', 'quick-row');
systemRow.htmlFor = 'system-proxy';
const systemProxy = gate(el('input', 'switch'), () => Boolean(status?.running || status?.systemProxy));
systemProxy.id = 'system-proxy';
systemProxy.type = 'checkbox';
systemProxy.setAttribute('aria-label', t("系统代理"));
const proxyScope = el('span', 'capsule');
const proxyTargetWarning = el('p', 'settings-hint orange', t("系统代理当前指向远程内核，请确认远程地址可达。"));
proxyTargetWarning.id = 'proxy-target-warning';
proxyTargetWarning.hidden = true;
systemProxy.setAttribute('aria-describedby', proxyTargetWarning.id);
systemRow.append(icon('globe', 'green'), el('strong', '', t("系统代理")), proxyScope, el('span', 'flex-space'), systemProxy);
systemProxy.addEventListener('change', () => { const enabled = systemProxy.checked; systemProxy.checked = status?.systemProxy ?? false; void mutate(enabled ? t("开启系统代理") : t("关闭系统代理"), () => invoke<Status>('set_system_proxy', { enabled })); });
const tunRow = el('div', 'quick-row');
const tunModes = el('span', 'tun-modes');
const tunStacks = new Map<string, HTMLButtonElement>();
for (const stack of ['system', 'gvisor', 'mixed', 'mips']) {
    const control = gate(button(stack, () => mutate(t("更改 TUN 协议栈"), () => invoke<Status>('set_tun', { enabled: tunSwitch.checked, stack })), 'capsule'), () => canRun() && (stack !== 'mips' || supportsMips(status?.version)));
    control.setAttribute('aria-label', `TUN ${stack}`);
    tunModes.append(control);
    tunStacks.set(stack, control);
}
const tunSwitch = gate(el('input', 'switch'), canRun);
tunSwitch.type = 'checkbox';
tunSwitch.setAttribute('aria-label', t("TUN 模式"));
tunSwitch.addEventListener('change', () => { const enabled = tunSwitch.checked; tunSwitch.checked = status?.tunEnabled ?? false; void mutate(enabled ? t("开启 TUN") : t("关闭 TUN"), () => invoke<Status>('set_tun', { enabled, stack: status?.tunStack ?? 'mixed' })); });
tunRow.append(icon('shield', 'green'), el('strong', '', t("TUN 模式")), tunModes, tunSwitch);
const terminalRow = el('div', 'quick-row');
const copyTerminal: HTMLButtonElement = gate(button('', async () => { const ports = status?.localProxyPorts; const mixedPort = ports?.['mixed-port'] ?? status?.localMixedPort ?? status?.mixedPort; if (await copyText(terminalProxyCommand('127.0.0.1', mixedPort || ports?.port, mixedPort || ports?.['socks-port']), t("已复制 PowerShell 代理命令"))) {
    copyTerminal.replaceChildren(el('span', '', t("已复制")), icon('check', 'green'));
    setTimeout(() => copyTerminal.replaceChildren(el('span', '', '127.0.0.1'), icon('copy')), 1600);
} }, 'capsule copy-command'), () => Boolean(status));
copyTerminal.append(el('span', '', '127.0.0.1'), icon('copy'));
copyTerminal.setAttribute('aria-label', t("复制 PowerShell 代理命令"));
const copyRemote = gate(button(t("远程"), () => copyText(isLocal() ? lanTerminalCommand() : remoteTerminalCommand(), t("已复制当前端点命令")), 'capsule copy-command'), () => Boolean(isLocal() ? lanTerminalCommand() : remoteTerminalCommand()));
copyRemote.setAttribute('aria-label', t("复制当前端点命令"));
terminalRow.append(icon('terminal', 'orange'), el('strong', '', t("复制终端命令")), el('span', 'flex-space'), copyTerminal);
terminalRow.append(copyRemote);
const providerHeader = sectionHeading(t("代理提供者"), 'drive');
const providerCounter = el('span', 'count-badge', '0');
providerHeader.insertBefore(providerCounter, providerHeader.lastElementChild);
const providerProgress = el('span', 'provider-age'); providerProgress.setAttribute('role', 'status'); providerProgress.hidden = true; providerHeader.insertBefore(providerProgress, providerHeader.lastElementChild);
const collapseProviders = iconButton('down', t("折叠代理提供者"), () => { providerCollapsed = !providerCollapsed; savePreference('providers-collapsed', providerCollapsed); renderProxies(); });
providerHeader.append(collapseProviders);
const providers = el('div', 'providers');
const groupHeader = sectionHeading(t("代理组"), 'network');
const groupCounter = el('span', 'count-badge', '0');
groupHeader.insertBefore(groupCounter, groupHeader.lastElementChild);
const sortButton = iconButton('list', t("按延迟排序节点"), () => { sortLatency = !sortLatency; savePreference('sort-latency', sortLatency); updateChrome(); });
const historyButton = iconButton('chart', t("显示延迟历史"), () => { showHistory = !showHistory; savePreference('show-history', showHistory); renderProxies(); });
const hiddenButton = iconButton('eye', t("显示隐藏代理组"), () => { hideHidden = !hideHidden; savePreference('hide-hidden', hideHidden); renderProxies(); });
const allDelay = gate(iconButton('gauge', t("测试全部代理组延迟"), async () => {
    await mutate(t("测试全部代理组延迟"), async () => { for (const [name] of currentGroups()) {
        const result = await invoke<Record<string, number>>('test_group_delay', { name });
        for (const [node, delay] of Object.entries(result))
            delays.set(node, delay);
    } });
}), canRun);
groupHeader.append(sortButton, historyButton, hiddenButton, allDelay);
const proxyGroups = el('div', 'proxy-groups');
proxyPanel.append(traffic, configButton, systemRow, proxyTargetWarning, tunRow, terminalRow, providerHeader, providers, groupHeader, proxyGroups);
const rulePanel = panels.get('rules')!;
const ruleToolbar = el('div', 'compact-toolbar');
const ruleStats = el('span', 'muted');
const groupRulesButton = iconButton('list', t("按策略分组"), () => { groupRules = !groupRules; savePreference('group-rules', groupRules); renderRules(); });
const refreshRulesButton = gate(iconButton('refresh', t("刷新规则"), () => mutate(t("刷新规则提供者"), () => invoke('refresh_rule_providers'))), canRun);
ruleToolbar.append(ruleStats, el('span', 'flex-space'), groupRulesButton, refreshRulesButton);
const ruleChips = el('div', 'filter-chips');
const ruleSearch = searchField(t("搜索目标、类型、策略..."), 'rule-search', value => { ruleQuery = value; renderRules(); });
const policySelect = settingSelect(t("筛选策略"), () => [t("全部策略"), ...new Set(snapshot?.rules.rules.map(rule => rule.proxy).sort() ?? [])], () => rulePolicy || t("全部策略"), value => { rulePolicy = value === t("全部策略") ? '' : value; renderRules(); });
const ruleSearchRow = el('div', 'search-row');
ruleSearchRow.append(ruleSearch.wrap, policySelect.control);
const ruleContent = el('div', 'rule-content');
rulePanel.append(ruleToolbar, ruleChips, ruleSearchRow, ruleContent);
const connectionPanel = panels.get('connections')!;
const connectionToolbar = el('div', 'compact-toolbar');
const transportSelect = settingSelect(t("连接协议"), [t("全部协议"), t("仅 TCP"), t("仅 UDP"), t("其他协议")], () => transport || t("全部协议"), value => { transport = value === t("全部协议") ? '' : value; localStorage.setItem('clashbar-transport', transport); transportSelect.update(); renderConnections(); });
const sortSelect = settingSelect(t("连接排序"), [t("默认顺序"), t("最新连接"), t("最早连接"), t("上传流量高到低"), t("下载流量高到低"), t("总流量高到低")], () => connectionSort, value => { connectionSort = value; localStorage.setItem('clashbar-connection-sort', value); sortSelect.update(); renderConnections(); });
const connectionFraction = el('span', 'count-badge');
const closeAll = gate(iconButton('x', t("关闭全部连接"), () => mutate(t("关闭全部连接"), () => invoke('close_all_connections')), 'danger-text'), () => Boolean(status?.running && snapshot?.connections.connections.length));
connectionToolbar.append(transportSelect.control, sortSelect.control, el('span', 'flex-space'), connectionFraction, closeAll);
const connectionSearch = searchField(t("过滤域名、IP..."), 'connection-search', value => { connectionQuery = value; renderConnections(); });
const connectionContent = el('div', 'connections-list');
connectionPanel.append(connectionToolbar, connectionSearch.wrap, connectionContent);
const logsPanel = panels.get('logs')!;
const sourceChips = el('div', 'filter-chips');
const logLevel = settingSelect(t("内核日志级别"), ['silent', 'error', 'warning', 'info', 'debug'], () => snapshot?.configs['log-level'] || 'info', level => mutate(t("更改日志级别"), () => invoke('set_log_level', { level })));
gate(logLevel.control, canRun);
const logFraction = el('span', 'count-badge');
const logTop = el('div', 'compact-toolbar');
logTop.append(sourceChips, el('span', 'flex-space'), logLevel.control, logFraction);
const levelChips = el('div', 'filter-chips');
const copyLogs = iconButton('copy', t("复制全部日志"), () => copyText(combinedLogs.map(fullLogRecord).join('\n'), t("已复制全部日志")));
const clearLogs = gate(iconButton('trash', t("清理全部日志"), () => mutate(t("清理全部日志"), async () => { await invoke('clear_logs'); logs = []; appLogs = []; combinedLogs = []; })), () => true);
const logActions = el('div', 'compact-toolbar');
logActions.append(levelChips, el('span', 'flex-space'), copyLogs, clearLogs);
const logSearch = searchField(t("搜索日志..."), 'log-search', value => { logQuery = value; renderLogs(); });
const logContent = el('div', 'logs-list');
logContent.setAttribute('aria-label', t("内核日志"));
logsPanel.append(logTop, logActions, logSearch.wrap, logContent);
const settings = panels.get('settings')!;
const basicSettingsHeading = sectionHeading(t("基础设置"), 'settings');
settings.append(basicSettingsHeading);
const startupSwitches = new Map<'launchAtLogin' | 'autoStartCore', HTMLInputElement>();
for (const [key, label, symbol, command] of [['launchAtLogin', t("开机自启"), 'power', 'set_launch_at_login'], ['autoStartCore', t("内核自启"), 'play', 'set_core_autostart']] as const) {
    const control = gate(el('input', 'switch'), () => typeof status?.[key] === 'boolean');
    control.type = 'checkbox';
    control.id = `startup-${key}`;
    control.setAttribute('aria-label', label);
    control.addEventListener('change', () => { const enabled = control.checked; control.checked = status?.[key] ?? false; void mutate(t("更改{0}", label), () => invoke<Status>(command, { enabled })); });
    const row = settingRow(label, symbol, control);
    const caption = el('label', 'setting-label', label);
    caption.htmlFor = control.id;
    row.querySelector('.setting-label')!.replaceWith(caption);
    settings.append(row);
    startupSwitches.set(key, control);
}
const startupError = el('p', 'field-message error-text');
startupError.id = 'startup-error';
startupError.hidden = true;
startupError.setAttribute('aria-live', 'polite');
settings.append(startupError);
startupSwitches.get('launchAtLogin')!.setAttribute('aria-describedby', startupError.id);
let trayStyle = localizedPreference(localStorage.getItem('clashbar-tray-style') || '图标和网速');
const trayStyleSelect = settingSelect(t("状态栏样式"), [t("图标和网速"), t("仅图标"), t("仅网速")], () => trayStyle, async (value) => { if (await mutate(t("更改状态栏样式"), () => invoke('set_status_bar_style', { style: value === t("仅图标") ? 'iconOnly' : value === t("仅网速") ? 'speedOnly' : 'iconAndSpeed' }), false)) {
    trayStyle = value;
    localStorage.setItem('clashbar-tray-style', value);
    trayStyleSelect.update();
} });
gate(trayStyleSelect.control, () => true);
settings.append(settingRow(t("状态栏样式"), 'chart', trayStyleSelect.control));
const languageSelect = settingSelect(t('界面语言'), ['简体中文', 'English'], () => locale === 'en' ? 'English' : '简体中文', async value => {
    if (busy) return;
    if (portsDirty) { clearTimeout(portSaveTimer); await savePortSettings(true); if (portsDirty) return; }
    const language = value === 'English' ? 'en' : 'zh-CN';
    if (desktop && !await mutate(t('界面语言'), () => invoke('set_ui_language', { language }), false)) return;
    localStorage.setItem('clashbar-language', language);
    sessionStorage.setItem('clashbar-resume-tab', activeTab);
    location.reload();
});
settings.append(settingRow(t('界面语言'), 'globe', languageSelect.control));
let appearance = localizedPreference(localStorage.getItem('clashbar-appearance') || '跟随系统');
const appearanceSelect = settingSelect(t("外观模式"), [t("跟随系统"), t("浅色"), t("深色")], () => appearance, value => { appearance = value; localStorage.setItem('clashbar-appearance', value); applyAppearance(); appearanceSelect.update(); });
settings.append(settingRow(t("外观模式"), 'sun', appearanceSelect.control));
const settingsLogLevel = settingSelect(t("设置内核日志级别"), ['silent', 'error', 'warning', 'info', 'debug'], () => snapshot?.configs['log-level'] || 'info', level => mutate(t("更改日志级别"), () => invoke('set_log_level', { level })));
gate(settingsLogLevel.control, canRun);
settings.append(settingRow(t("日志级别"), 'document', settingsLogLevel.control));
const exceptionsButton = gate(button(t("编辑"), () => openProxyExceptions(), 'value-select'), () => Boolean(status));
settings.append(settingRow(t("代理绕过"), 'network', exceptionsButton));
const coreSettingsHeading = sectionHeading(t("内核设置"), 'chip');
settings.append(coreSettingsHeading);
const coreSwitches = new Map<'allow-lan' | 'ipv6' | 'tcp-concurrent', HTMLInputElement>();
for (const [label, symbol, key] of [[t("允许局域网"), 'globe', 'allow-lan'], ['IPv6', 'network', 'ipv6'], [t("TCP 并发"), 'link', 'tcp-concurrent']] as const) {
    const control = gate(el('input', 'switch'), () => canRun() && typeof snapshot?.configs[key] === 'boolean');
    control.type = 'checkbox';
    control.setAttribute('aria-label', label);
    control.id = `core-${key}`;
    control.addEventListener('change', () => {
        const value = control.checked;
        control.checked = snapshot?.configs[key] ?? false;
        void mutate(t("更改{0}", label), () => invoke('set_core_boolean', { setting: key, value }));
    });
    const row = settingRow(label, symbol, control);
    const caption = el('label', 'setting-label', label);
    caption.htmlFor = control.id;
    row.querySelector('.setting-label')!.replaceWith(caption);
    if (key === 'allow-lan')
        row.title = t("允许同一局域网内的设备连接代理端口；控制器仍只接受本机连接。");
    settings.append(row);
    coreSwitches.set(key, control);
}
const coreButton: HTMLButtonElement = gate(button(t("管理内核"), () => openCoreMenu(coreButton), 'value-select'), () => isLocal() && Boolean(status));
settings.append(settingRow(t("mihomo 内核"), 'chip', coreButton));
const corePath = el('p', 'settings-path');
settings.append(corePath);
const portForm = el('form', 'port-form');
portForm.noValidate = true;
portForm.append(sectionHeading(t("代理端口"), 'network'));
const httpPort = portField(t("HTTP 端口"), 'http-port', 'port'), socksPort = portField(t("SOCKS 端口"), 'socks-port', 'socks-port');
portForm.append(httpPort.row, socksPort.row);
const mixed = portField(t("混合端口"), 'mixed-port'), controller = portField(t("控制端口"), 'controller-port');
portForm.append(mixed.row, controller.row);
const redirPort = portField(t("重定向端口"), 'redir-port', 'redir-port'), tproxyPort = portField(t("TProxy 端口"), 'tproxy-port', 'tproxy-port');
portForm.append(redirPort.row, tproxyPort.row);
const remotePortFields = { port: httpPort, 'socks-port': socksPort, 'mixed-port': mixed, 'redir-port': redirPort, 'tproxy-port': tproxyPort };
const portInputs = [httpPort.input, socksPort.input, mixed.input, redirPort.input, tproxyPort.input, controller.input];
const portError = el('p', 'field-message error-text');
portError.id = 'port-error';
for (const input of portInputs) {
    input.setAttribute('aria-describedby', portError.id);
    input.addEventListener('input', () => { portsDirty = true; portError.textContent = ''; input.setAttribute('aria-invalid', 'false'); clearTimeout(portSaveTimer); portSaveTimer = setTimeout(() => { void savePortSettings(false); }, 750); });
}
for (const input of portInputs)
    input.addEventListener('keydown', event => { if (event.key === 'Enter' && !event.isComposing) {
        event.preventDefault();
        clearTimeout(portSaveTimer);
        void savePortSettings(true);
    } });
portForm.append(portError);
portForm.addEventListener('submit', event => { event.preventDefault(); clearTimeout(portSaveTimer); void savePortSettings(true); });
async function savePortSettings(focusInvalid: boolean) {
    if (busy || !desktop || !status || (isLocal() ? status.running : !status.running) || !portsDirty)
        return;
    const proxyValues = Object.values(remotePortFields).map(field => field.input.value);
    const message = isLocal() ? validateLocalPorts(proxyValues, controller.input.value) : validateRemotePorts(proxyValues);
    portError.textContent = message || '';
    portError.classList.toggle('error-text', Boolean(message));
    for (const input of portInputs.filter(input => !input.disabled))
        input.setAttribute('aria-invalid', String(Boolean(message)));
    if (message) {
        if (focusInvalid)
            mixed.input.focus();
        return;
    }
    const ports = Object.fromEntries(Object.entries(remotePortFields).map(([key, field]) => [key, Number(field.input.value)]));
    const success = await mutate(t("保存端口"), () => isLocal() ? invoke<Status>('save_local_ports', { ports, controllerPort: Number(controller.input.value) }) : invoke('save_remote_ports', { ports }));
    if (success) {
        portsDirty = false;
        portError.textContent = t("已保存");
        portError.classList.remove('error-text');
    }
}
settings.append(portForm, sectionHeading(t("系统维护"), 'settings'));
const maintenance = el('div', 'maintenance-grid');
for (const [label, command] of [[t("清理 FakeIP 缓存"), 'flush_fakeip_cache'], [t("清理 DNS 缓存"), 'flush_dns_cache'], [t("更新 Geo 数据库"), 'upgrade_geo']]) {
    maintenance.append(gate(button(label, () => mutate(label, () => invoke(command)), 'maintenance-action'), canRun));
}
maintenance.append(gate(button(t("打开内核目录"), () => mutate(t("打开内核目录"), () => invoke('open_core_directory'), false), 'maintenance-action'), () => isLocal() && Boolean(status?.corePath)));
settings.append(maintenance);
const settingsHint = el('p', 'settings-hint', t("更换内核或本机端口前请先停止内核。"));
settings.append(settingsHint);
function sectionHeading(label: string, symbol: string) { const row = el('div', 'section-heading'); row.append(icon(symbol), el('h2', '', label), el('span', 'flex-space')); return row; }
function settingRow(label: string, symbol: string, control: HTMLElement) { const row = el('div', 'setting-row'); row.append(icon(symbol), el('span', 'setting-label', label), control); return row; }
function portField(label: string, id: string, remoteOnly?: string) {
    const input = gate(el('input', 'port-input'), () => isLocal() ? Boolean(status && !status.running) : id !== 'controller-port' && canRun() && ['port', 'socks-port', 'mixed-port', 'redir-port', 'tproxy-port'].every(key => typeof snapshot?.configs[key as keyof Snapshot['configs']] === 'number'));
    input.type = 'number';
    input.id = id;
    input.min = remoteOnly || id !== 'controller-port' ? '0' : '1024';
    input.max = '65535';
    input.step = '1';
    const row = el('div', 'setting-row');
    const caption = el('label', 'setting-label', label);
    caption.htmlFor = id;
    row.append(icon('network'), caption, input);
    return { row, input };
}
function applyAppearance() { document.documentElement.dataset.appearance = appearance === t("深色") ? 'dark' : appearance === t("浅色") ? 'light' : 'system'; }
function openCoreMenu(anchor: HTMLElement) {
    choose(anchor, t("mihomo 内核"), [
        { label: t("选择本机内核..."), disabled: Boolean(status?.running), action: () => mutate(t("选择内核"), () => invoke<Status>('choose_core')) },
        { label: t("更新 mihomo 内核"), disabled: !status?.running, action: () => mutate(t("更新 mihomo 内核"), () => invoke('upgrade_core')) },
        { label: t("打开内核目录"), disabled: !status?.corePath, action: () => mutate(t("打开内核目录"), () => invoke('open_core_directory'), false) },
    ]);
}
async function checkAppUpdate() {
    let release: AppReleaseInfo | null = null;
    if (!await mutate(t("检查 ClashBar 更新"), async () => { release = await invoke<AppReleaseInfo>('check_app_update'); }, false) || !release)
        return;
    const result = release as AppReleaseInfo;
    const dialog = el('dialog', 'dialog');
    dialog.setAttribute('aria-labelledby', 'update-title');
    const title = el('h2', '', result.updateAvailable ? t("发现新版本 {0}", result.displayVersion) : t("已是最新版本"));
    title.id = 'update-title';
    const description = el('p', 'dialog-copy', t("当前版本 {0} \u00B7 最新版本 {1}", result.currentVersion, result.displayVersion));
    const error = el('p', 'field-message error-text');
    const close = button(t("关闭"), () => dialog.close(), 'small-action');
    const open = button(t("打开发布页"), async () => { open.disabled = close.disabled = true; const success = await mutate(t("打开发布页"), () => invoke('open_app_release'), false); open.disabled = close.disabled = false; if (success)
        dialog.close();
    else
        error.textContent = actionError; }, 'small-action primary');
    const actions = el('div', 'dialog-actions');
    actions.append(el('span', 'flex-space'), close, open);
    dialog.append(title, description, error, actions);
    presentDialog(dialog, close, appVersion, () => open.disabled);
}
function openWebUI() { return mutate(t('打开 WebUI'), () => invoke('open_web_ui'), false); }

function openProxyExceptions() {
    const dialog = el('dialog', 'dialog');
    dialog.setAttribute('aria-labelledby', 'exceptions-title');
    const title = el('h2', '', t("代理绕过"));
    title.id = 'exceptions-title';
    const form = el('form');
    form.noValidate = true;
    const caption = el('label', '', t("每行一个域名、IP 或通配符"));
    caption.htmlFor = 'exceptions-input';
    const input = el('textarea', 'exceptions-input');
    input.id = caption.htmlFor;
    input.rows = 8;
    input.spellcheck = false;
    input.value = (status?.systemProxyExceptions ?? []).join('\n');
    const error = el('p', 'field-message error-text');
    error.id = 'exceptions-error';
    input.setAttribute('aria-describedby', error.id);
    const cancel = button(t("取消"), () => dialog.close(), 'small-action');
    const save = button(t("保存绕过"), () => { }, 'small-action primary');
    save.type = 'submit';
    const defaults = button(t("恢复默认"), () => { input.value = ['::1', '*.local', '<local>', 'localhost', '127.0.0.1', '192.168.0.0/16', '10.0.0.0/8', '172.16.0.0/12'].join('\n'); }, 'small-action');
    const actions = el('div', 'dialog-actions');
    actions.append(defaults, el('span', 'flex-space'), cancel, save);
    form.append(caption, input, error, actions);
    dialog.append(title, form);
    form.addEventListener('submit', async (event) => { event.preventDefault(); if (save.disabled)
        return; const exceptions = [...new Set(input.value.split(/\r?\n/).map(item => item.trim()).filter(Boolean))]; save.disabled = cancel.disabled = defaults.disabled = true; input.disabled = true; save.setAttribute('aria-busy', 'true'); const success = await mutate(t("保存代理绕过"), () => invoke<Status>('save_proxy_exceptions', { exceptions })); save.disabled = cancel.disabled = defaults.disabled = false; input.disabled = false; save.setAttribute('aria-busy', 'false'); if (success)
        dialog.close();
    else {
        error.textContent = actionError;
        input.setAttribute('aria-invalid', 'true');
        input.focus();
    } });
    presentDialog(dialog, input, exceptionsButton, () => save.disabled);
}
function openSSIDSettings() {
    const dialog = el('dialog', 'dialog');
    dialog.setAttribute('aria-labelledby', 'ssid-title');
    const title = el('h2', '', t("Wi-Fi 自动切换"));
    title.id = 'ssid-title';
    const body = el('div', 'ssid-bindings');
    const feedback = el('p', 'field-message error-text');
    feedback.setAttribute('aria-live', 'polite');
    let pending = false;
    const close = button(t("关闭"), () => dialog.close(), 'small-action');
    const run = async (label: string, operation: () => Promise<unknown>) => { if (pending)
        return; pending = true; close.disabled = true; render(); const success = await mutate(label, operation); pending = false; close.disabled = false; feedback.textContent = success ? '' : actionError; render(); };
    const render = () => {
        const network = status?.ssidSnapshot;
        const current = el('p', 'dialog-copy', network?.currentSsid ? t("当前 Wi-Fi：{0}", network.currentSsid) : network?.error ? safeError(network.error) : t("未读取到 Wi-Fi 名称。启用或刷新后读取当前网络。"));
        const enabled = button(status?.ssidEnabled ? t("停用自动切换") : t("启用自动切换"), () => run(t("更改 SSID 自动切换"), () => invoke<Status>('set_ssid_enabled', { enabled: !status?.ssidEnabled })), 'small-action');
        const refresh = button(t("刷新 Wi-Fi"), () => run(t("刷新 Wi-Fi"), () => invoke<Status>('refresh_ssid')), 'small-action');
        enabled.disabled = refresh.disabled = pending;
        const controls = el('div', 'dialog-actions');
        controls.append(enabled, refresh);
        body.replaceChildren(current, controls);
        if (network?.status === 'permissionDenied') {
            const permission = button(t('打开定位设置'), () => run(t('打开定位设置'), () => invoke('open_location_settings')), 'small-action');
            permission.disabled = pending; body.append(permission);
        }
        if (status?.ssidError)
            body.append(el('p', 'error-text', safeError(status.ssidError)));
        if (!status?.ssidRules?.length)
            body.append(el('p', 'muted', t("在配置菜单中选择“绑定当前 Wi-Fi”，连接该网络时将自动切换配置。")));
        for (const binding of status?.ssidRules ?? []) {
            const row = el('div', 'setting-row');
            const label = el('span', 'truncate', `${binding.ssid} → ${binding.configFileName}`);
            label.title = `${binding.ssid} → ${binding.configFileName}`;
            const remove = iconButton('x', t("解绑 Wi-Fi {0}", binding.ssid), () => run(t("解绑 Wi-Fi"), () => invoke<Status>('remove_ssid_binding', { ssid: binding.ssid })));
            remove.disabled = pending;
            row.append(label, remove);
            body.append(row);
        }
    };
    const actions = el('div', 'dialog-actions');
    actions.append(el('span', 'flex-space'), close);
    dialog.append(title, body, feedback, actions);
    render();
    presentDialog(dialog, close, configButton, () => pending);
}
function remoteTerminalCommand() {
    const machine = status?.remoteMachines?.find(machine => machine.id === status?.activeRemoteId);
    if (!machine || !snapshot || !status?.running)
        return '';
    const config = snapshot.configs;
    const mixed = config['mixed-port'];
    return terminalProxyCommand(machine.host, mixed || config.port, mixed || config['socks-port']);
}
function lanTerminalCommand() {
    if (!status?.localLanAddress) return '';
    const ports = status.localProxyPorts;
    const mixedPort = ports?.['mixed-port'] ?? status.localMixedPort ?? status.mixedPort;
    return terminalProxyCommand(status.localLanAddress, mixedPort || ports?.port, mixedPort || ports?.['socks-port']);
}
function supportsMips(version?: string | null) {
    const match = version?.match(/(\d+)\.(\d+)\.(\d+)/);
    if (!match) return false;
    const parts = match.slice(1).map(Number);
    return parts[0] > 1 || (parts[0] === 1 && (parts[1] > 19 || (parts[1] === 19 && parts[2] >= 31)));
}
function openEndpointMenu(focus = true) {
    const choices: MenuChoice[] = [{ label: t("本机"), detail: `127.0.0.1:${status?.controllerPort ?? 19090}`, checked: isLocal(), action: () => switchMachineTarget(null) }];
    for (const machine of status?.remoteMachines ?? [])
        choices.push({ label: machine.name, detail: machine.address, checked: status?.activeRemoteId === machine.id, action: () => switchMachineTarget(machine.id) });
    choices.push({ label: '', kind: 'separator', action: () => { } }, { label: t("管理远程机器"), action: async () => {
            if (portsDirty) {
                clearTimeout(portSaveTimer);
                await savePortSettings(true);
                if (portsDirty) {
                    actionError = t("请先修正并保存端口设置，再管理远程机器。");
                    updateChrome();
                    return;
                }
            }
            openMachineManager({ status: () => status, busy: () => busy, error: () => actionError, mutate, select: switchMachineTarget, resize: () => { lastRequestedHeight = 0; requestResize(); } }, endpoint);
        } });
    choose(endpoint, t("控制器"), choices, focus);
}
async function switchMachineTarget(id: string | null): Promise<boolean> {
    if (portsDirty) {
        clearTimeout(portSaveTimer);
        await savePortSettings(true);
        if (portsDirty) {
            actionError = t("请先修正并保存端口设置，再切换机器。");
            updateChrome();
            return false;
        }
    }
    return mutate(t("切换机器"), () => invoke<Status>('select_machine', { id }));
}
function acceptStatus(next: Status) {
    if (next.launchAtLogin == null && !next.launchAtLoginError && status) {
        next.launchAtLogin = status.launchAtLogin;
        next.launchAtLoginError = status.launchAtLoginError;
    }
    if (status && ((status.activeRemoteId ?? null) !== (next.activeRemoteId ?? null) || status.targetRevision !== next.targetRevision)) {
        clearTimeout(portSaveTimer);
        portsDirty = false;
        portError.textContent = '';
        snapshot = null;
        logs = [];
        appLogs = [];
        combinedLogs = [];
        delays.clear();
        lastSample = null;
        rateUp = rateDown = null;
        trafficSamples.length = 0;
        refreshError = '';
        resetEphemeralState();
    }
    status = next;
    if (!localStorage.getItem('clashbar-language') && next.uiLanguage && next.uiLanguage !== locale) {
        localStorage.setItem('clashbar-language', next.uiLanguage);
        sessionStorage.setItem('clashbar-resume-tab', activeTab);
        location.reload();
    }
    if (next.statusBarStyle) { trayStyle = next.statusBarStyle === 'iconOnly' ? t('仅图标') : next.statusBarStyle === 'speedOnly' ? t('仅网速') : t('图标和网速'); trayStyleSelect.update(); }
}
function openConfigMenu(focus = true) {
    const choices: MenuChoice[] = (status?.profiles ?? []).map(profile => {
        const subscription = status?.subscriptions?.find(item => item.profileId === profile.id);
        const networks = status?.ssidRules?.filter(item => item.configFileName === profile.name).map(item => item.ssid) ?? [];
        return { label: profile.name, detail: subscription ? t("订阅") : t("本地"), value: networks.join(', ') || (subscription?.lastError ? t("更新失败") : undefined), checked: profile.id === status?.activeProfileId, action: () => mutate(t("切换配置"), () => invoke<Status>('select_profile', { id: profile.id })), secondaryKind: 'context' as const, secondaryLabel: t("管理配置 {0}", profile.name), secondaryAction: () => openProfileActions(profile) };
    });
    if (!choices.length && status?.configName)
        choices.push({ label: status.configName, checked: true, action: () => { } });
    choices.push({ label: '', kind: 'separator', action: () => { } }, { label: t("导入配置文件..."), action: () => importLocalConfig() }, { label: t("导入订阅链接..."), action: () => openSubscription() });
    choices.push({ label: t("更新全部订阅"), disabled: !status?.subscriptions?.length, action: () => mutate(t("更新全部订阅"), () => invoke<Status>('refresh_all_subscriptions')) }, { label: t("重新加载配置列表"), action: () => refresh() }, { label: t('在文件夹中显示当前配置'), disabled: !status?.activeProfileId, action: () => mutate(t('在文件夹中显示当前配置'), () => invoke('open_selected_profile'), false) }, { label: t("打开配置目录"), action: () => mutate(t("打开配置目录"), () => invoke('open_profiles_directory'), false) });
    choices.push({ label: '', kind: 'separator', action: () => { } }, { label: t("SSID 自动切换"), checked: status?.ssidEnabled ?? false, action: () => mutate(t("更改 SSID 自动切换"), () => invoke<Status>('set_ssid_enabled', { enabled: !status?.ssidEnabled })) }, { label: t("管理 Wi-Fi 绑定..."), detail: status?.ssidSnapshot?.currentSsid ?? undefined, action: () => openSSIDSettings() });
    choose(configButton, t("配置文件"), choices, focus);
}
function openProfileActions(profile: {
    id: string;
    name: string;
}) {
    const subscription = status?.subscriptions?.find(item => item.profileId === profile.id);
    const currentSSID = status?.ssidSnapshot?.currentSsid;
    const networks = status?.ssidRules?.filter(item => item.configFileName === profile.name) ?? [];
    const choices: MenuChoice[] = [];
    if (subscription)
        choices.push({ label: t("更新订阅"), detail: subscription.lastUpdatedAt ? relativeTime(new Date(subscription.lastUpdatedAt).toISOString()) : undefined, action: () => mutate(t("更新订阅"), () => invoke<Status>('refresh_subscription', { id: profile.id })) }, { label: t("编辑订阅与自动更新..."), action: () => openSubscription(subscription) }, { label: t("复制订阅链接"), action: () => mutate(t("复制订阅链接"), () => invoke('copy_subscription_url', { id: profile.id }), false) });
    if (currentSSID)
        choices.push({ label: networks.some(item => item.ssid === currentSSID) ? t("解绑当前 Wi-Fi {0}", currentSSID) : t("绑定当前 Wi-Fi {0}", currentSSID), disabled: !status?.ssidEnabled, detail: status?.ssidEnabled ? undefined : t('先启用 SSID 自动切换'), action: () => mutate(t("更改 Wi-Fi 绑定"), () => invoke<Status>('bind_ssid', { profileId: profile.id })) });
    for (const binding of networks.filter(item => item.ssid !== currentSSID))
        choices.push({ label: t("解绑 Wi-Fi {0}", binding.ssid), action: () => mutate(t("解绑 Wi-Fi"), () => invoke<Status>('remove_ssid_binding', { ssid: binding.ssid })) });
    choices.push({ label: t("删除配置"), action: () => confirmDeleteProfile(profile) }, { label: t("返回配置列表"), action: () => openConfigMenu() });
    choose(configButton, profile.name, choices);
}
function confirmDeleteProfile(profile: {
    id: string;
    name: string;
}) {
    const dialog = el('dialog', 'dialog');
    dialog.setAttribute('aria-labelledby', 'delete-profile-title');
    dialog.setAttribute('aria-describedby', 'delete-profile-description');
    const title = el('h2', '', t("删除配置「{0}」？", profile.name));
    title.id = 'delete-profile-title';
    const active = profile.id === status?.activeProfileId;
    const consequence = active ? (status?.profiles?.length === 1 ? t("这是最后一个配置，删除后将停止内核。") : t("删除后将切换到列表中的首个可用配置。")) : t("当前使用的配置不受影响。");
    const description = el('p', 'muted', t("{0}文件移至应用的配置回收目录，原始导入文件不受影响。", consequence));
    description.id = 'delete-profile-description';
    const error = el('p', 'field-message error-text');
    const cancel = button(t("取消"), () => dialog.close(), 'small-action');
    const submit = button(t("删除配置"), async () => {
        if (submit.disabled)
            return;
        cancel.disabled = submit.disabled = true;
        submit.setAttribute('aria-busy', 'true');
        error.textContent = '';
        const success = await mutate(t("删除配置"), () => invoke<Status>('delete_profile', { id: profile.id }));
        cancel.disabled = submit.disabled = false;
        submit.setAttribute('aria-busy', 'false');
        if (success)
            dialog.close();
        else {
            error.textContent = actionError;
            cancel.focus();
        }
    }, 'small-action danger-text');
    const actions = el('div', 'dialog-actions');
    actions.append(el('span', 'flex-space'), cancel, submit);
    dialog.append(title, description, error, actions);
    presentDialog(dialog, cancel, configButton, () => submit.disabled);
}
async function importLocalConfig() {
    let prepared: { name: string; overwriteRequired: boolean } | null = null;
    if (!await mutate(t('选择配置文件'), async () => { prepared = await invoke('prepare_config_import'); }, false) || !prepared) return;
    const candidate = prepared as { name: string; overwriteRequired: boolean };
    if (!candidate.overwriteRequired) { await mutate(t('导入配置'), () => invoke<Status>('finish_config_import', { overwrite: false })); return; }
    const dialog = el('dialog', 'dialog'); dialog.setAttribute('aria-labelledby', 'overwrite-title'); dialog.setAttribute('aria-describedby', 'overwrite-description');
    const title = el('h2', '', t('覆盖配置「{0}」？', candidate.name)); title.id = 'overwrite-title';
    const description = el('p', 'dialog-copy', t('同名配置将被替换。若该配置正在使用，内核将应用新内容；其他配置保留。')); description.id = 'overwrite-description';
    const error = el('p', 'field-message error-text'); let completed = false;
    const cancel = button(t('取消'), () => dialog.close(), 'small-action');
    const overwrite = button(t('覆盖配置'), async () => {
        if (overwrite.disabled) return;
        cancel.disabled = overwrite.disabled = true; overwrite.setAttribute('aria-busy', 'true');
        completed = await mutate(t('导入配置'), () => invoke<Status>('finish_config_import', { overwrite: true }));
        cancel.disabled = overwrite.disabled = false; overwrite.setAttribute('aria-busy', 'false');
        if (completed) dialog.close(); else error.textContent = actionError;
    }, 'small-action danger-text');
    dialog.addEventListener('close', () => { if (!completed) void invoke('cancel_config_import').catch(() => {}); }, { once: true });
    const actions = el('div', 'dialog-actions'); actions.append(el('span', 'flex-space'), cancel, overwrite); dialog.append(title, description, error, actions);
    presentDialog(dialog, cancel, configButton, () => overwrite.disabled);
}
function openSubscription(existing?: SubscriptionSummary) {
    const trigger = document.activeElement as HTMLElement | null;
    const dialog = el('dialog', 'dialog subscription-dialog');
    dialog.setAttribute('aria-labelledby', 'subscription-title');
    const title = el('h2', '', existing ? t("编辑订阅") : t("导入订阅链接"));
    title.id = 'subscription-title';
    const form = el('form');
    form.noValidate = true;
    const url = field(t("订阅链接"), 'subscription-url', 'password');
    url.input.autocomplete = 'off';
    url.input.spellcheck = false;
    url.input.placeholder = 'https://…';
    if (existing)
        url.input.placeholder = t("留空保留已有链接");
    const name = field(t("配置名称（可选）"), 'subscription-name');
    const interval = field(t("更新间隔（小时）"), 'subscription-interval', 'number');
    interval.input.min = '1';
    interval.input.max = '8760';
    interval.input.step = '1';
    interval.input.value = String(existing?.autoUpdateIntervalHours ?? 6);
    const auto = el('input', 'switch');
    auto.type = 'checkbox';
    auto.checked = existing?.autoUpdateEnabled ?? true;
    auto.id = 'subscription-auto';
    const autoCaption = el('label', 'setting-label', t("自动更新"));
    autoCaption.htmlFor = auto.id;
    const autoRow = el('div', 'setting-row');
    autoRow.append(autoCaption, auto);
    const reveal = button(t("显示链接"), () => { const visible = url.input.type === 'password'; url.input.type = visible ? 'text' : 'password'; reveal.textContent = visible ? t("隐藏链接") : t("显示链接"); reveal.setAttribute('aria-pressed', String(visible)); }, 'small-action');
    reveal.setAttribute('aria-pressed', 'false');
    const cancel = button(t("取消"), () => dialog.close(), 'small-action');
    const submit = button(existing ? t("保存订阅") : t("导入订阅"), () => { }, 'small-action primary');
    submit.type = 'submit';
    let overwriteName: string | null = null;
    const overwriteWarning = el('p', 'field-message orange'); overwriteWarning.hidden = true; overwriteWarning.setAttribute('role', 'status');
    for (const input of [name.input, url.input]) input.addEventListener('input', () => { overwriteName = null; overwriteWarning.hidden = true; submit.textContent = existing ? t('保存订阅') : t('导入订阅'); submit.classList.remove('danger-text'); });
    const actions = el('div', 'dialog-actions');
    actions.append(reveal, el('span', 'flex-space'), cancel, submit);
    if (!existing)
        form.append(name.wrapper);
    form.append(url.wrapper, autoRow, interval.wrapper, el('p', 'muted', existing ? t("订阅来源：{0}。留空保留已有链接。", existing.sourceHost) : t("使用 HTTPS 链接导入；已有配置会保留，可随时切换。")), overwriteWarning, actions);
    dialog.append(title, form);
    if (existing?.lastError)
        form.insertBefore(el('p', 'error-text', safeError(existing.lastError)), actions);
    form.addEventListener('submit', async event => {
        event.preventDefault();
        if (submit.disabled || event.isTrusted && url.input.matches(':disabled'))
            return;
        const validation = existing && !url.input.value.trim() ? null : validateSubscription(url.input.value);
        setFieldError(url.input, url.error, validation);
        if (validation) {
            url.input.focus();
            return;
        }
        const hours = Number(interval.input.value);
        const intervalError = !Number.isInteger(hours) || hours < 1 || hours > 8760 ? t("更新间隔需为 1–8760 小时的整数。") : null;
        setFieldError(interval.input, interval.error, intervalError);
        if (intervalError) {
            interval.input.focus();
            return;
        }
        const setPending = (pending: boolean) => { for (const control of [submit, cancel, reveal, name.input, url.input, interval.input, auto]) control.disabled = pending; submit.setAttribute('aria-busy', String(pending)); };
        setPending(true);
        const input = { url: url.input.value.trim() || null, autoUpdateEnabled: auto.checked, autoUpdateIntervalHours: hours };
        let candidate: { name: string; overwriteRequired: boolean } | null = null;
        if (!existing) {
            const checked = await mutate(t('检查订阅'), async () => { candidate = await invoke('prepare_subscription', { input: { ...input, name: name.input.value.trim() || null } }); }, false);
            if (!checked || !candidate) { setPending(false); setFieldError(url.input, url.error, actionError); url.input.focus(); return; }
            const prepared = candidate as { name: string; overwriteRequired: boolean };
            if (prepared.overwriteRequired && overwriteName !== prepared.name) {
                overwriteName = prepared.name; overwriteWarning.hidden = false;
                overwriteWarning.textContent = t('配置「{0}」已存在。确认后替换同名配置；正在使用时内核将应用新内容。', prepared.name);
                submit.textContent = t('覆盖配置'); submit.classList.add('danger-text'); setPending(false); cancel.focus(); return;
            }
        }
        const success = await mutate(existing ? t("保存订阅") : t("导入订阅"), () => existing ? invoke<Status>('save_subscription', { id: existing.profileId, ...input }) : invoke<Status>('add_subscription', { input: { ...input, name: ((candidate as { name: string } | null)?.name ?? name.input.value.trim()) || null, overwrite: overwriteName !== null } }));
        setPending(false);
        if (success) dialog.close(); else { setFieldError(url.input, url.error, actionError); url.input.focus(); }
    });
    presentDialog(dialog, url.input, trigger?.isConnected ? trigger : configButton, () => submit.disabled);
}
function updateChrome() {
    for (const [control, enabled] of gated) {
        if (!control.isConnected) {
            gated.delete(control);
            continue;
        }
        control.disabled = !desktop || busy || !enabled();
    }
    retryButton.disabled = !desktop || busy || refreshing;
    const refreshProgress = status?.providerRefresh;
    providerProgress.hidden = !refreshProgress?.running;
    providerProgress.textContent = refreshProgress?.running ? `${refreshProgress.done}/${refreshProgress.total}` : '';
    providerProgress.title = refreshProgress?.error ?? '';
    setIcon(runButton, status?.running ? 'stop' : 'play', status?.running ? t("停止内核") : t("启动内核"));
    runButton.setAttribute('aria-busy', String(busy));
    runButton.classList.toggle('orange', Boolean(status?.running));
    runButton.classList.toggle('green', !status?.running);
    restartButton.title = status?.running ? t("重启内核") : t("启动内核");
    pinButton.classList.toggle('selected', pinned);
    pinButton.setAttribute('aria-pressed', String(pinned));
    pinButton.setAttribute('aria-label', pinned ? t("取消固定") : t("固定面板"));
    endpointText.textContent = status?.controllerAddress ?? `127.0.0.1:${status?.controllerPort ?? 9090}`;
    endpoint.setAttribute('aria-label', t("{0}控制器，{1}", isLocal() ? t("本机") : status?.targetName ?? '', status?.running ? t("运行中") : t("已停止")));
    endpoint.title = `${status?.targetName ?? t("本机")} · ${endpointText.textContent}`;
    statusDot.dataset.state = status?.running ? 'running' : 'stopped';
    configName.textContent = isLocal() ? status?.configName || t("未导入") : t("(远程连接不可变更)");
    configName.title = configName.textContent;
    systemProxy.checked = status?.systemProxy ?? false;
    tunSwitch.checked = status?.tunEnabled ?? snapshot?.configs.tun?.enable ?? false;
    for (const [stack, control] of tunStacks)
        control.setAttribute('aria-pressed', String(stack === (status?.tunStack ?? snapshot?.configs.tun?.stack)));
    systemRow.title = status?.systemProxyTarget ? t("本机系统代理当前指向 {0}", status.systemProxyTarget) : t("控制本机系统代理，启用后使用当前选中内核的代理端口。");
    proxyScope.hidden = !status?.systemProxy;
    proxyScope.textContent = status?.systemProxyRemote ? t("远程") : t("本地");
    proxyTargetWarning.hidden = !status?.systemProxyRemote;
    coreVersion.querySelector('span')!.textContent = `MIHOMO ${status?.version ? `v${status.version.replace(/^v/, '')}` : '—'}`;
    corePath.textContent = isLocal() ? status?.corePath || t("尚未选择 mihomo.exe") : t("远程机器的内核由该机器管理。");
    copyRemote.hidden = false;
    copyRemote.textContent = isLocal() ? status?.localLanAddress ?? t('局域网') : t('远程');
    copyRemote.title = isLocal() ? status?.localLanAddress ?? t('未检测到局域网 IPv4 地址') : status?.controllerAddress ?? '';
    copyRemote.setAttribute('aria-label', isLocal() ? t('复制局域网代理命令') : t('复制当前端点命令'));
    const mips = tunStacks.get('mips'); if (mips) mips.title = supportsMips(status?.version) ? 'mips' : t('mips 需要 mihomo 1.19.31 或更新版本');
    controller.row.hidden = !isLocal();
    mixed.input.min = '0';
    settingsHint.textContent = isLocal() ? t("更换内核或本机端口前请先停止内核。TUN 按需请求系统授权。") : t("端口修改作用于远程内核，0 表示关闭。本机启动设置不受影响。");
    basicSettingsHeading.querySelector('h2')!.textContent = isLocal() ? t("基础设置") : t("应用设置 (本地)");
    coreSettingsHeading.querySelector('h2')!.textContent = isLocal() ? t("内核设置") : t("内核设置 (远程)");
    if (status && !portsDirty) {
        if (isLocal()) {
            controller.input.value = String(status.controllerPort);
            for (const [key, field] of Object.entries(remotePortFields)) {
                field.input.value = String(status.localProxyPorts?.[key as keyof typeof remotePortFields] ?? (key === 'mixed-port' ? status.mixedPort : 0));
                field.input.title = '';
            }
        }
        else {
            for (const [key, field] of Object.entries(remotePortFields)) {
                const value = snapshot?.configs[key as keyof typeof remotePortFields];
                field.input.value = value == null ? '' : String(value);
                field.input.min = '0';
                field.input.title = value == null ? t("当前内核未返回此端口。") : '';
            }
        }
    }
    const error = actionError || refreshError || (status?.lastError ? safeError(status.lastError) : '') || (activeTab === 'logs' && status?.logStreamError ? safeError(status.logStreamError) : '');
    banner.hidden = !error;
    errorCopy.textContent = error;
    if (busy)
        notice.textContent = `${pendingLabel}…`;
    for (const [mode, control] of modeButtons) {
        control.setAttribute('aria-pressed', String(snapshot?.configs.mode?.toLowerCase() === mode));
    }
    sortButton.setAttribute('aria-pressed', String(sortLatency));
    historyButton.setAttribute('aria-pressed', String(showHistory));
    hiddenButton.setAttribute('aria-pressed', String(!hideHidden));
    groupRulesButton.setAttribute('aria-pressed', String(groupRules));
    settingsLogLevel.update();
    logLevel.update();
    requestResize();
    for (const [key, control] of startupSwitches) {
        control.checked = status?.[key] ?? false;
        control.indeterminate = status != null && status[key] == null;
    }
    startupError.textContent = status?.launchAtLoginError ? safeError(status.launchAtLoginError) : '';
    startupError.hidden = !startupError.textContent;
    for (const [key, control] of coreSwitches) {
        if (key === 'allow-lan' && control.parentElement)
            control.parentElement.title = isLocal() ? t("允许同一局域网内的设备连接代理端口；控制器仍只接受本机连接。") : t("只修改远程代理的局域网访问设置，不修改远程控制器监听地址。");
        control.checked = snapshot?.configs[key] ?? false;
        control.title = !status?.running ? t("启动内核后更改此设置。") : typeof snapshot?.configs[key] !== 'boolean' ? t("当前内核未返回此设置。") : '';
    }
}
function activateTab(tab: Tab) {
    scrollPositions.set(activeTab, content.scrollTop);
    activeTab = tab;
    void dismissMenu();
    shell.dataset.layout = ['proxies', 'settings'].includes(tab) ? 'dynamic' : 'fixed';
    for (const [key, panel] of panels)
        panel.hidden = key !== tab;
    for (const [key, control] of tabButtons) {
        control.setAttribute('aria-selected', String(key === tab));
        control.tabIndex = key === tab ? 0 : -1;
    }
    document.title = `ClashBar · ${tabs.find(([key]) => key === tab)![1]}`;
    renderActive();
    content.scrollTop = scrollPositions.get(tab) ?? 0;
    if (desktop && !busy)
        void refresh();
}
function renderActive() { if (activeTab === 'proxies')
    renderProxies(); if (activeTab === 'rules')
    renderRules(); if (activeTab === 'connections')
    renderConnections(); if (activeTab === 'logs')
    renderLogs(); updateChrome(); }
function stopped() { return emptyState(status?.running ? t("正在读取数据") : t("内核未启动"), status?.running ? t("读取失败时可重新刷新。") : status?.corePath && status?.configName ? t("点击顶部启动按钮。") : t("选择 mihomo 内核并导入配置后启动。")); }
function currentGroups() { return Object.entries(snapshot?.proxies.proxies ?? {}).filter(([name, proxy]) => proxy.all && (!hideHidden || !proxy.hidden) && (name !== 'GLOBAL' || snapshot?.configs.mode === 'global')); }
function nodeDelay(name: string, proxy?: Proxy) { return delays.get(name) ?? snapshot?.proxies.proxies[name]?.history?.at(-1)?.delay ?? proxy?.history?.at(-1)?.delay; }
function delayText(delay?: number) { return delay == null ? '—' : delay === 0 ? t("超时") : String(delay); }
async function testNode(name: string) { await mutate(t("测试延迟"), async () => { const result = await invoke<{
    delay: number;
}>('test_delay', { name }); delays.set(name, result.delay); await updateMenuValue(name, delayText(result.delay)); }); }
function groupMenu(anchor: HTMLElement, name: string, group: Proxy, focus = true) {
    let names = [...new Set(group.all ?? [])];
    if (sortLatency)
        names = names.sort((a, b) => (nodeDelay(a) || Infinity) - (nodeDelay(b) || Infinity));
    choose(anchor, `${name}  ${names.length}`, names.map(node => ({ label: node, detail: snapshot?.proxies.proxies[node]?.type, value: delayText(nodeDelay(node)), checked: group.now === node, action: () => mutate(t("切换节点"), () => invoke('select_proxy', { group: name, name: node })), secondaryLabel: t("测试 {0} 延迟", node), secondaryAction: () => testNode(node) })), focus);
}
function renderProxies() {
    renderTraffic();
    const groups = currentGroups();
    groupCounter.textContent = String(groups.length);
    const groupNodes = groups.map(([name, group], index) => {
        const row = el('div', 'proxy-group-row');
        const trigger: HTMLButtonElement = gate(button('', () => groupMenu(trigger, name, group), 'group-trigger'), canRun);
        trigger.id = `group-${index}`;
        trigger.setAttribute('aria-label', t("代理组 {0}", name));
        trigger.setAttribute('aria-haspopup', 'dialog');
        trigger.addEventListener('pointerenter', () => { if (!trigger.disabled)
            scheduleMenuOpen(() => groupMenu(trigger, name, group, false)); });
        row.addEventListener('pointerleave', scheduleMenuClose);
        const title = el('span', 'group-name', name);
        title.title = name;
        const selected = el('span', 'selected-node capsule', group.now || '—');
        selected.title = group.now || '';
        const delay = nodeDelay(group.now || name, group);
        const latency = el('span', 'latency', delayText(delay));
        latency.dataset.tone = delay === 0 ? 'error' : delay == null ? 'unknown' : delay > 500 ? 'warning' : 'success';
        trigger.append(title, selected, latency);
        if (group.icon && desktop) {
            const iconKey = `${status?.targetRevision ?? 0}:${name}:${group.icon}`;
            let pendingIcon = groupIcons.get(iconKey);
            if (!pendingIcon) { pendingIcon = invoke<string>('get_proxy_group_icon', { name }).catch(() => null); groupIcons.set(iconKey, pendingIcon); }
            void pendingIcon.then(data => {
                if (!data || !data.startsWith('data:image/') || !trigger.isConnected) return;
                const image = el('img', 'group-icon'); image.alt = ''; image.width = image.height = 16;
                image.addEventListener('error', () => image.remove(), { once: true }); image.src = data; trigger.prepend(image);
            });
        }
        const samples = snapshot?.proxies.proxies[group.now ?? '']?.history ?? group.history;
        if (showHistory && samples?.length) {
            const history = el('span', 'history-bars');
            for (const sample of samples.slice(-8)) {
                const bar = el('i');
                bar.style.height = `${Math.min(14, Math.max(2, sample.delay / 40))}px`;
                history.append(bar);
            }
            latency.replaceChildren(history, document.createTextNode(delayText(delay)));
        }
        const test = gate(iconButton('gauge', t("测试 {0} 分组延迟", name), () => mutate(t("测试分组延迟"), async () => { const result = await invoke<Record<string, number>>('test_group_delay', { name }); for (const [node, value] of Object.entries(result))
            delays.set(node, value); })), canRun);
        test.id = `delay-${index}`;
        row.append(trigger, test);
        return row;
    });
    if (!menuIsOpen())
        replacePreservingFocus(proxyGroups, ...(status?.running && snapshot ? groupNodes.length ? groupNodes : [emptyState(t("暂无代理组"), t("当前配置没有可用的代理组。"))] : [stopped()]));
    const entries = Object.entries(snapshot?.providers.providers ?? {}).filter(([name, provider]) => name.toLowerCase() !== 'default' && provider.vehicleType?.toLowerCase() !== 'compatible');
    providerCounter.textContent = String(entries.length);
    providers.hidden = providerCollapsed;
    setIcon(collapseProviders, providerCollapsed ? 'chevron' : 'down', providerCollapsed ? t("展开代理提供者") : t("折叠代理提供者"));
    collapseProviders.setAttribute('aria-expanded', String(!providerCollapsed));
    const providerNodes = entries.map(([name, provider], index) => {
        const row = el('div', 'provider-item');
        const top = el('div', 'provider-row');
        top.append(icon('drive', 'teal'), el('strong', 'provider-name', name), el('span', 'muted', String(provider.proxies?.length ?? 0)), el('span', 'flex-space'));
        const updated = provider.updatedAt ? relativeTime(provider.updatedAt) : '—';
        top.append(el('span', 'provider-age', updated));
        const update = gate(iconButton('refresh', t("更新代理提供者 {0}", name), () => mutate(t("更新代理提供者"), () => invoke('update_provider', { name }))), canRun);
        update.id = `provider-${index}`;
        top.append(update);
        row.append(top);
        const info = provider.subscriptionInfo;
        if (info && info.total > 0) {
            const usage = el('div', 'provider-usage');
            const labels = el('div', 'usage-labels');
            const days = info.expire ? Math.ceil((info.expire * 1000 - Date.now()) / 86400000) : null;
            labels.append(el('span', 'orange', days == null ? t("不限时") : days < 0 ? t("已过期") : t("剩 {0} 天", days)), el('span', '', `${compactBytes(info.upload + info.download)} / ${compactBytes(info.total)}`));
            const progress = el('progress');
            progress.max = info.total;
            progress.value = info.upload + info.download;
            progress.setAttribute('aria-label', t("{0} 订阅用量", name));
            usage.append(labels, progress);
            row.append(usage);
        }
        return row;
    });
    replacePreservingFocus(providers, ...(providerNodes.length ? providerNodes : [el('p', 'compact-empty', t("暂无代理提供者"))]));
    updateChrome();
}
function compactBytes(bytes?: number) { return formatBytes(bytes).replace('KiB', 'KB').replace('MiB', 'MB').replace('GiB', 'GB').replace('TiB', 'TB'); }
function relativeTime(raw: string) { const seconds = Math.max(0, Math.floor((Date.now() - Date.parse(raw)) / 1000)); if (!Number.isFinite(seconds))
    return '—'; return seconds < 60 ? t("刚刚") : seconds < 3600 ? t("{0}m前", Math.floor(seconds / 60)) : t("{0}h前", Math.floor(seconds / 3600)); }
function renderTraffic() {
    const data = status?.running ? snapshot : null;
    connectionCount.replaceChildren(icon('link', 'purple'), document.createTextNode(data ? String(data.connections.connections.length) : '—'));
    memoryMetric.replaceChildren(icon('chip', 'teal'), document.createTextNode(data ? compactBytes(data.memory ?? undefined) : '—'));
    upMetric.replaceChildren(document.createTextNode(`${rateUp == null ? '—' : compactBytes(rateUp) + '/s'} · ${data ? compactBytes(data.connections.uploadTotal) : '—'}`), icon('up', 'blue'));
    downMetric.replaceChildren(document.createTextNode(`${rateDown == null ? '—' : compactBytes(rateDown) + '/s'} · ${data ? compactBytes(data.connections.downloadTotal) : '—'}`), icon('arrowDown', 'green'));
    chart.replaceChildren();
    const maximum = Math.max(1, ...trafficSamples.flatMap(sample => [sample.up, sample.down]));
    for (const direction of ['up', 'down'] as const) {
        const line = document.createElementNS(chart.namespaceURI, 'polyline');
        const points = trafficSamples.map((sample, i) => `${336 - (trafficSamples.length - 1 - i) / 59 * 336},${30 + (direction === 'up' ? -1 : 1) * sample[direction] / maximum * 27}`).join(' ');
        line.setAttribute('points', points);
        line.setAttribute('class', `traffic-line ${direction}`);
        chart.append(line);
    }
}
function classifyRule(rule: Rule) { const type = rule.type.toLowerCase(); return type.includes('domain') ? t("域名") : type.includes('ip') || type.includes('cidr') || type.includes('geoip') ? 'IP' : type.includes('ruleset') || type.includes('rule-set') ? t("规则集") : t("其他"); }
function renderRules() {
    const all = snapshot?.rules.rules ?? [];
    if (snapshot && rulePolicy && !all.some(rule => rule.proxy === rulePolicy)) rulePolicy = '';
    const base = all.filter(rule => matchesQuery([rule.type, rule.payload, rule.proxy], ruleQuery) && (!rulePolicy || rule.proxy === rulePolicy));
    const filtered = ruleType === t("全部") ? base : base.filter(rule => classifyRule(rule) === ruleType);
    const providerCount = snapshot?.ruleProviders ? Object.keys(snapshot.ruleProviders.providers).length : null;
    ruleStats.textContent = t("规则 {0}  规则集 {1}", all.length, providerCount ?? '—');
    policySelect.update();
    ruleChips.replaceChildren(...[t("全部"), t("域名"), 'IP', t("规则集"), t("其他")].map(type => { const count = type === t("全部") ? base.length : base.filter(rule => classifyRule(rule) === type).length; const chip = button(`${type} ${count}`, () => { ruleType = type; renderRules(); }, 'filter-chip'); chip.setAttribute('aria-pressed', String(ruleType === type)); return chip; }));
    if (!status?.running || !snapshot) {
        ruleContent.replaceChildren(stopped());
        return;
    }
    if (!filtered.length) {
        ruleContent.replaceChildren(emptyState(all.length ? t("无匹配规则") : t("暂无规则数据"), all.length ? t("修改或清除筛选条件。") : t("当前配置没有路由规则。")));
        return;
    }
    const head = el('div', 'rule-head');
    head.append(el('span', '', t("目标 / 类型")), el('span', '', t("策略")), el('span', '', t("统计")));
    if (groupRules) {
        const groups = new Map<string, Rule[]>();
        for (const rule of filtered)
            groups.set(rule.proxy, [...(groups.get(rule.proxy) ?? []), rule]);
        const nodes = [...groups.entries()].sort((a, b) => b[1].length - a[1].length || a[0].localeCompare(b[0], locale)).map(([policy, rules]) => {
            const group = el('section', 'policy-group');
            const toggle = button('', () => { if (expandedPolicies.has(policy))
                expandedPolicies.delete(policy);
            else
                expandedPolicies.add(policy); renderRules(); }, 'policy-heading');
            toggle.append(icon(expandedPolicies.has(policy) ? 'down' : 'chevron'), el('strong', '', policy), el('span', 'count-badge', String(rules.length)));
            toggle.setAttribute('aria-expanded', String(expandedPolicies.has(policy)));
            group.append(toggle);
            if (expandedPolicies.has(policy))
                group.append(ruleList(rules));
            return group;
        });
        ruleContent.replaceChildren(head, ...nodes);
    }
    else
        ruleContent.replaceChildren(head, ruleList(filtered));
    updateChrome();
}
function ruleList(rules: Rule[]) {
    const providerLookup = new Map<string, RuleProvider>();
    for (const [key, provider] of Object.entries(snapshot?.ruleProviders?.providers ?? {})) {
        providerLookup.set(key.toLowerCase(), provider);
        if (provider.name?.trim())
            providerLookup.set(provider.name.trim().toLowerCase(), provider);
    }
    const host = el('div', 'virtual-rules');
    host.style.height = `${rules.length * 32}px`;
    host.setAttribute('role', 'table');
    host.setAttribute('aria-label', t("路由规则"));
    host.setAttribute('aria-rowcount', String(rules.length));
    let previousStart = -1;
    const render = () => {
        if (!host.isConnected)
            return;
        const offset = content.getBoundingClientRect().top - host.getBoundingClientRect().top;
        const start = Math.max(0, Math.floor(offset / 32) - 6);
        if (start === previousStart)
            return;
        previousStart = start;
        const rows = rules.slice(start, start + 50).map((rule, i) => {
            const row = el('div', 'rule-row');
            row.style.top = `${(start + i) * 32}px`;
            row.setAttribute('role', 'row');
            row.setAttribute('aria-rowindex', String(start + i + 1));
            const target = el('span', 'rule-target');
            target.setAttribute('role', 'cell');
            target.append(icon(classifyRule(rule) === 'IP' ? 'network' : 'globe', 'muted-icon'));
            const text = el('span');
            const name = el('span', 'truncate', rule.payload || '—');
            name.title = rule.payload;
            text.append(name, el('small', '', rule.type));
            target.append(text);
            const policy = el('span', 'rule-policy truncate', rule.proxy);
            policy.setAttribute('role', 'cell');
            policy.title = rule.proxy;
            const provider = providerLookup.get(rule.payload.trim().toLowerCase());
            const count = el('span', 'rule-stat muted', provider?.ruleCount == null ? (provider || !snapshot?.ruleProviders ? '—' : '0') : String(Math.max(0, provider.ruleCount)));
            count.setAttribute('role', 'cell');
            if (provider?.updatedAt) {
                const updated = el('small', '', relativeTime(provider.updatedAt));
                updated.title = provider.updatedAt;
                count.append(updated);
            }
            row.append(target, policy, count);
            return row;
        });
        host.replaceChildren(...rows);
    };
    const onScroll = () => { if (!host.isConnected)
        content.removeEventListener('scroll', onScroll);
    else
        render(); };
    content.addEventListener('scroll', onScroll, { passive: true });
    requestAnimationFrame(render);
    return host;
}
function connectionName(connection: Connection) { return connection.metadata.host || connection.metadata.destinationIP || connection.id; }
function renderConnections() {
    if (!status?.running || !snapshot) {
        connectionFraction.textContent = '0/0';
        connectionContent.replaceChildren(stopped());
        return;
    }
    const all = snapshot.connections.connections ?? [];
    let visible = all.slice(0, 120).filter(connection => {
        const net = connection.metadata.network?.toUpperCase();
        const transportMatch = !transport || (transport === t("仅 TCP") ? net === 'TCP' : transport === t("仅 UDP") ? net === 'UDP' : Boolean(net) && !['TCP', 'UDP'].includes(net!));
        return transportMatch && matchesQuery([connectionName(connection), connection.id, connection.metadata.sourceIP, connection.metadata.destinationIP, connection.metadata.process, connection.metadata.network, connection.start, connection.rule, connection.rulePayload, ...(connection.chains ?? [])], connectionQuery);
    });
    if (connectionSort !== t("默认顺序"))
        visible = visible.sort((a, b) => connectionSort === t("最新连接") ? Date.parse(b.start || '') - Date.parse(a.start || '') : connectionSort === t("最早连接") ? Date.parse(a.start || '') - Date.parse(b.start || '') : connectionSort === t("上传流量高到低") ? (b.upload ?? 0) - (a.upload ?? 0) : connectionSort === t("下载流量高到低") ? (b.download ?? 0) - (a.download ?? 0) : (b.upload ?? 0) + (b.download ?? 0) - (a.upload ?? 0) - (a.download ?? 0));
    connectionFraction.textContent = `${visible.length}/${Math.min(all.length, 120)}`;
    const rows = visible.map(connection => {
        const row = el('article', 'connection-row');
        const leading = icon('globe', 'blue');
        const body = el('div', 'connection-body');
        const top = el('div', 'connection-top');
        const name = el('strong', 'truncate', connectionName(connection));
        name.title = connectionName(connection);
        const rule = el('span', 'capsule rule-badge truncate', [connection.rule, connection.rulePayload].filter(Boolean).join(' · ') || '—'); rule.title = rule.textContent || '';
        top.append(name, rule);
        const metrics = el('div', 'connection-metrics');
        metrics.append(el('span', '', connection.start ? timeText(connection.start) : '—'), el('span', '', connection.metadata.network?.toUpperCase() || '—'), el('span', 'blue', `↑ ${compactBytes(connection.upload)}`), el('span', 'green', `↓ ${compactBytes(connection.download)}`));
        const chains = el('div', 'connection-chains truncate', [...(connection.chains ?? [])].reverse().join(' › ') || '—');
        chains.title = chains.textContent || '';
        body.append(top, metrics, chains);
        const close = gate(iconButton('x', t("关闭连接 {0}", connectionName(connection)), () => closeConnection(connection), 'row-close danger-text'), canRun);
        close.id = `close-${connection.id}`;
        row.append(leading, body, close);
        row.addEventListener('contextmenu', event => { event.preventDefault(); choose(row, connectionName(connection), [{ label: t("关闭连接"), action: () => closeConnection(connection) }, { label: t("复制 Host"), action: () => copyText(connectionName(connection), t("已复制 Host")) }, { label: t("复制连接 ID"), action: () => copyText(connection.id, t("已复制连接 ID")) }]); });
        return row;
    });
    replacePreservingFocus(connectionContent, ...(rows.length ? rows : [emptyState(all.length ? t("无匹配连接") : t("暂无活动连接"), all.length ? t("修改或清除筛选条件。") : t("通过代理访问网络后，连接会显示在这里。"))]));
    updateChrome();
}
function closeConnection(connection: Connection) { return mutate(t("关闭连接"), () => invoke('close_connection', { id: connection.id })); }
function timeText(value: string) { const date = new Date(value); return Number.isFinite(date.valueOf()) ? date.toLocaleTimeString(locale, { hour12: false }) : '—'; }
function logSeverity(line: string) { return /\b(error|fatal|panic)\b/i.test(line) ? t("错误") : /\bwarn(ing)?\b/i.test(line) ? t("警告") : t("信息"); }
function fullLogRecord(item: LogRow) {
    const level = item.line.match(/\b(error|fatal|panic|warn(?:ing)?|info|debug|trace)\b/i)?.[1].toUpperCase() ?? 'INFO';
    const date = new Date(item.timestamp), pad = (value: number) => String(value).padStart(2, '0');
    const timestamp = `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
    return `[${timestamp}] [${item.source.toUpperCase()}] [${level}] ${item.line}`;
}
function renderLogs() {
    sourceChips.replaceChildren(...[t("全部"), 'ClashBar', 'Mihomo'].map(source => { const chip = button(source, () => { if (source === t("全部"))
        sourceFilter.clear();
    else if (sourceFilter.has(source))
        sourceFilter.delete(source);
    else
        sourceFilter.add(source); renderLogs(); }, 'filter-chip'); chip.setAttribute('aria-pressed', String(source === t("全部") ? !sourceFilter.size : sourceFilter.has(source))); return chip; }));
    levelChips.replaceChildren(...[t("全部"), t("信息"), t("警告"), t("错误")].map(level => { const chip = button(level, () => { if (level === t("全部"))
        levelFilter.clear();
    else if (levelFilter.has(level))
        levelFilter.delete(level);
    else
        levelFilter.add(level); renderLogs(); }, 'filter-chip'); chip.setAttribute('aria-pressed', String(level === t("全部") ? !levelFilter.size : levelFilter.has(level))); return chip; }));
    const all = [...combinedLogs].reverse();
    const visible = all.slice(0, 120).filter(item => (!sourceFilter.size || sourceFilter.has(item.source)) && (!levelFilter.size || levelFilter.has(logSeverity(item.line))) && matchesQuery([item.line], logQuery));
    logFraction.textContent = `${visible.length}/${all.length}`;
    const rows = visible.map(item => {
        const row = el('article', 'log-row'); const level = logSeverity(item.line);
        row.append(icon(level === t("错误") ? 'x' : level === t("警告") ? 'warning' : 'info', level === t("错误") ? 'danger-text' : level === t("警告") ? 'orange' : 'blue'));
        const body = el('div', 'log-body'); const timestamp = timeText(new Date(item.timestamp).toISOString()); const protocol = item.line.match(/\[(TCP|UDP)\]/)?.[1];
        body.append(el('div', 'log-meta', [item.source, protocol, timestamp].filter(Boolean).join(' • ')), el('p', 'log-message', item.line)); row.append(body);
        const openContext = () => choose(row, t("日志"), [{ label: t('复制消息'), action: () => copyText(item.line, t("已复制日志")) }, { label: t('复制完整记录'), action: () => copyText(fullLogRecord(item), t("已复制日志")) }]);
        row.tabIndex = 0; row.addEventListener('contextmenu', event => { event.preventDefault(); openContext(); });
        row.addEventListener('keydown', event => { if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) { event.preventDefault(); openContext(); } });
        return row;
    });
    logContent.replaceChildren(...(rows.length ? rows : [emptyState(logQuery || sourceFilter.size || levelFilter.size ? t("无匹配日志") : t("暂无日志"), logQuery || sourceFilter.size || levelFilter.size ? t("修改或清除筛选条件。") : t("启动内核后查看日志。"))]));
    updateChrome();
}
async function copyText(text: string, feedback: string) { try {
    await navigator.clipboard.writeText(text);
    notice.textContent = feedback;
    return true;
}
catch {
    actionError = t("复制失败，请检查剪贴板权限后重试。");
    updateChrome();
    return false;
} }
function sampleTraffic(next: Snapshot) {
    const now = performance.now();
    if (next.traffic) {
        rateUp = Math.max(0, next.traffic.up);
        rateDown = Math.max(0, next.traffic.down);
    } else if (lastSample && now > lastSample.time) {
        const seconds = (now - lastSample.time) / 1000;
        rateUp = Math.max(0, (next.connections.uploadTotal - lastSample.up) / seconds);
        rateDown = Math.max(0, (next.connections.downloadTotal - lastSample.down) / seconds);
    }
    if (rateUp !== null && rateDown !== null) {
        trafficSamples.push({ up: rateUp, down: rateDown });
        if (trafficSamples.length > 60)
            trafficSamples.shift();
    }
    lastSample = { time: now, up: next.connections.uploadTotal, down: next.connections.downloadTotal };
}
async function refresh(nativeVisible = false) {
    // 原生窗口显示事件可能早于 WebView2 的可见性更新。
    if (!desktop || busy || refreshing || (!nativeVisible && document.hidden) || !popupVisible)
        return;
    const ticket = epoch.next();
    refreshing = true;
    updateChrome();
    try {
        const nextStatus = await invoke<Status>('get_status');
        if (!epoch.current(ticket))
            return;
        acceptStatus(nextStatus);
        if (!nextStatus.running) {
            snapshot = null;
            lastSample = null;
            rateUp = rateDown = null;
            trafficSamples.length = 0;
        }
        else {
            const next = await invoke<Snapshot>('get_snapshot');
            if (!epoch.current(ticket))
                return;
            sampleTraffic(next);
            snapshot = next;
        }
        if (activeTab === 'logs') {
            const nextLogs = await invoke<{
                timestamp: number;
                source: string;
                message: string;
            }[]>('get_log_entries');
            if (!epoch.current(ticket))
                return;
            logs = nextLogs.slice(-LOG_LIMIT).map(item => ({ timestamp: item.timestamp, source: item.source, line: item.message }));
            combinedLogs = mergeLogs(appLogs, logs);
        }
        refreshError = '';
    }
    catch (error) {
        if (epoch.current(ticket))
            refreshError = t("读取失败，显示上次数据。{0}", safeError(error));
    }
    finally {
        if (epoch.current(ticket)) {
            refreshing = false;
            renderActive();
        }
    }
}
async function mutate(label: string, operation: () => Promise<unknown>, reload = true): Promise<boolean> {
    if (!desktop || busy)
        return false;
    epoch.next();
    refreshing = false;
    busy = true;
    pendingLabel = label;
    actionError = '';
    updateChrome();
    let success = false;
    try {
        const result = await operation();
        if (result && typeof result === 'object' && 'running' in result)
            acceptStatus(result as Status);
        success = true;
    }
    catch (error) {
        actionError = t("{0}未完成。{1}", label, safeError(error));
    }
    finally {
        pendingLabel = '';
        if (label !== t("清理全部日志") || !success) {
            let record = { timestamp: Date.now(), source: 'ClashBar', line: `${success ? 'info' : 'error'} ${label}${success ? t("已完成") : t("未完成")}` };
            try {
                const saved = await invoke<{ timestamp: number; source: string; message: string }>('record_app_action', { message: record.line });
                if (typeof saved?.timestamp === 'number' && typeof saved.message === 'string') record = { timestamp: saved.timestamp, source: saved.source, line: saved.message };
            } catch { /* 日志写入失败不覆盖原操作结果，当前会话保留内存记录。 */ }
            appLogs.push(record);
            appLogs = appLogs.slice(-LOG_LIMIT);
            combinedLogs = mergeLogs(combinedLogs, [record]);
        }
        busy = false;
        renderActive();
    }
    if (reload)
        await refresh();
    if (success)
        notice.textContent = t("{0}已完成。", label);
    return success;
}
function mergeLogs(...buffers: LogRow[][]) {
    const unique = new Map<string, LogRow>();
    for (const item of buffers.flat()) unique.set(JSON.stringify([item.timestamp, item.source, item.line]), item);
    return [...unique.values()].sort((left, right) => left.timestamp - right.timestamp).slice(-LOG_LIMIT);
}
function requestResize() {
    if (!desktop || !['proxies', 'settings'].includes(activeTab) || document.querySelector('.machine-manager[open]'))
        return;
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => {
        if (!['proxies', 'settings'].includes(activeTab) || document.querySelector('.machine-manager[open]'))
            return;
        const desired = Math.min(window.screen.availHeight || 900, Math.ceil(header.offsetHeight + modes.offsetHeight + navigation.offsetHeight + footer.offsetHeight + panels.get(activeTab)!.scrollHeight + (banner.hidden ? 0 : banner.offsetHeight) + 3));
        if (Math.abs(desired - lastRequestedHeight) < 2)
            return;
        lastRequestedHeight = desired;
        void invoke('resize_popup', { height: desired }).catch(() => { lastRequestedHeight = 0; });
    }, 30);
}
function resetEphemeralState() { ruleQuery = connectionQuery = logQuery = ''; ruleSearch.input.value = connectionSearch.input.value = logSearch.input.value = ''; rulePolicy = ''; ruleType = t("全部"); sourceFilter.clear(); levelFilter.clear(); expandedPolicies.clear(); scrollPositions.clear(); content.scrollTop = 0; void dismissMenu(); }
async function initializeNative() {
    if (!desktop)
        return;
    await initializeMenus();
    pinned = await invoke<boolean>('get_popup_pinned');
    unlisten.push(await listen<{
        visible: boolean;
        pinned: boolean;
    }>('popup-visibility', event => { const reopening = !popupVisible && event.payload.visible; popupVisible = event.payload.visible; pinned = event.payload.pinned; if (reopening) {
        resetEphemeralState();
        lastSample = null;
    } if (popupVisible)
        void refresh(true); updateChrome(); }));
    unlisten.push(await listen<{
        tab: string;
    }>('popup-tab', event => { if (event.payload.tab === 'system')
        activateTab('settings'); }));
    updateChrome();
}
document.addEventListener('keydown', event => { if (event.key === 'Escape' && !event.isComposing && !event.defaultPrevented && !document.querySelector('dialog[open]')) {
    event.preventDefault();
    if (menuIsOpen())
        void dismissMenu();
    else if (desktop)
        void invoke('hide_popup');
} });
document.addEventListener('keydown', event => {
    if (event.isComposing || event.repeat || !(event.ctrlKey || event.metaKey) || document.querySelector('dialog[open]') || (event.target instanceof Element && event.target.closest('input,textarea,[contenteditable="true"]'))) return;
    const key = event.key.toLowerCase();
    let action: (() => void) | undefined;
    if (event.altKey && /^[1-5]$/.test(key)) action = () => activateTab(tabs[Number(key) - 1][0]);
    else if (event.shiftKey && /^[1-3]$/.test(key)) action = () => modeButtons.get(['rule', 'global', 'direct'][Number(key) - 1])?.click();
    else if (event.code.startsWith('Digit') && event.shiftKey && !event.altKey && ['1', '2', '3'].includes(event.code.slice(-1))) action = () => modeButtons.get(['rule', 'global', 'direct'][Number(event.code.slice(-1)) - 1])?.click();
    else if (event.altKey && key === 'c') action = () => { (isLocal() ? copyTerminal : copyRemote).click(); };
    else if (event.shiftKey && key === 'r') action = () => restartButton.click();
    else if (event.shiftKey && event.code === 'Period') action = () => { if (status?.running) runButton.click(); };
    else if (!event.shiftKey && !event.altKey && key === ',') action = () => activateTab('settings');
    else if (!event.shiftKey && !event.altKey && key === 's') action = () => systemProxy.click();
    else if (!event.shiftKey && !event.altKey && key === 'e') action = () => tunSwitch.click();
    if (action) { event.preventDefault(); action(); }
});
systemProxy.setAttribute('aria-keyshortcuts', 'Control+S Meta+S');
tunSwitch.setAttribute('aria-keyshortcuts', 'Control+E Meta+E');
window.addEventListener('beforeunload', event => { if (portsDirty) {
    event.preventDefault();
    event.returnValue = '';
} });
document.addEventListener('visibilitychange', () => { if (!document.hidden)
    void refresh(); });
window.addEventListener('unload', () => { for (const cleanup of unlisten)
    cleanup(); });
new ResizeObserver(requestResize).observe(shell);
applyAppearance();
const resumeTab = sessionStorage.getItem('clashbar-resume-tab');
sessionStorage.removeItem('clashbar-resume-tab');
activateTab(tabs.some(([tab]) => tab === resumeTab) ? resumeTab as Tab : 'proxies');
void initializeNative().catch(error => { actionError = t("托盘连接失败。{0}", safeError(error)); updateChrome(); });
setInterval(() => { void refresh(); }, 3000);
