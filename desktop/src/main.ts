import { invoke, isTauri } from '@tauri-apps/api/core';
import type { Connection, Snapshot, Status, Tab } from './types';
import { formatBytes, LOG_LIMIT, matchesQuery, pageSlice, ReadEpoch, safeError, validatePorts, validateSubscription } from './utils';
import { button, confirmAction, el, emptyState, field, replacePreservingFocus, searchField, setFieldError, table } from './ui';
import './style.css';

const desktop = isTauri();
const epoch = new ReadEpoch();
const tabs: [Tab, string][] = [['proxies', '代理'], ['rules', '规则'], ['connections', '连接'], ['logs', '日志'], ['settings', '设置']];
let status: Status | null = null;
let snapshot: Snapshot | null = null;
let logs: string[] = [];
let activeTab: Tab = 'proxies';
let busy = false;
let refreshing = false;
let pendingLabel = '';
let refreshError = '';
let actionError = '';
let lastUpdated: Date | null = null;
let ruleQuery = '', connectionQuery = '', logQuery = '';
let rulePage = 1, connectionPage = 1;
const delays = new Map<string, number>();
const gated = new Map<HTMLButtonElement | HTMLInputElement | HTMLSelectElement, () => boolean>();

const app = document.querySelector<HTMLDivElement>('#app')!;
const shell = el('main', 'app-shell');
const header = el('header', 'app-header');
const brand = el('div', 'brand');
const brandMark = el('span', 'brand-mark', 'C'); brandMark.setAttribute('aria-hidden', 'true');
const brandCopy = el('div'); brandCopy.append(el('h1', '', 'ClashBar'), el('p', 'brand-caption', 'Windows · mihomo'));
brand.append(brandMark, brandCopy);
const coreBadge = el('span', 'status-badge', '正在连接');
header.append(brand, coreBadge);
const controls = el('section', 'control-strip'); controls.setAttribute('aria-label', '内核运行控制');
const coreSummary = el('div', 'core-summary');
const configLabel = el('strong', 'config-name', '读取配置状态…');
const versionLabel = el('span', 'muted', ''); coreSummary.append(configLabel, versionLabel);
const runButton = gate(button('启动内核', () => mutate(status?.running ? '停止内核' : '启动内核', () => invoke<Status>(status?.running ? 'stop_core' : 'start_core')), 'primary'), () => Boolean(status && (status.running || (status.corePath && status.configName))));
const proxyLabel = el('label', 'switch-control');
const proxyToggle = el('input'); proxyToggle.type = 'checkbox'; proxyToggle.id = 'system-proxy';
gate(proxyToggle, () => Boolean(status?.running || status?.systemProxy));
proxyToggle.addEventListener('change', () => {
  const enabled = proxyToggle.checked;
  proxyToggle.checked = status?.systemProxy ?? false;
  if (enabled) confirmAction('开启系统代理？', '将当前 Windows 用户的系统代理指向 ClashBar。停止内核或退出时会恢复原设置。', '开启系统代理', () => mutate('开启系统代理', () => invoke<Status>('set_system_proxy', { enabled })));
  else void mutate('关闭系统代理', () => invoke<Status>('set_system_proxy', { enabled }));
});
proxyLabel.append(proxyToggle, el('span', '', '系统代理'));
controls.append(coreSummary, runButton, proxyLabel);
const navigation = el('nav', 'tabs'); navigation.setAttribute('role', 'tablist'); navigation.setAttribute('aria-label', '功能页面');
const panels = new Map<Tab, HTMLElement>();
const tabButtons = new Map<Tab, HTMLButtonElement>();
for (const [key, title] of tabs) {
  const tab = button(title, () => activateTab(key), 'tab');
  tab.id = `tab-${key}`; tab.setAttribute('role', 'tab'); tab.setAttribute('aria-controls', `panel-${key}`);
  tab.addEventListener('keydown', event => {
    if (event.isComposing || !['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    const index = tabs.findIndex(([id]) => id === key);
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? tabs.length - 1 : (index + (event.key === 'ArrowRight' ? 1 : -1) + tabs.length) % tabs.length;
    activateTab(tabs[next][0]); tabButtons.get(tabs[next][0])?.focus();
  });
  navigation.append(tab); tabButtons.set(key, tab);
  const panel = el('section', 'tab-panel'); panel.id = `panel-${key}`; panel.setAttribute('role', 'tabpanel'); panel.setAttribute('aria-labelledby', tab.id); panel.tabIndex = 0;
  panels.set(key, panel);
}
const notice = el('div', 'notice'); notice.setAttribute('role', 'status'); notice.setAttribute('aria-live', 'polite');
const errors = el('div', 'error-banner'); errors.setAttribute('role', 'alert'); errors.hidden = true;
const errorCopy = el('span');
const retry = button('重新读取', () => refresh());
errors.append(errorCopy, retry);
const content = el('div', 'content'); content.append(notice, errors, ...panels.values());
const footer = el('footer', 'app-footer');
const freshness = el('span', '', '尚未读取内核');
const refreshButton = button('刷新', () => refresh(), 'ghost');
footer.append(freshness, refreshButton);
shell.append(header, controls, navigation, content, footer); app.append(shell);

const proxyPanel = panels.get('proxies')!;
const modeRow = el('div', 'section-heading'); modeRow.append(el('h2', '', '代理模式'));
const modeSelect = el('select'); modeSelect.id = 'proxy-mode'; modeSelect.setAttribute('aria-label', '代理模式');
for (const [value, name] of [['rule', '规则'], ['global', '全局'], ['direct', '直连']]) { const option = el('option', '', name); option.value = value; modeSelect.append(option); }
gate(modeSelect, () => Boolean(status?.running && snapshot));
modeSelect.addEventListener('change', () => {
  const mode = modeSelect.value; modeSelect.value = snapshot?.configs.mode ?? 'rule';
  void mutate('切换代理模式', () => invoke('set_mode', { mode }));
});
modeRow.append(modeSelect);
const proxyGroups = el('div', 'proxy-groups');
const providerHeading = el('div', 'section-heading'); providerHeading.append(el('h2', '', '代理提供者'));
const providers = el('div', 'providers');
proxyPanel.append(modeRow, proxyGroups, providerHeading, providers);

const rulePanel = panels.get('rules')!;
const ruleSearch = searchField('筛选规则、内容或策略', 'rule-search', query => { ruleQuery = query; rulePage = 1; renderRules(); });
const ruleContent = el('div', 'data-content');
rulePanel.append(ruleSearch.wrap, ruleContent);
const connectionPanel = panels.get('connections')!;
const connectionSearch = searchField('筛选主机、进程或代理链', 'connection-search', query => { connectionQuery = query; connectionPage = 1; renderConnections(); });
const traffic = el('p', 'traffic-total', '等待连接数据');
const connectionContent = el('div', 'data-content');
connectionPanel.append(connectionSearch.wrap, traffic, connectionContent);
const logsPanel = panels.get('logs')!;
const logSearch = searchField('筛选日志', 'log-search', query => { logQuery = query; renderLogs(); });
const logHint = el('p', 'muted log-hint', `仅显示当前应用会话最近 ${LOG_LIMIT} 行内核输出，每 3 秒刷新。`);
const logContent = el('pre', 'log-output'); logContent.tabIndex = 0; logContent.setAttribute('aria-label', '内核日志');
logsPanel.append(logSearch.wrap, logHint, logContent);

const settings = panels.get('settings')!;
const settingsIntro = el('p', 'muted section-intro', '先选择本机 mihomo 内核并导入配置，再启动代理。更换文件或端口前需停止内核。');
const coreSetting = el('section', 'settings-section'); coreSetting.append(el('h2', '', '内核与配置'));
const corePath = el('p', 'path-value', '未选择内核');
const chooseCore = gate(button('选择 mihomo.exe', () => mutate('选择内核', () => invoke<Status>('choose_core'))), () => Boolean(status && !status.running));
const configPath = el('p', 'path-value', '未导入配置');
const importConfig = gate(button('导入 YAML 配置', () => {
  const operation = () => mutate('导入配置', () => invoke<Status>('import_config'));
  if (status?.configName) confirmAction('替换当前配置？', `导入后将替换 ${status.configName}。请保留原配置文件，以便需要时重新导入。`, '选择并替换配置', operation);
  else void operation();
}), () => Boolean(status && !status.running));
coreSetting.append(corePath, chooseCore, configPath, importConfig);
const subscriptionForm = el('form', 'settings-section'); subscriptionForm.noValidate = true;
subscriptionForm.append(el('h2', '', '订阅配置'), el('p', 'muted', '从 HTTPS 链接下载并替换本地配置。链接仅用于此次导入。'));
const subscription = field('订阅链接', 'subscription-url', 'password'); subscription.input.autocomplete = 'off'; subscription.input.spellcheck = false;
subscription.input.placeholder = 'https://…'; subscription.input.maxLength = 8192;
gate(subscription.input, () => Boolean(status && !status.running));
const showSubscription = button('显示链接', () => {
  const show = subscription.input.type === 'password'; subscription.input.type = show ? 'text' : 'password';
  showSubscription.textContent = show ? '隐藏链接' : '显示链接'; showSubscription.setAttribute('aria-pressed', String(show));
}, 'ghost'); showSubscription.setAttribute('aria-pressed', 'false');
const subscriptionSubmit = gate(button('导入订阅', () => {}), () => Boolean(status && !status.running)); subscriptionSubmit.type = 'submit';
const subscriptionActions = el('div', 'form-actions'); subscriptionActions.append(showSubscription, subscriptionSubmit);
subscriptionForm.append(subscription.wrapper, subscriptionActions);
subscription.input.addEventListener('input', () => setFieldError(subscription.input, subscription.error, null));
subscriptionForm.addEventListener('submit', event => {
  event.preventDefault();
  if (busy || subscriptionSubmit.disabled) return;
  const error = validateSubscription(subscription.input.value);
  setFieldError(subscription.input, subscription.error, error);
  if (error) { subscription.input.focus(); return; }
  const operation = () => mutate('导入订阅', () => invoke<Status>('import_subscription', { url: subscription.input.value.trim() })).then(success => {
    if (success) { subscription.input.value = ''; subscription.input.type = 'password'; showSubscription.textContent = '显示链接'; showSubscription.setAttribute('aria-pressed', 'false'); }
    return success;
  });
  if (status?.configName) confirmAction('替换当前配置？', `订阅下载成功后将替换 ${status.configName}。请保留原配置文件，以便需要时重新导入。`, '导入并替换配置', operation);
  else void operation();
});
const portForm = el('form', 'settings-section'); portForm.noValidate = true;
portForm.append(el('h2', '', '本地端口'), el('p', 'muted', '只监听本机回环地址。端口设置保存后在下次启动时生效。'));
const mixed = field('HTTP / SOCKS 代理端口', 'mixed-port', 'number');
const controller = field('控制端口', 'controller-port', 'number');
for (const input of [mixed.input, controller.input]) { input.min = '1024'; input.max = '65535'; input.step = '1'; gate(input, () => Boolean(status && !status.running)); }
const portFields = el('div', 'field-grid'); portFields.append(mixed.wrapper, controller.wrapper);
const savePorts = gate(button('保存端口', () => {}, 'primary'), () => Boolean(status && !status.running)); savePorts.type = 'submit';
portForm.append(portFields, savePorts);
let portsDirty = false;
for (const input of [mixed.input, controller.input]) input.addEventListener('input', () => {
  portsDirty = true; setFieldError(mixed.input, mixed.error, null); setFieldError(controller.input, controller.error, null);
});
portForm.addEventListener('submit', event => {
  event.preventDefault();
  if (busy || savePorts.disabled) return;
  const error = validatePorts(mixed.input.value, controller.input.value);
  setFieldError(mixed.input, mixed.error, error); setFieldError(controller.input, controller.error, error);
  if (error) { mixed.input.focus(); return; }
  void mutate('保存端口', () => invoke<Status>('save_settings', { mixedPort: Number(mixed.input.value), controllerPort: Number(controller.input.value) })).then(success => { if (success) portsDirty = false; });
});
const setupNote = el('p', 'settings-footnote', '请使用可信来源的 mihomo Windows 可执行文件。此版本不包含内核，不会自动下载或更新内核。');
settings.append(settingsIntro, coreSetting, subscriptionForm, portForm, setupNote);

function gate<T extends HTMLButtonElement | HTMLInputElement | HTMLSelectElement>(control: T, enabled: () => boolean): T {
  gated.set(control, enabled); return control;
}

function updateControls() {
  for (const [control, enabled] of gated) {
    if (!control.isConnected) { gated.delete(control); continue; }
    control.disabled = !desktop || busy || !enabled();
  }
  refreshButton.disabled = !desktop || busy || refreshing;
  retry.disabled = !desktop || busy || refreshing;
  runButton.setAttribute('aria-busy', String(busy));
  runButton.textContent = status?.running ? '停止内核' : '启动内核';
  proxyToggle.checked = status?.systemProxy ?? false;
  coreBadge.textContent = !desktop ? '仅桌面可用' : !status ? '未连接' : status.running ? '运行中' : '已停止';
  coreBadge.dataset.state = status?.running ? 'running' : 'stopped';
  configLabel.textContent = status?.configName || (status ? '尚未导入配置' : '等待桌面服务');
  versionLabel.textContent = status?.running ? `mihomo ${status.version || ''} · 127.0.0.1:${status.mixedPort}` : '本地代理客户端';
  corePath.textContent = status?.corePath || '未选择内核'; configPath.textContent = status?.configName || '未导入配置';
  if (status && !portsDirty) { mixed.input.value = String(status.mixedPort); controller.input.value = String(status.controllerPort); }
  const message = actionError || refreshError || (status?.lastError ? safeError(status.lastError) : '');
  errors.hidden = !message; errorCopy.textContent = message;
  notice.textContent = !desktop ? '此页面需要 ClashBar 桌面应用。请在桌面窗口中打开，以管理本机内核和系统代理。' : busy ? `${pendingLabel}…` : !status ? '正在读取桌面状态…' : !status.corePath || !status.configName ? '完成“设置”中的内核与配置后，即可启动。' : !status.running ? '内核已停止。启动后可查看节点、规则和连接。' : status.systemProxy ? '系统代理已开启。' : '内核运行中；需要接管系统流量时，请开启系统代理。';
  freshness.textContent = refreshing ? '正在刷新…' : lastUpdated ? `上次读取 ${new Intl.DateTimeFormat('zh-CN', { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false }).format(lastUpdated)}${refreshError ? ' · 数据可能已过期' : ''}` : '尚未读取内核';
  content.setAttribute('aria-busy', String(busy));
}

function activateTab(tab: Tab) {
  activeTab = tab;
  for (const [key, node] of panels) node.hidden = key !== tab;
  for (const [key, node] of tabButtons) { node.setAttribute('aria-selected', String(key === tab)); node.tabIndex = key === tab ? 0 : -1; }
  document.title = `ClashBar · ${tabs.find(([key]) => key === tab)![1]}`;
  // Drafts remain mounted across tabs; secrets are remasked when leaving settings.
  if (tab !== 'settings') { subscription.input.type = 'password'; showSubscription.textContent = '显示链接'; showSubscription.setAttribute('aria-pressed', 'false'); }
  renderActive();
  if (desktop && !busy) void refresh();
}

function renderActive() {
  if (activeTab === 'proxies') renderProxies();
  if (activeTab === 'rules') renderRules();
  if (activeTab === 'connections') renderConnections();
  if (activeTab === 'logs') renderLogs();
  updateControls();
}

function notRunning() { return emptyState(status?.running ? '正在读取数据' : '内核未启动', status?.running ? '若读取失败，请使用下方的“刷新”重试。' : '在“设置”中导入配置，然后启动内核。'); }

function renderProxies() {
  modeSelect.value = snapshot?.configs.mode?.toLowerCase() ?? 'rule';
  if (document.activeElement instanceof HTMLSelectElement && proxyPanel.contains(document.activeElement) && status?.running && snapshot) {
    // Preserve the OS popup and focus while committing authoritative selections.
    for (const select of proxyGroups.querySelectorAll<HTMLSelectElement>('select[data-group]')) {
      select.value = snapshot.proxies.proxies[select.dataset.group!]?.now ?? '';
    }
    return;
  }
  if (!snapshot || !status?.running) { proxyGroups.replaceChildren(notRunning()); providers.replaceChildren(el('p', 'muted', '启动内核后读取代理提供者。')); return; }
  const groups = Object.entries(snapshot.proxies.proxies).filter(([, proxy]) => Array.isArray(proxy.all));
  const nodes: HTMLElement[] = groups.map(([name, proxy], index) => {
    const group = el('section', 'proxy-group');
    const heading = el('div', 'group-heading');
    const label = el('label', 'group-name', name); label.htmlFor = `group-${index}`;
    heading.append(label, el('span', 'subtle-tag', proxy.type));
    const row = el('div', 'group-control');
    const select = el('select'); select.id = `group-${index}`;
    select.dataset.group = name;
    for (const candidate of proxy.all ?? []) { const option = el('option', '', candidate); option.value = candidate; select.append(option); }
    select.value = proxy.now ?? '';
    gate(select, () => Boolean(status?.running && proxy.type === 'Selector'));
    select.addEventListener('change', () => {
      const chosen = select.value; select.value = proxy.now ?? '';
      void mutate('切换节点', () => invoke('select_proxy', { group: name, name: chosen }));
    });
    const delayValue = delays.get(proxy.now ?? name) ?? snapshot?.proxies.proxies[proxy.now ?? name]?.history?.at(-1)?.delay;
    const delay = el('span', 'latency', delayValue == null ? '未测速' : delayValue === 0 ? '超时' : `${delayValue} ms`);
    const test = gate(button('测速', () => mutate('测试延迟', async () => {
      const node = proxy.now ?? name; const result = await invoke<{ delay: number }>('test_delay', { name: node }); delays.set(node, result.delay);
    }), 'ghost'), () => Boolean(status?.running)); test.id = `delay-${index}`; test.setAttribute('aria-label', `测试 ${name} 的当前节点延迟`);
    row.append(select, delay, test); group.append(heading, row);
    if (proxy.type !== 'Selector') group.append(el('p', 'group-hint', '此分组由内核自动选择节点。'));
    return group;
  });
  replacePreservingFocus(proxyGroups, ...(nodes.length ? nodes : [emptyState('没有代理分组', '当前配置未提供可切换的代理组。')]));
  const providerNodes = Object.entries(snapshot.providers.providers).map(([name, provider], index) => {
    const row = el('div', 'provider-row');
    const copy = el('div', 'provider-copy'); copy.append(el('strong', '', name), el('span', 'muted', `${provider.proxies?.length ?? 0} 个节点 · ${provider.vehicleType || provider.type || '提供者'}`));
    const update = gate(button('更新', () => mutate('更新代理提供者', () => invoke('update_provider', { name })), 'ghost'), () => Boolean(status?.running)); update.id = `provider-${index}`; update.setAttribute('aria-label', `更新代理提供者 ${name}`);
    row.append(copy, update); return row;
  });
  replacePreservingFocus(providers, ...(providerNodes.length ? providerNodes : [el('p', 'muted', '当前配置没有代理提供者。')]));
}

function paginator(page: number, pages: number, start: number, end: number, total: number, label: string, onPage: (page: number) => void) {
  const row = el('nav', 'pagination'); row.setAttribute('aria-label', `${label}分页`);
  const count = el('span', 'muted', `${start}–${end} / ${total} 条`);
  const previous = button('上一页', () => onPage(page - 1), 'ghost'); previous.disabled = page <= 1;
  const next = button('下一页', () => onPage(page + 1), 'ghost'); next.disabled = page >= pages;
  previous.id = `${label}-previous`; next.id = `${label}-next`;
  row.append(count, previous, el('span', 'page-number', `${page} / ${pages}`), next); return row;
}

function renderRules() {
  if (!snapshot || !status?.running) { ruleContent.replaceChildren(notRunning()); return; }
  const all = snapshot.rules.rules ?? [];
  const filtered = all.filter(rule => matchesQuery([rule.type, rule.payload, rule.proxy], ruleQuery));
  const page = pageSlice(filtered, rulePage); rulePage = page.page;
  const rows = page.items.map(rule => [el('span', 'subtle-tag', rule.type), el('span', 'technical break-anywhere', rule.payload || '—'), el('span', 'break-anywhere', rule.proxy)]);
  const body = rows.length ? table(['类型', '匹配内容', '策略'], rows, '路由规则') : emptyState(all.length ? '没有匹配规则' : '没有规则', all.length ? '请修改或清除筛选条件。' : '当前配置未定义路由规则。');
  replacePreservingFocus(ruleContent, body, paginator(page.page, page.pages, page.start, page.end, page.total, '规则', value => { rulePage = value; renderRules(); }));
}

function connectionName(connection: Connection) { return connection.metadata.host || connection.metadata.destinationIP || connection.id; }

function renderConnections() {
  if (!snapshot || !status?.running) { traffic.textContent = '等待连接数据'; connectionContent.replaceChildren(notRunning()); return; }
  traffic.textContent = `累计上传 ${formatBytes(snapshot.connections.uploadTotal)}　累计下载 ${formatBytes(snapshot.connections.downloadTotal)}`;
  const all = snapshot.connections.connections ?? [];
  const filtered = all.filter(connection => matchesQuery([connectionName(connection), connection.metadata.process, connection.metadata.processPath, connection.metadata.network, ...(connection.chains ?? [])], connectionQuery));
  const page = pageSlice(filtered, connectionPage); connectionPage = page.page;
  const rows = page.items.map(connection => {
    const target = el('div', 'connection-target');
    target.append(el('strong', 'break-anywhere', connectionName(connection)), el('span', 'muted', `${connection.metadata.network?.toUpperCase() ?? ''} · ${connection.metadata.destinationPort ?? '—'}`));
    const route = el('div', 'connection-target'); route.append(el('span', 'break-anywhere', connection.chains?.join(' → ') || '—'), el('span', 'muted break-anywhere', connection.metadata.process || connection.rule || '—'));
    const amount = el('span', 'technical', `↑ ${formatBytes(connection.upload)}\n↓ ${formatBytes(connection.download)}`);
    const close = gate(button('关闭', () => confirmAction('关闭此连接？', `将中断 ${connectionName(connection)} 的当前连接。应用可能自动重新连接。`, '关闭连接', () => mutate('关闭连接', () => invoke('close_connection', { id: connection.id }))), 'ghost danger-text'), () => Boolean(status?.running)); close.id = `close-${connection.id}`; close.setAttribute('aria-label', `关闭连接 ${connectionName(connection)}`);
    return [target, route, amount, close];
  });
  const body = rows.length ? table(['目标', '代理链 / 进程', '流量', '操作'], rows, '当前连接') : emptyState(all.length ? '没有匹配连接' : '暂无连接', all.length ? '请修改或清除筛选条件。' : '通过代理访问网络后，连接会显示在这里。');
  replacePreservingFocus(connectionContent, body, paginator(page.page, page.pages, page.start, page.end, page.total, '连接', value => { connectionPage = value; renderConnections(); updateControls(); }));
}

function renderLogs() {
  const filtered = logs.slice(-LOG_LIMIT).filter(line => matchesQuery([line], logQuery));
  const atBottom = logContent.scrollHeight - logContent.scrollTop - logContent.clientHeight < 24;
  logContent.textContent = filtered.length ? filtered.join('\n') : logQuery ? '没有匹配日志。请修改或清除筛选条件。' : '暂无内核日志。启动内核后可在这里查看输出。';
  if (atBottom) logContent.scrollTop = logContent.scrollHeight;
}

async function refresh() {
  if (!desktop || busy || refreshing || document.hidden) return;
  const ticket = epoch.next(); refreshing = true; updateControls();
  try {
    const nextStatus = await invoke<Status>('get_status');
    if (!epoch.current(ticket)) return;
    status = nextStatus;
    if (!status.running) snapshot = null;
    if (status.running) {
      const nextSnapshot = await invoke<Snapshot>('get_snapshot');
      if (!epoch.current(ticket)) return;
      snapshot = nextSnapshot;
    }
    if (activeTab === 'logs') {
      const nextLogs = await invoke<string[]>('get_logs');
      if (!epoch.current(ticket)) return;
      logs = nextLogs.slice(-LOG_LIMIT);
    }
    refreshError = ''; lastUpdated = new Date();
  } catch (error) {
    if (epoch.current(ticket)) refreshError = `读取失败，已保留上次数据。${safeError(error)}`;
  } finally {
    if (epoch.current(ticket)) { refreshing = false; renderActive(); }
  }
}

async function mutate(label: string, operation: () => Promise<unknown>): Promise<boolean> {
  if (!desktop || busy) return false;
  epoch.next(); refreshing = false; busy = true; pendingLabel = label; actionError = ''; updateControls();
  let success = false;
  try {
    const result = await operation();
    if (result && typeof result === 'object' && 'running' in result) status = result as Status;
    success = true;
  } catch (error) { actionError = `${label}未完成。${safeError(error)}`; }
  finally { busy = false; pendingLabel = ''; renderActive(); }
  await refresh();
  if (success) notice.textContent = `${label}已完成。`;
  return success;
}

window.addEventListener('beforeunload', event => {
  if (portsDirty || subscription.input.value) { event.preventDefault(); event.returnValue = ''; }
});
document.addEventListener('visibilitychange', () => { if (!document.hidden) void refresh(); });
// A focused native popup owns its keyboard interaction. Refresh it after focus leaves.
proxyPanel.addEventListener('focusout', () => { setTimeout(() => { if (!proxyPanel.contains(document.activeElement)) { renderProxies(); updateControls(); } }, 0); });
activateTab('proxies');
setInterval(() => { void refresh(); }, 3000);
