import { expect, test, type Page } from '@playwright/test';

// 测试数据仅存在于隔离桥接层，生产界面只读取内核实测数据。
async function installBridge(page: Page, running = true, submenu = false) {
  await page.addInitScript(({ running, submenu }) => {
    const callbacks = new Map<number, (event: unknown) => void>();
    const listeners = new Map<string, number[]>(); let nextCallback = 0;
    const groups = ['广告拦截', '苹果服务', '手动切换', '自动选择', '节点选择', '瓦工节点', '美国节点', '美国 AN', '日本节点', '狮城节点', '韩国节点', '欧洲节点'];
    const proxies: Record<string, any> = Object.fromEntries(groups.map((name, index) => [name, { type: 'Selector', now: index === 0 ? 'REJECT' : index === 1 ? 'DIRECT' : '香港 01', all: ['香港 01', '东京 02', '新加坡 03'], history: [{ delay: 92 + index * 13 }] }]));
    Object.assign(proxies, { '香港 01': { type: 'Shadowsocks', history: [{ delay: 156 }] }, '东京 02': { type: 'Trojan', history: [{ delay: 84 }] }, '新加坡 03': { type: 'VLESS', history: [{ delay: 64 }] } });
    const state: any = {
      status: { running, localRunning: running, localMixedPort: 7890, activeRemoteId: null, remoteMachines: [], targetRevision: 0, corePath: 'C:\\mihomo.exe', configName: 'clashmini.yaml', profiles: [{ id: 'one', name: 'clashmini.yaml' }, { id: 'two', name: '备用配置.yaml' }], activeProfileId: 'one', mixedPort: 7890, controllerPort: 9090, systemProxy: false, version: '1.19.31', lastError: null },
      snapshot: {
        proxies: { proxies }, configs: { mode: 'rule', 'log-level': 'info', 'allow-lan': false, ipv6: true, 'tcp-concurrent': false, port: 0, 'socks-port': 0, 'mixed-port': 7890, 'redir-port': 0, 'tproxy-port': 0 }, memory: 107 * 1024 * 1024,
        ruleProviders: { providers: {} },
        rules: { rules: Array.from({ length: 112 }, (_, index) => ({ type: 'DOMAIN-SUFFIX', payload: `host-${index}.example`, proxy: index % 2 ? '手动切换' : '自动选择' })) },
        connections: { connections: Array.from({ length: 61 }, (_, index) => ({ id: `connection-${index}`, metadata: { host: `host-${index}.example`, destinationPort: '443', network: index % 2 ? 'udp' : 'tcp', process: 'browser.exe' }, rule: 'DOMAIN', start: '2026-10-08T12:00:00Z', chains: ['香港 01', '手动切换'], upload: 1024, download: 2048 })), uploadTotal: 641800, downloadTotal: 170100000 },
        providers: { providers: { ccsub: { vehicleType: 'HTTP', updatedAt: new Date().toISOString(), proxies: Array(119).fill({ type: 'VLESS' }), subscriptionInfo: { upload: 0, download: 28800000000000, total: 34100000000000, expire: 1810000000 } }, jmsub: { vehicleType: 'HTTP', updatedAt: new Date().toISOString(), proxies: Array(6).fill({ type: 'Shadowsocks' }) } } },
      },
      logs: Array.from({ length: 600 }, (_, i) => `12:01:00 info [TCP] log line ${i}`), appEntries: [],
      commands: [], failClose: false, deferSnapshot: false, resolveSnapshot: null, pinned: false,
      menu: submenu ? { id: 'sample-menu', title: '手动切换  3', items: [{ id: 'hongkong', label: '香港 01', detail: 'Shadowsocks', value: '156', checked: true, secondaryId: 'hongkong-test', secondaryLabel: '测试 香港 01 延迟' }, { id: 'tokyo', label: '东京 02', detail: 'Trojan', value: '84', checked: false }, { id: 'singapore', label: '新加坡 03', detail: 'VLESS', value: '64', checked: false }] } : null,
    };
    const emit = (event: string, payload: unknown) => { for (const id of listeners.get(event) || []) callbacks.get(id)?.({ event, payload, id }); };
    state.emit = emit;
    state.remoteSnapshots = {}; state.remoteSecrets = {};
    state.status.autoStartCore = false; state.status.launchAtLogin = false;
    state.status.localProxyPorts = { port: 0, 'socks-port': 0, 'mixed-port': 7890, 'redir-port': 0, 'tproxy-port': 0 };
    state.status.localLanAddress = '192.168.10.8';
    state.status.subscriptions = []; state.status.ssidEnabled = false; state.status.ssidRules = [];
    state.status.ssidSnapshot = { currentSsid: 'Home Wi-Fi', status: 'available', error: null };
    state.status.systemProxyExceptions = ['localhost', '127.*']; state.status.tunEnabled = false; state.status.tunStack = 'mixed';
    const selectTarget = (id: string | null) => {
      if (!state.status.activeRemoteId) { state.localSnapshot = state.snapshot; state.localLogs = state.logs; }
      if (id) {
        if (state.failProbe) throw new Error('连接失败，请检查地址和密钥');
        const machine = state.status.remoteMachines.find((machine: any) => machine.id === id);
        if (!state.remoteSnapshots[id]) { state.remoteSnapshots[id] = structuredClone(state.localSnapshot); state.remoteSnapshots[id].configs.mode = 'direct'; state.remoteSnapshots[id].configs['mixed-port'] = 8890; state.remoteSnapshots[id].rules.rules = [{ type: 'DOMAIN', payload: 'remote.example', proxy: 'DIRECT' }]; }
        state.snapshot = state.remoteSnapshots[id]; state.logs = ['info 远程内核日志'];
        Object.assign(state.status, { activeRemoteId: id, running: true, version: 'remote-fixture', controllerAddress: machine.address, targetName: machine.name });
      } else {
        state.snapshot = state.localSnapshot ?? state.snapshot; state.logs = state.localLogs ?? state.logs;
        Object.assign(state.status, { activeRemoteId: null, running: state.status.localRunning, version: '1.19.31', controllerAddress: `127.0.0.1:${state.status.controllerPort}`, targetName: '本机' });
      }
      state.status.targetRevision++;
    };
    const call = async (command: string, args: any = {}) => {
      state.commands.push({ command, args });
      if (command === 'plugin:event|listen') { listeners.set(args.event, [...(listeners.get(args.event) || []), args.handler]); return args.handler; }
      if (command === 'plugin:event|unlisten') return;
      if (command === 'get_status') return structuredClone(state.status);
      if (command === 'set_launch_at_login') { if (state.failLaunch) throw new Error('无法更改开机自启'); state.status.launchAtLogin = args.enabled; return structuredClone(state.status); }
      if (command === 'set_core_autostart') { state.status.autoStartCore = args.enabled; return structuredClone(state.status); }
      if (command === 'check_remote_machine') {
        if (state.deferProbe) await new Promise(resolve => { state.resolveProbe = resolve; });
        return { id: args.id, connected: !state.failProbe, version: state.failProbe ? null : 'remote-fixture', error: state.failProbe ? '连接失败' : null };
      }
      if (command === 'save_remote_machine') {
        if (state.failSaveRemote) throw new Error('保存机器失败');
        const input = args.input, id = input.id || `remote-${state.status.remoteMachines.length + 1}`;
        if (input.secret !== null) state.remoteSecrets[id] = input.secret;
        const machine = { id, name: input.name, host: input.host, port: input.port, useHttps: input.useHttps, hasSecret: Boolean(state.remoteSecrets[id]), address: `${input.useHttps ? 'https' : 'http'}://${input.host}:${input.port}` };
        state.status.remoteMachines = [...state.status.remoteMachines.filter((existing: any) => existing.id !== id), machine];
        if (state.status.activeRemoteId === id) selectTarget(id);
        return structuredClone(state.status);
      }
      if (command === 'select_machine') { selectTarget(args.id); return structuredClone(state.status); }
      if (command === 'delete_remote_machine') {
        if (state.failDeleteRemote) throw new Error('删除机器失败');
        state.status.remoteMachines = state.status.remoteMachines.filter((machine: any) => machine.id !== args.id); delete state.remoteSecrets[args.id];
        if (state.status.activeRemoteId === args.id) selectTarget(null);
        return structuredClone(state.status);
      }
      if (command === 'get_snapshot') { const captured = structuredClone(state.snapshot); if (state.deferSnapshot) { state.deferSnapshot = false; await new Promise(resolve => { state.resolveSnapshot = resolve; }); } return captured; }
      if (command === 'get_logs') return state.logs;
      if (command === 'get_log_entries') return [...state.logs.map((message: string, index: number) => ({ timestamp: Date.parse('2026-10-08T12:01:00Z') + index, source: 'Mihomo', message })), ...state.appEntries];
      if (command === 'record_app_action') { const entry = { timestamp: Date.now(), source: 'ClashBar', message: args.message }; state.appEntries.push(entry); return entry; }
      if (command === 'get_proxy_group_icon') { if (state.failIcon) throw new Error('图标下载失败'); return state.iconData; }
      if (command === 'get_popup_pinned') return state.pinned;
      if (command === 'set_popup_pinned') { state.pinned = args.pinned; emit('popup-visibility', { visible: true, pinned: state.pinned }); return; }
      if (command === 'resize_popup') return { width: 360, height: Math.min(900, args.height), maxHeight: 900 };
      if (command === 'show_attached_menu') { state.menu = structuredClone(args.menu); return { side: 'left', width: args.width, height: args.height }; }
      if (command === 'hide_attached_menu') { if (state.menu) { const menuId = state.menu.id; state.menu = null; emit('attached-menu-closed', { menuId }); } return; }
      if (command === 'get_attached_menu') return state.menu;
      if (command === 'attached_menu_action') {
        const isSecondary = state.menu.items.some((item: any) => item.secondaryId === args.actionId);
        emit('attached-menu-action', args);
        if (!isSecondary) { state.menu = null; emit('attached-menu-closed', { menuId: args.menuId }); }
        return;
      }
      if (command === 'set_mode') state.snapshot.configs.mode = args.mode;
      if (command === 'select_proxy') state.snapshot.proxies.proxies[args.group].now = args.name;
      if (command === 'test_delay') return { delay: 42 };
      if (command === 'test_group_delay') return { '香港 01': 42, '东京 02': 84, '新加坡 03': 64 };
      if (command === 'close_connection') { await new Promise(resolve => setTimeout(resolve, 150)); if (state.failClose) throw new Error('Test close failure'); state.snapshot.connections.connections = state.snapshot.connections.connections.filter((c: any) => c.id !== args.id); }
      if (command === 'close_all_connections') state.snapshot.connections.connections = [];
      if (command === 'start_core' || command === 'restart_core') state.status.running = state.status.localRunning = true;
      if (command === 'stop_core') { state.status.running = state.status.localRunning = false; state.status.systemProxy = false; }
      if (command === 'set_system_proxy') state.status.systemProxy = args.enabled;
      if (command === 'save_settings') Object.assign(state.status, args);
      if (command === 'save_local_ports') { state.status.localProxyPorts = args.ports; state.status.mixedPort = args.ports['mixed-port']; state.status.controllerPort = args.controllerPort; }
      if (command === 'set_tun') { if (state.failTun) throw new Error('TUN 授权失败'); state.status.tunEnabled = args.enabled; state.status.tunStack = args.stack; }
      if (command === 'save_proxy_exceptions') { if (state.failExceptions) throw new Error('保存绕过失败'); state.status.systemProxyExceptions = args.exceptions; }
      if (command === 'set_status_bar_style') state.status.statusBarStyle = args.style;
      if (command === 'set_ssid_enabled') state.status.ssidEnabled = args.enabled;
      if (command === 'bind_ssid') { const ssid = state.status.ssidSnapshot.currentSsid; const name = state.status.profiles.find((item: any) => item.id === args.profileId).name; const existed = state.status.ssidRules.some((item: any) => item.ssid === ssid && item.configFileName === name); state.status.ssidRules = state.status.ssidRules.filter((item: any) => item.ssid !== ssid); if (!existed) state.status.ssidRules.push({ ssid, configFileName: name }); }
      if (command === 'remove_ssid_binding') state.status.ssidRules = state.status.ssidRules.filter((item: any) => item.ssid !== args.ssid);
      if (command === 'prepare_subscription') { const raw = args.input.name || 'imported.yaml'; const name = /\.ya?ml$/i.test(raw) ? raw : `${raw}.yaml`; return { name, overwriteRequired: state.status.profiles.some((item: any) => item.name === name) }; }
      if (command === 'add_subscription') { if (state.failSubscription) throw new Error('订阅下载失败 https://example.com/sub?token=hidden'); const name = args.input.name || 'imported.yaml'; const existing = state.status.profiles.find((item: any) => item.name === name); if (existing && !args.input.overwrite) throw new Error('配置已存在，请确认覆盖'); const id = existing?.id || 'sub'; if (!existing) state.status.profiles.push({ id, name }); if (!state.status.activeProfileId) { state.status.activeProfileId = id; state.status.configName = name; } state.status.subscriptions = state.status.subscriptions.filter((item: any) => item.profileId !== id); state.status.subscriptions.push({ profileId: id, sourceHost: 'example.com', autoUpdateEnabled: args.input.autoUpdateEnabled, autoUpdateIntervalHours: args.input.autoUpdateIntervalHours, lastUpdatedAt: Date.now() - 120000, lastCheckedAt: Date.now(), nextUpdateAt: Date.now() + 21600000, lastError: null }); }
      if (command === 'prepare_config_import') { state.pendingImport = state.importName || 'imported.yaml'; return { name: state.pendingImport, overwriteRequired: state.status.profiles.some((item: any) => item.name === state.pendingImport) }; }
      if (command === 'cancel_config_import') { state.pendingImport = null; return; }
      if (command === 'finish_config_import') { if (state.failImport) throw new Error('配置导入失败'); const existing = state.status.profiles.find((item: any) => item.name === state.pendingImport); if (existing && !args.overwrite) throw new Error('配置已存在，请确认覆盖'); if (!existing) state.status.profiles.push({ id: 'local-new', name: state.pendingImport }); if (!state.status.activeProfileId) { state.status.activeProfileId = existing?.id || 'local-new'; state.status.configName = state.pendingImport; } state.pendingImport = null; }
      if (command === 'save_subscription') { Object.assign(state.status.subscriptions.find((item: any) => item.profileId === args.id), { autoUpdateEnabled: args.autoUpdateEnabled, autoUpdateIntervalHours: args.autoUpdateIntervalHours }); }
      if (command === 'check_app_update') return { currentVersion: '0.1.0', displayVersion: '0.2.0', tagName: 'v0.2.0', releaseUrl: 'https://github.com/Cyli00/ClashBar/releases/tag/v0.2.0', updateAvailable: true };
      if (command === 'save_remote_ports') {
        if (state.failRemotePorts) throw new Error('远程端口保存失败，已恢复原内核端口');
        Object.assign(state.snapshot.configs, args.ports); return;
      }
      if (command === 'select_profile') { state.status.activeProfileId = args.id; state.status.configName = state.status.profiles.find((profile: any) => profile.id === args.id).name; }
      if (command === 'delete_profile') {
        if (state.failDelete) throw new Error('配置回收失败，已恢复原配置');
        state.status.profiles = state.status.profiles.filter((profile: any) => profile.id !== args.id);
        if (state.status.activeProfileId === args.id) {
          const next = state.status.profiles[0]; state.status.activeProfileId = next?.id ?? null; state.status.configName = next?.name ?? null;
          if (!next) state.status.running = state.status.systemProxy = false;
        }
      }
      if (command === 'import_subscription' || command === 'import_config') state.status.configName = 'imported.yaml';
      if (command === 'clear_logs') { state.logs = []; state.appEntries = []; }
      if (command === 'set_log_level') state.snapshot.configs['log-level'] = args.level;
      if (command === 'set_core_boolean') {
        if (state.failCoreSetting) throw new Error('保存设置失败，已恢复原值');
        state.snapshot.configs[args.setting] = args.value;
        return;
      }
      if (['refresh_rule_providers', 'flush_fakeip_cache', 'flush_dns_cache', 'upgrade_geo'].includes(command)) {
        if (state.deferMaintenance) await new Promise(resolve => { state.resolveMaintenance = resolve; });
        if (state.failMaintenance) throw new Error(state.failMaintenance);
        return;
      }
      return structuredClone(state.status);
    };
    state.choose = async (label: string, secondary = false) => { const item = state.menu.items.find((row: any) => row.label === label); if (!item) throw new Error(`Menu item missing: ${label}`); await call('attached_menu_action', { menuId: state.menu.id, actionId: secondary ? item.secondaryId : item.id }); };
    Object.assign(window, { isTauri: true, testState: state, __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => {} }, __TAURI_INTERNALS__: { invoke: call, transformCallback: (callback: (event: unknown) => void) => { const id = ++nextCallback; callbacks.set(id, callback); return id; }, unregisterCallback: (id: number) => callbacks.delete(id) } });
  }, { running, submenu });
}
async function chooseMenu(page: Page, label: string, secondary = false) {
  await expect.poll(() => page.evaluate(label => (window as any).testState.menu?.items.some((item: any) => item.label === label), label)).toBe(true);
  await page.evaluate(({ label, secondary }) => (window as any).testState.choose(label, secondary), { label, secondary });
}

test.afterEach(async ({ page }, testInfo) => { await page.screenshot({ path: testInfo.outputPath('screen.png'), fullPage: true }); });

test('browser-only popup is honest, compact and cannot mutate desktop state', async ({ page }) => {
  await page.goto('/'); await expect(page.getByText('此页面需要 ClashBar 桌面应用。浏览器无法管理本机内核和系统代理。')).toBeVisible();
  await expect(page.getByRole('button', { name: '启动内核', exact: true })).toBeDisabled();
  expect(await page.locator('.menu-panel').evaluate(node => node.getBoundingClientRect().width)).toBe(360);
});

test('first native show refreshes a hidden startup without a document visibility event', async ({ page }) => {
  await installBridge(page);
  // Model a WebView whose native show notification arrives before hidden updates.
  await page.addInitScript(() => Object.defineProperty(document, 'hidden', { configurable: true, get: () => true }));
  await page.goto('/');
  await expect.poll(() => page.evaluate(() => (window as any).testState.commands.some((call: any) => call.command === 'plugin:event|listen' && call.args.event === 'popup-visibility'))).toBe(true);
  expect(await page.evaluate(() => (window as any).testState.commands.filter((call: any) => call.command === 'get_status').length)).toBe(0);
  await page.evaluate(() => (window as any).testState.emit('popup-visibility', { visible: true, pinned: false }));
  await expect(page.getByRole('button', { name: '代理组 手动切换', exact: true })).toBeVisible();
  expect(await page.evaluate(() => (window as any).testState.commands.filter((call: any) => call.command === 'get_snapshot').length)).toBe(1);
});

test('source-style populated popup renders at 360px in light and dark', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 360, height: 900 }); await installBridge(page); await page.goto('/');
  await expect(page.getByRole('button', { name: '代理组 手动切换', exact: true })).toBeVisible();
  await expect(page.getByRole('tab')).toHaveText(['节点', '分流', '连接', '日志', '设置']);
  await expect(page.getByText('MIHOMO v1.19.31')).toBeVisible();
  await expect.poll(() => page.locator('.brand-logo').evaluate((image: HTMLImageElement) => image.naturalWidth)).toBe(512);
  await page.locator('.menu-panel').screenshot({ path: testInfo.outputPath('proxy-light.png') });
  await page.emulateMedia({ colorScheme: 'dark' }); await page.locator('.menu-panel').screenshot({ path: testInfo.outputPath('proxy-dark.png') });
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test('settings retain drafts and native profile/subscription menus activate real commands', async ({ page }) => {
  await installBridge(page, false); await page.goto('/'); await page.getByRole('tab', { name: '设置' }).click();
  await expect(page.getByLabel('混合端口', { exact: true })).toHaveValue('7890');
  await page.getByLabel('混合端口', { exact: true }).fill('9090'); await page.getByLabel('混合端口', { exact: true }).press('Enter');
  await expect(page.getByLabel('混合端口', { exact: true })).toHaveAttribute('aria-invalid', 'true');
  await page.getByLabel('混合端口', { exact: true }).fill('7891'); await page.getByRole('tab', { name: '分流' }).click(); await page.getByRole('tab', { name: '设置' }).click();
  await expect(page.getByLabel('混合端口', { exact: true })).toHaveValue('7891');
  await expect.poll(() => page.evaluate(() => (window as any).testState.status.mixedPort)).toBe(7891);
  await page.getByRole('tab', { name: '节点' }).click(); await page.getByRole('button', { name: /切换配置/ }).click(); await chooseMenu(page, '备用配置.yaml');
  await expect(page.getByRole('button', { name: /切换配置/ })).toContainText('备用配置.yaml');
  await page.getByRole('button', { name: /切换配置/ }).click(); await chooseMenu(page, '导入订阅链接...');
  await page.getByLabel('订阅链接', { exact: true }).fill('http://example.com'); await page.getByRole('button', { name: '导入订阅', exact: true }).click();
  await expect(page.getByLabel('订阅链接', { exact: true })).toHaveAttribute('aria-invalid', 'true');
  await page.getByLabel('订阅链接', { exact: true }).fill('https://example.com/sub?token=private'); await page.getByRole('button', { name: '导入订阅', exact: true }).click();
  await expect(page.getByRole('dialog')).toHaveCount(0); await page.getByRole('button', { name: '启动内核', exact: true }).click(); await expect(page.getByRole('button', { name: '停止内核', exact: true })).toBeEnabled();
});

test('native group menu dispatch preserves callback ordering, secondary tests and keyboard intent', async ({ page }) => {
  await installBridge(page); await page.goto('/');
  await page.getByRole('button', { name: '代理组 手动切换', exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).testState.commands.findLast((call: any) => call.command === 'show_attached_menu')?.args.focus)).toBe(true);
  await chooseMenu(page, '东京 02', true);
  await expect.poll(() => page.evaluate(() => (window as any).testState.menu?.items.find((row: any) => row.label === '东京 02')?.value)).toBe('42');
  await chooseMenu(page, '东京 02');
  await expect.poll(() => page.evaluate(() => (window as any).testState.snapshot.proxies.proxies['手动切换'].now)).toBe('东京 02');
  await page.getByRole('button', { name: '全局模式', exact: true }).click(); await expect(page.getByRole('button', { name: '全局模式', exact: true })).toHaveAttribute('aria-pressed', 'true');
  await page.getByRole('button', { name: '测试 手动切换 分组延迟', exact: true }).click();
  await page.getByRole('tab', { name: '节点' }).focus(); await page.keyboard.press('ArrowRight'); await expect(page.getByRole('tab', { name: '分流' })).toBeFocused();
});

test('rules virtualize all data, filter IME safely and keep data tabs within existing popup height', async ({ page }) => {
  await installBridge(page); await page.goto('/'); await page.getByRole('tab', { name: '分流' }).click();
  await expect(page.locator('.virtual-rules')).toHaveAttribute('aria-rowcount', '112'); await expect(page.locator('.rule-row')).toHaveCount(50);
  const search = page.getByRole('searchbox', { name: '搜索目标、类型、策略...' });
  await search.dispatchEvent('compositionstart'); await search.fill('host-111'); await expect(page.locator('.rule-row')).toHaveCount(50); await search.dispatchEvent('compositionend');
  await expect(page.locator('.rule-row')).toHaveCount(1); await page.getByRole('button', { name: '清除搜索目标、类型、策略...' }).click(); await expect(search).toBeFocused();
  await expect(page.locator('.rule-row')).toHaveCount(50);
  await page.getByRole('button', { name: '按策略分组', exact: true }).click(); await expect(page.getByRole('button', { name: /手动切换 56/ })).toBeVisible();
  const heights = await page.evaluate(() => (window as any).testState.commands.filter((call: any) => call.command === 'resize_popup').map((call: any) => call.args.height));
  expect(heights.every((height: number) => height <= 900)).toBe(true);
});

test('connections close directly like the source, serialize retries and expose compact filters', async ({ page }) => {
  await installBridge(page); await page.goto('/'); await page.getByRole('tab', { name: '连接' }).click();
  await page.evaluate(() => { (window as any).testState.failClose = true; });
  const close = page.getByRole('button', { name: '关闭连接 host-0.example', exact: true }); await close.focus(); await close.click();
  await expect(close).toBeDisabled(); await expect(page.getByRole('dialog')).toHaveCount(0); await expect(page.getByRole('alert')).toContainText('关闭连接未完成');
  await page.evaluate(() => { (window as any).testState.failClose = false; }); await page.getByRole('button', { name: '关闭连接 host-0.example', exact: true }).click();
  await expect(page.getByRole('button', { name: '关闭连接 host-0.example', exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: '连接协议', exact: true }).click(); await chooseMenu(page, '仅 UDP');
  await expect(page.locator('.connection-row')).toHaveCount(30); await page.getByRole('button', { name: '关闭全部连接', exact: true }).click(); await expect(page.getByText('暂无活动连接', { exact: true })).toBeVisible();
});

test('stale snapshot cannot overwrite stop and reopening resets ephemeral filters only', async ({ page }) => {
  await installBridge(page); await page.goto('/'); await expect(page.getByRole('button', { name: '代理组 手动切换' })).toBeVisible();
  await page.evaluate(() => { (window as any).testState.deferSnapshot = true; }); await page.getByRole('tab', { name: '分流' }).click();
  await expect.poll(() => page.evaluate(() => Boolean((window as any).testState.resolveSnapshot))).toBe(true);
  await page.getByRole('button', { name: '停止内核', exact: true }).click(); await page.evaluate(() => (window as any).testState.resolveSnapshot());
  await expect(page.getByText('内核未启动', { exact: true })).toBeVisible();
  const search = page.getByRole('searchbox', { name: '搜索目标、类型、策略...' }); await search.fill('example');
  await page.evaluate(() => (window as any).testState.emit('popup-visibility', { visible: true, pinned: true })); await expect(search).toHaveValue('example');
  await page.evaluate(() => { (window as any).testState.emit('popup-visibility', { visible: false, pinned: true }); (window as any).testState.emit('popup-visibility', { visible: true, pinned: true }); });
  await expect(search).toHaveValue(''); await expect(page.getByRole('tab', { name: '分流' })).toHaveAttribute('aria-selected', 'true');
});

test('system toggle and logs keep original direct actions, multiple filters and bounded output', async ({ page }) => {
  await installBridge(page); await page.goto('/'); await page.getByLabel('系统代理', { exact: true }).click(); await expect(page.getByLabel('系统代理', { exact: true })).toBeChecked();
  await expect(page.getByRole('dialog')).toHaveCount(0); await page.getByRole('tab', { name: '日志' }).click();
  await expect(page.locator('.log-row')).toHaveCount(120); await expect(page.locator('.logs-list')).toContainText('log line 599');
  await page.getByRole('button', { name: '信息', exact: true }).click(); await page.getByRole('button', { name: '警告', exact: true }).click();
  await expect(page.getByRole('button', { name: '信息', exact: true })).toHaveAttribute('aria-pressed', 'true'); await expect(page.getByRole('button', { name: '警告', exact: true })).toHaveAttribute('aria-pressed', 'true');
  await page.getByRole('button', { name: '清理全部日志', exact: true }).click(); await expect(page.locator('.log-row')).toHaveCount(0);
  await page.getByRole('tab', { name: '设置' }).click(); await page.getByText('系统维护', { exact: true }).scrollIntoViewIfNeeded(); await expect(page.getByText('系统维护', { exact: true })).toBeVisible();
});

test('native submenu renderer supports keyboard selection, secondary action, theme and Escape', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 300, height: 160 }); await installBridge(page, true, true);
  await page.addInitScript(() => localStorage.setItem('clashbar-appearance', '深色')); await page.goto('/?surface=submenu');
  await expect(page.getByRole('menuitemradio', { name: /香港 01/ })).toBeFocused();
  await page.evaluate(() => (window as any).testState.emit('menu-focus', null));
  await page.keyboard.press('ArrowDown'); await expect(page.getByRole('button', { name: '测试 香港 01 延迟', exact: true })).toBeFocused();
  await page.keyboard.press('Enter'); await expect.poll(() => page.evaluate(() => (window as any).testState.commands.some((call: any) => call.command === 'attached_menu_action' && call.args.actionId === 'hongkong-test'))).toBe(true);
  await page.screenshot({ path: testInfo.outputPath('native-submenu-dark.png') });
  await expect(page.locator('html')).toHaveAttribute('data-appearance', 'dark');
  await page.keyboard.press('Escape'); await expect.poll(() => page.evaluate(() => (window as any).testState.commands.some((call: any) => call.command === 'hide_attached_menu'))).toBe(true);
});

test('规则提供者数量、别名和更新时间与旧端一致，刷新失败后可重试', async ({ page }) => {
  await installBridge(page);
  await page.addInitScript(() => {
    const state = (window as any).testState;
    state.snapshot.ruleProviders.providers = { original: { name: 'Alias', ruleCount: 1234, updatedAt: new Date().toISOString() }, unused: { ruleCount: 80 } };
    state.snapshot.rules.rules = [
      { type: 'RuleSet', payload: ' ALIAS ', proxy: 'DIRECT' },
      { type: 'RuleSet', payload: 'original', proxy: 'DIRECT' },
      { type: 'DOMAIN', payload: 'example.com', proxy: 'DIRECT' },
    ];
  });
  await page.goto('/'); await page.getByRole('tab', { name: '分流' }).click();
  await expect(page.getByText('规则 3  规则集 2', { exact: true })).toBeVisible();
  await expect(page.locator('.rule-stat')).toHaveText(['1234刚刚', '1234刚刚', '0']);
  await page.evaluate(() => { (window as any).testState.failMaintenance = '规则提供者更新完成 1/2；失败：original。'; });
  await page.getByRole('button', { name: '刷新规则', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('1/2');
  await expect(page.locator('.rule-row')).toHaveCount(3);
  await page.evaluate(() => { (window as any).testState.failMaintenance = null; });
  await page.getByRole('button', { name: '刷新规则', exact: true }).click();
  await expect(page.getByRole('alert')).toBeHidden();
  await expect(page.getByRole('status')).toContainText('刷新规则提供者已完成');
  expect(await page.evaluate(() => (window as any).testState.commands.filter((call: any) => call.command === 'refresh_rule_providers').length)).toBe(2);
});

test('维护动作串行执行，失败可重试，停止内核后禁用', async ({ page }) => {
  await installBridge(page); await page.goto('/'); await page.getByRole('tab', { name: '设置' }).click();
  const geo = page.getByRole('button', { name: '更新 Geo 数据库', exact: true });
  const dns = page.getByRole('button', { name: '清理 DNS 缓存', exact: true });
  const fakeip = page.getByRole('button', { name: '清理 FakeIP 缓存', exact: true });
  await page.evaluate(() => { (window as any).testState.deferMaintenance = true; });
  await geo.click(); await expect(geo).toBeDisabled(); await expect(dns).toBeDisabled();
  await expect(page.getByRole('status')).toContainText('更新 Geo 数据库…');
  await page.evaluate(() => { const state = (window as any).testState; state.failMaintenance = 'Geo 下载失败'; state.deferMaintenance = false; state.resolveMaintenance(); });
  await expect(page.getByRole('alert')).toContainText('Geo 下载失败'); await expect(geo).toBeEnabled();
  await page.evaluate(() => { (window as any).testState.failMaintenance = null; });
  await geo.click(); await expect(page.getByRole('status')).toContainText('更新 Geo 数据库已完成');
  await dns.click(); await expect(page.getByRole('status')).toContainText('清理 DNS 缓存已完成');
  await fakeip.click(); await expect(page.getByRole('status')).toContainText('清理 FakeIP 缓存已完成');
  const commands = await page.evaluate(() => (window as any).testState.commands.map((call: any) => call.command).filter((name: string) => ['upgrade_geo', 'flush_dns_cache', 'flush_fakeip_cache'].includes(name)));
  expect(commands).toEqual(['upgrade_geo', 'upgrade_geo', 'flush_dns_cache', 'flush_fakeip_cache']);
  await page.getByRole('button', { name: '停止内核', exact: true }).click();
  await expect(geo).toBeDisabled(); await expect(dns).toBeDisabled(); await expect(fakeip).toBeDisabled();
});

test('内核开关读取实际值、保存失败后恢复，并支持键盘操作', async ({ page }) => {
  await installBridge(page); await page.goto('/'); await page.getByRole('tab', { name: '设置' }).click();
  const lan = page.getByRole('checkbox', { name: '允许局域网', exact: true });
  const ipv6 = page.getByRole('checkbox', { name: 'IPv6', exact: true });
  const tcp = page.getByRole('checkbox', { name: 'TCP 并发', exact: true });
  await expect(lan).not.toBeChecked(); await expect(ipv6).toBeChecked(); await expect(tcp).not.toBeChecked();
  await lan.check(); await expect(lan).toBeChecked();
  await page.evaluate(() => { (window as any).testState.failCoreSetting = true; });
  await ipv6.click(); await expect(page.getByRole('alert')).toContainText('已恢复原值'); await expect(ipv6).toBeChecked();
  await page.evaluate(() => { (window as any).testState.failCoreSetting = false; });
  await ipv6.click(); await expect(ipv6).not.toBeChecked();
  await tcp.focus(); await page.keyboard.press('Space'); await expect(tcp).toBeChecked();
  await page.getByRole('tab', { name: '节点' }).click(); await page.getByRole('tab', { name: '设置' }).click();
  await expect(lan).toBeChecked(); await expect(ipv6).not.toBeChecked();
  await page.getByRole('button', { name: '停止内核', exact: true }).click();
  await expect(lan).toBeDisabled(); await expect(ipv6).toBeDisabled(); await expect(tcp).toBeDisabled();
});

test('配置删除先确认，失败保留对话框，删除最后配置后停止内核', async ({ page }) => {
  await installBridge(page); await page.goto('/');
  const config = page.getByRole('button', { name: /切换配置/ });
  await config.click(); await chooseMenu(page, 'clashmini.yaml', true); await chooseMenu(page, '删除配置');
  const dialog = page.getByRole('dialog');
  await expect(dialog.getByRole('button', { name: '取消', exact: true })).toBeFocused();
  await expect(dialog).toContainText('将切换');
  await page.keyboard.press('Escape'); await expect(dialog).toHaveCount(0); await expect(config).toBeFocused();
  expect(await page.evaluate(() => (window as any).testState.commands.some((call: any) => call.command === 'delete_profile'))).toBe(false);
  await config.click(); await chooseMenu(page, 'clashmini.yaml', true); await chooseMenu(page, '删除配置');
  await page.evaluate(() => { (window as any).testState.failDelete = true; });
  await dialog.getByRole('button', { name: '删除配置', exact: true }).click();
  await expect(dialog).toContainText('已恢复原配置');
  await page.evaluate(() => { (window as any).testState.failDelete = false; });
  await dialog.getByRole('button', { name: '删除配置', exact: true }).click(); await expect(dialog).toHaveCount(0);
  await expect(config).toContainText('备用配置.yaml');
  await config.click(); await chooseMenu(page, '备用配置.yaml', true); await chooseMenu(page, '删除配置');
  await expect(dialog).toContainText('删除后将停止内核');
  await dialog.getByRole('button', { name: '删除配置', exact: true }).click();
  await expect(config).toContainText('未导入'); await expect(page.getByRole('button', { name: '启动内核', exact: true })).toBeDisabled();
});

test('目录入口分发限定命令，内核停止时仍可打开目录', async ({ page }) => {
  await installBridge(page, false); await page.goto('/');
  await page.getByRole('button', { name: /切换配置/ }).click(); await chooseMenu(page, '打开配置目录');
  await expect(page.getByRole('status')).toContainText('打开配置目录已完成');
  await page.getByRole('tab', { name: '设置' }).click(); await page.getByRole('button', { name: '打开内核目录', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('打开内核目录已完成');
  expect(await page.evaluate(() => (window as any).testState.commands.filter((call: any) => ['open_profiles_directory', 'open_core_directory'].includes(call.command)))).toEqual([
    { command: 'open_profiles_directory', args: {} }, { command: 'open_core_directory', args: {} },
  ]);
});

test('配置菜单右键与 Shift+F10 分发相同管理动作', async ({ page }) => {
  await installBridge(page, true, true);
  await page.addInitScript(() => {
    (window as any).testState.menu = { id: 'profiles', title: '配置文件', items: [{ id: 'select', label: '本地配置.yaml', secondaryId: 'manage', secondaryKind: 'context', secondaryLabel: '管理配置 本地配置.yaml' }] };
  });
  await page.goto('/?surface=submenu');
  const item = page.getByRole('menuitem', { name: '本地配置.yaml', exact: true });
  await item.click({ button: 'right' });
  await item.focus(); await page.keyboard.press('Shift+F10');
  await page.getByRole('button', { name: '管理配置 本地配置.yaml', exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).testState.commands.filter((call: any) => call.command === 'attached_menu_action' && call.args.actionId === 'manage').length)).toBe(3);
});

async function openMachines(page: Page) {
  await page.getByRole('button', { name: /控制器，/ }).click(); await chooseMenu(page, '管理远程机器');
  await expect(page.getByRole('dialog', { name: '管理远程机器', exact: true })).toBeVisible();
}

async function addMachine(page: Page, name = '家里路由器') {
  await page.getByRole('button', { name: '添加机器', exact: true }).click();
  await page.getByLabel('名称', { exact: true }).fill(name);
  await page.getByLabel('主机地址', { exact: true }).fill('router.example');
  await page.getByLabel('密钥', { exact: true }).fill('fixture-remote-secret');
  await page.getByRole('button', { name: '保存', exact: true }).click();
  await expect(page.getByRole('dialog', { name: '管理远程机器', exact: true })).toBeVisible();
}

test('远程机器新增编辑保留密钥，切换隔离数据和本地生命周期，删除回到本机', async ({ page }, testInfo) => {
  await installBridge(page); await page.goto('/'); await openMachines(page);
  await addMachine(page);
  const dialog = page.getByRole('dialog');
  await expect(dialog.locator('.machine-select')).toBeEnabled();
  await dialog.screenshot({ path: testInfo.outputPath('machines-list.png') });
  await dialog.getByRole('button', { name: '编辑机器 家里路由器', exact: true }).click();
  await expect(page.getByLabel('密钥', { exact: true })).toHaveValue('');
  await expect(page.getByLabel('密钥', { exact: true })).toHaveAttribute('placeholder', '留空保留已保存的密钥');
  await dialog.screenshot({ path: testInfo.outputPath('machine-editor.png') });
  await page.getByLabel('名称', { exact: true }).fill('远程路由器');
  await page.getByRole('button', { name: '保存', exact: true }).click();
  await expect(dialog.locator('.machine-select')).toBeEnabled();
  await dialog.locator('.machine-select').click(); await expect(dialog).toHaveCount(0);
  await expect(page.getByRole('button', { name: '远程路由器控制器，运行中', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '停止内核', exact: true })).toBeDisabled();
  await expect(page.getByRole('button', { name: /切换配置/ })).toBeDisabled();
  await page.getByRole('tab', { name: '分流' }).click(); await expect(page.locator('.rule-target')).toContainText('remote.example');
  await page.getByRole('tab', { name: '日志' }).click(); await expect(page.locator('.log-message').filter({ hasText: '远程内核日志' })).toBeVisible();
  await expect(page.locator('.log-message').filter({ hasText: 'log line' })).toHaveCount(0);
  await page.getByRole('tab', { name: '设置' }).click(); await page.getByRole('checkbox', { name: 'IPv6', exact: true }).uncheck();
  await expect(page.getByRole('checkbox', { name: 'IPv6', exact: true })).not.toBeChecked();
  expect(await page.evaluate(() => (window as any).testState.localSnapshot.configs.ipv6)).toBe(true);
  await openMachines(page); await dialog.getByRole('button', { name: '删除机器 远程路由器', exact: true }).click();
  await expect(dialog.getByRole('button', { name: '取消', exact: true })).toBeFocused();
  await expect(dialog).toContainText('不会停止远程内核');
  await dialog.getByRole('button', { name: '删除机器', exact: true }).click();
  await dialog.getByRole('button', { name: '关闭', exact: true }).click();
  await expect(page.getByRole('button', { name: '本机控制器，运行中', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '停止内核', exact: true })).toBeEnabled();
  expect(await page.evaluate(() => (window as any).testState.commands.some((call: any) => call.command === 'stop_core'))).toBe(false);
  expect(await page.evaluate(() => JSON.stringify(localStorage))).not.toContain('fixture-remote-secret');
});

test('远程表单校验与保存失败保留输入，失败探测不能切换目标', async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 480 });
  await installBridge(page); await page.goto('/'); await openMachines(page);
  await page.getByRole('button', { name: '添加机器', exact: true }).click();
  await page.getByRole('button', { name: '保存', exact: true }).click();
  await expect(page.getByLabel('名称', { exact: true })).toBeFocused();
  await page.getByLabel('名称', { exact: true }).fill('离线设备');
  await page.getByLabel('主机地址', { exact: true }).fill('https://router.example');
  await page.getByRole('button', { name: '保存', exact: true }).click();
  await expect(page.getByLabel('主机地址', { exact: true })).toHaveAttribute('aria-invalid', 'true');
  await page.getByLabel('主机地址', { exact: true }).fill('router.example');
  await page.getByLabel('密钥', { exact: true }).fill('fixture-password');
  await page.evaluate(() => { (window as any).testState.failSaveRemote = true; });
  await page.getByRole('button', { name: '保存', exact: true }).click();
  await expect(page.getByRole('dialog')).toContainText('保存机器失败');
  await expect(page.getByLabel('密钥', { exact: true })).toHaveValue('fixture-password');
  await page.evaluate(() => { const state = (window as any).testState; state.failSaveRemote = false; state.failProbe = true; });
  await page.getByRole('button', { name: '保存', exact: true }).click();
  await expect(page.getByRole('dialog').locator('.machine-select')).toBeDisabled();
  await expect(page.getByRole('dialog').locator('.machine-state')).toContainText('连接失败');
  expect(await page.getByRole('dialog').evaluate(node => node.scrollWidth <= node.clientWidth)).toBe(true);
  await page.getByRole('dialog').getByRole('button', { name: '关闭', exact: true }).click();
  await page.getByRole('button', { name: /控制器，/ }).click(); await chooseMenu(page, '离线设备');
  await expect(page.getByRole('alert')).toContainText('连接失败');
  await expect(page.getByRole('button', { name: '本机控制器，运行中', exact: true })).toBeVisible();
});

test('远程端口校验重试与端点命令使用远程参数，不改写本地配置', async ({ page }) => {
  await installBridge(page);
  await page.addInitScript(() => { Object.defineProperty(navigator, 'clipboard', { value: { writeText: async (value: string) => { (window as any).testState.clipboard = value; } } }); });
  await page.goto('/'); await openMachines(page); await addMachine(page);
  await expect(page.getByRole('dialog').locator('.machine-select')).toBeEnabled(); await page.getByRole('dialog').locator('.machine-select').click();
  await page.getByRole('tab', { name: '设置' }).click();
  const mixed = page.getByLabel('混合端口', { exact: true });
  const http = page.getByLabel('HTTP 端口', { exact: true });
  await expect(mixed).toHaveValue('8890'); await expect(http).toBeEnabled();
  await http.fill('8890'); await http.press('Enter'); await expect(http).toHaveAttribute('aria-invalid', 'true');
  await http.fill('8080'); await page.getByLabel('SOCKS 端口', { exact: true }).fill('1080'); await mixed.fill('0'); await mixed.press('Enter');
  await expect(page.getByRole('status')).toContainText('保存端口已完成');
  expect(await page.evaluate(() => (window as any).testState.status.mixedPort)).toBe(7890);
  await page.evaluate(() => { (window as any).testState.failRemotePorts = true; });
  await mixed.fill('8990'); await mixed.press('Enter'); await expect(page.getByRole('alert')).toContainText('已恢复原内核端口'); await expect(mixed).toHaveValue('8990');
  await page.evaluate(() => { (window as any).testState.failRemotePorts = false; });
  await mixed.press('Enter'); await expect(page.getByRole('alert')).toBeHidden();
  await page.getByRole('tab', { name: '节点' }).click(); await page.getByRole('button', { name: '复制当前端点命令', exact: true }).click();
  expect(await page.evaluate(() => (window as any).testState.clipboard)).toContain('http://router.example:8990');
  await page.getByRole('button', { name: '复制 PowerShell 代理命令', exact: true }).click();
  expect(await page.evaluate(() => (window as any).testState.clipboard)).toContain('http://127.0.0.1:7890');
  await page.getByRole('button', { name: /控制器，/ }).click(); await chooseMenu(page, '本机');
  await expect(page.getByRole('button', { name: '本机控制器，运行中', exact: true })).toBeVisible();
  expect(await page.evaluate(() => (window as any).testState.commands.filter((call: any) => call.command === 'save_settings').length)).toBe(0);
});

test('远程编辑离开前保留草稿，密码清除需要明确选择', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 320, height: 480 }); await page.emulateMedia({ colorScheme: 'dark', reducedMotion: 'reduce' });
  await installBridge(page); await page.goto('/'); await openMachines(page); await addMachine(page);
  const dialog = page.getByRole('dialog');
  await dialog.getByRole('button', { name: '编辑机器 家里路由器', exact: true }).click();
  await page.getByLabel('名称', { exact: true }).fill('未保存的名称');
  await page.keyboard.press('Escape'); await expect(dialog).toContainText('放弃未保存的更改');
  await expect(page.getByRole('button', { name: '继续编辑', exact: true })).toBeFocused();
  await page.getByRole('button', { name: '继续编辑', exact: true }).click(); await expect(page.getByLabel('名称', { exact: true })).toHaveValue('未保存的名称');
  await page.getByLabel('清除已保存的密钥', { exact: true }).check(); await expect(page.getByLabel('密钥', { exact: true })).toBeDisabled();
  await dialog.screenshot({ path: testInfo.outputPath('machine-narrow-dark.png') });
  await page.getByRole('button', { name: '保存', exact: true }).click();
  expect(await page.evaluate(() => (window as any).testState.status.remoteMachines[0].hasSecret)).toBe(false);
  await dialog.getByRole('button', { name: '关闭', exact: true }).click(); await expect(page.getByRole('button', { name: /控制器，/ })).toBeFocused();
});

test('切换机器后迟到的旧快照不能覆盖远程数据', async ({ page }) => {
  await installBridge(page); await page.goto('/'); await openMachines(page); await addMachine(page);
  await page.getByRole('dialog').getByRole('button', { name: '关闭', exact: true }).click();
  await page.evaluate(() => { (window as any).testState.deferSnapshot = true; });
  await page.getByRole('tab', { name: '分流' }).click();
  await expect.poll(() => page.evaluate(() => Boolean((window as any).testState.resolveSnapshot))).toBe(true);
  await page.getByRole('button', { name: /控制器，/ }).click(); await chooseMenu(page, '家里路由器');
  await expect(page.locator('.rule-target')).toContainText('remote.example');
  await page.evaluate(() => (window as any).testState.resolveSnapshot());
  await expect(page.locator('.rule-row')).toHaveCount(1); await expect(page.locator('.rule-target')).toContainText('remote.example');
  await expect(page.getByRole('button', { name: '家里路由器控制器，运行中', exact: true })).toBeVisible();
});

test('开机自启以系统结果为准，内核自启只保存偏好', async ({ page }) => {
  await installBridge(page, false); await page.goto('/'); await page.getByRole('tab', { name: '设置' }).click();
  const login = page.getByRole('checkbox', { name: '开机自启', exact: true }); const core = page.getByRole('checkbox', { name: '内核自启', exact: true });
  await expect(login).not.toBeChecked(); await expect(core).not.toBeChecked();
  await page.evaluate(() => { (window as any).testState.failLaunch = true; });
  await login.click(); await expect(page.getByRole('alert')).toContainText('无法更改开机自启'); await expect(login).not.toBeChecked();
  await page.evaluate(() => { (window as any).testState.failLaunch = false; });
  await login.click(); await expect(login).toBeChecked();
  await core.click(); await expect(core).toBeChecked();
  expect(await page.evaluate(() => (window as any).testState.commands.some((call: any) => call.command === 'start_core'))).toBe(false);
  await page.getByRole('tab', { name: '节点' }).click(); await page.getByRole('tab', { name: '设置' }).click(); await expect(core).toBeChecked(); await expect(login).toBeChecked();
});

test('订阅导入失败保留输入，定时设置可编辑，链接复制与批量刷新分发到后端', async ({ page }) => {
  await installBridge(page); await page.goto('/');
  await page.getByRole('button', { name: /切换配置/ }).click(); await chooseMenu(page, '导入订阅链接...');
  await page.getByLabel('配置名称（可选）').fill('自动订阅'); await page.getByLabel('订阅链接', { exact: true }).fill('https://example.com/sub?token=private');
  await page.getByLabel('更新间隔（小时）').fill('0'); await page.getByRole('button', { name: '导入订阅', exact: true }).click();
  await expect(page.getByLabel('更新间隔（小时）')).toHaveAttribute('aria-invalid', 'true');
  await page.getByLabel('更新间隔（小时）').fill('12'); await page.evaluate(() => { (window as any).testState.failSubscription = true; });
  await page.getByRole('button', { name: '导入订阅', exact: true }).click(); await expect(page.getByRole('dialog')).toContainText('订阅下载失败');
  await expect(page.getByLabel('订阅链接', { exact: true })).toHaveValue('https://example.com/sub?token=private');
  expect(await page.getByRole('dialog').innerText()).not.toContain('token=');
  await page.evaluate(() => { (window as any).testState.failSubscription = false; }); await page.getByRole('button', { name: '导入订阅', exact: true }).click();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await page.getByRole('button', { name: /切换配置/ }).click(); await chooseMenu(page, '自动订阅.yaml', true);
  await expect.poll(() => page.evaluate(() => (window as any).testState.menu?.items.find((item: any) => item.label === '更新订阅')?.detail)).toBe('2m前');
  await chooseMenu(page, '编辑订阅与自动更新...');
  await expect(page.getByLabel('更新间隔（小时）')).toHaveValue('12'); await expect(page.getByLabel('订阅链接', { exact: true })).toHaveValue('');
  await page.getByLabel('自动更新', { exact: true }).uncheck(); await page.getByRole('button', { name: '保存订阅', exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).testState.status.subscriptions[0].autoUpdateEnabled)).toBe(false);
  await page.getByRole('button', { name: /切换配置/ }).click(); await chooseMenu(page, '自动订阅.yaml', true); await chooseMenu(page, '复制订阅链接');
  await expect.poll(() => page.evaluate(() => (window as any).testState.commands.some((call: any) => call.command === 'copy_subscription_url' && call.args.id === 'sub'))).toBe(true);
  await page.getByRole('button', { name: /切换配置/ }).click(); await chooseMenu(page, '更新全部订阅');
  await expect.poll(() => page.evaluate(() => (window as any).testState.commands.some((call: any) => call.command === 'refresh_all_subscriptions'))).toBe(true);
});

test('TUN失败回滚、Wi-Fi绑定管理与本机独立端口迁移', async ({ page }) => {
  await installBridge(page); await page.goto('/');
  await page.evaluate(() => { (window as any).testState.failTun = true; }); await page.getByRole('checkbox', { name: 'TUN 模式', exact: true }).click();
  await expect(page.getByRole('checkbox', { name: 'TUN 模式', exact: true })).not.toBeChecked(); await expect(page.getByRole('alert')).toContainText('TUN 授权失败');
  await page.evaluate(() => { (window as any).testState.failTun = false; }); await page.getByRole('checkbox', { name: 'TUN 模式', exact: true }).check();
  await page.getByRole('button', { name: 'TUN gvisor', exact: true }).click(); await expect(page.getByRole('button', { name: 'TUN gvisor', exact: true })).toHaveAttribute('aria-pressed', 'true');
  await page.getByRole('button', { name: /切换配置/ }).click(); await chooseMenu(page, 'SSID 自动切换');
  await page.getByRole('button', { name: /切换配置/ }).click(); await chooseMenu(page, 'clashmini.yaml', true); await chooseMenu(page, '绑定当前 Wi-Fi Home Wi-Fi');
  await page.getByRole('button', { name: /切换配置/ }).click(); await chooseMenu(page, '管理 Wi-Fi 绑定...'); await expect(page.getByRole('dialog')).toContainText('Home Wi-Fi → clashmini.yaml');
  await expect(page.getByRole('button', { name: '停用自动切换', exact: true })).toBeEnabled();
  await page.getByRole('button', { name: '解绑 Wi-Fi Home Wi-Fi', exact: true }).click(); await expect(page.getByRole('dialog')).not.toContainText('Home Wi-Fi →');
  await page.getByRole('button', { name: '关闭', exact: true }).click(); await page.getByRole('button', { name: '停止内核', exact: true }).click(); await page.getByRole('tab', { name: '设置', exact: true }).click();
  await page.getByLabel('HTTP 端口', { exact: true }).fill('80'); await page.getByLabel('SOCKS 端口', { exact: true }).fill('1080'); await page.getByLabel('混合端口', { exact: true }).fill('0'); await page.getByLabel('混合端口', { exact: true }).press('Enter');
  await expect.poll(() => page.evaluate(() => (window as any).testState.status.localProxyPorts)).toMatchObject({ port: 80, 'socks-port': 1080, 'mixed-port': 0 });
});

test('绕过规则失败重试保留输入，恢复旧版8项默认，更新和WebUI可操作', async ({ page }) => {
  await installBridge(page); await page.goto('/'); await page.getByRole('tab', { name: '设置', exact: true }).click();
  await page.getByRole('button', { name: '编辑', exact: true }).click(); await page.getByLabel('每行一个域名、IP 或通配符').fill('localhost\n*.example.com');
  await page.evaluate(() => { (window as any).testState.failExceptions = true; }); await page.getByRole('button', { name: '保存绕过', exact: true }).click();
  await expect(page.getByLabel('每行一个域名、IP 或通配符')).toHaveValue('localhost\n*.example.com');
  await page.evaluate(() => { (window as any).testState.failExceptions = false; }); await page.getByRole('button', { name: '恢复默认', exact: true }).click(); await page.getByRole('button', { name: '保存绕过', exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).testState.status.systemProxyExceptions.length)).toBe(8);
  await page.getByRole('button', { name: '打开 WebUI', exact: true }).click(); await expect.poll(() => page.evaluate(() => (window as any).testState.commands.find((call: any) => call.command === 'open_web_ui')?.args)).toEqual({});
  await page.getByRole('button', { name: '检查 ClashBar 更新', exact: true }).click(); await expect(page.getByRole('dialog')).toContainText('发现新版本 0.2.0');
  await page.getByRole('button', { name: '打开发布页', exact: true }).click(); await expect(page.getByRole('dialog')).toHaveCount(0);
  await page.getByRole('button', { name: '更新或选择 mihomo 内核', exact: true }).click(); await chooseMenu(page, '更新 mihomo 内核');
  await expect.poll(() => page.evaluate(() => (window as any).testState.commands.some((call: any) => call.command === 'upgrade_core'))).toBe(true);
});

test('中英切换保留外观和筛选偏好，英文窄窗与键盘快捷键可用', async ({ page }, testInfo) => {
  await installBridge(page); await page.goto('/'); await page.getByRole('tab', { name: '设置', exact: true }).click();
  await page.getByRole('button', { name: '外观模式', exact: true }).click(); await chooseMenu(page, '深色');
  await page.getByRole('button', { name: '界面语言', exact: true }).click(); await chooseMenu(page, 'English');
  await expect(page.getByRole('tab')).toHaveText(['Proxies', 'Rules', 'Connections', 'Logs', 'Settings']);
  await expect(page.getByRole('tab', { name: 'Settings', exact: true })).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('html')).toHaveAttribute('data-appearance', 'dark');
  await page.getByRole('button', { name: 'Appearance', exact: true }).click(); await chooseMenu(page, 'Light');
  await page.getByRole('button', { name: 'Tray style', exact: true }).click(); await chooseMenu(page, 'Speed only');
  await expect.poll(() => page.evaluate(() => (window as any).testState.status.statusBarStyle)).toBe('speedOnly');
  await page.setViewportSize({ width: 320, height: 640 }); await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.keyboard.press('Control+Alt+1'); await expect(page.getByRole('tab', { name: 'Proxies', exact: true })).toHaveAttribute('aria-selected', 'true');
  await page.keyboard.press('Control+S'); await expect(page.getByRole('checkbox', { name: 'System proxy', exact: true })).toBeChecked();
  await page.keyboard.press('Control+E'); await expect(page.getByRole('checkbox', { name: 'TUN mode', exact: true })).toBeChecked();
  await page.keyboard.press('Control+Shift+2'); await expect(page.getByRole('button', { name: 'Global mode', exact: true })).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByRole('button', { name: 'Copy LAN proxy command', exact: true })).toBeEnabled();
  await expect(page.getByRole('button', { name: 'Copy PowerShell proxy command', exact: true })).toBeVisible();
  expect(await page.getByRole('button', { name: 'Copy LAN proxy command', exact: true }).evaluate(node => node.getBoundingClientRect().right <= window.innerWidth)).toBe(true);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: testInfo.outputPath('english-narrow.png'), fullPage: true });
  await page.keyboard.press('Control+,'); await page.getByRole('button', { name: 'Language', exact: true }).click(); await chooseMenu(page, '简体中文');
  await expect(page.getByRole('tab', { name: '设置', exact: true })).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('html')).toHaveAttribute('data-appearance', 'light');
});

test('流量优先使用WebSocket实测速率，并隐藏default与Compatible提供者', async ({ page }) => {
  await installBridge(page); await page.goto('/');
  await page.evaluate(() => { const state = (window as any).testState; state.snapshot.traffic = { up: 1024, down: 2048 }; state.snapshot.providers.providers.default = { vehicleType: 'HTTP' }; state.snapshot.providers.providers.compat = { vehicleType: 'Compatible' }; state.emit('popup-visibility', { visible: true, pinned: false }); });
  await expect(page.locator('.traffic')).toContainText('1 KB/s'); await expect(page.locator('.traffic')).toContainText('2 KB/s');
  await expect(page.locator('.provider-name')).toHaveText(['ccsub', 'jmsub']);
});

test('局域网代理命令使用真实地址，mips按版本禁用，提供者后台进度显示', async ({ page }) => {
  await installBridge(page); await page.goto('/');
  await expect(page.getByRole('button', { name: '停止内核', exact: true })).toBeEnabled();
  await page.keyboard.press('Control+Shift+R'); await expect.poll(() => page.evaluate(() => (window as any).testState.commands.some((call: any) => call.command === 'restart_core'))).toBe(true);
  await expect(page.getByRole('button', { name: '停止内核', exact: true })).toBeEnabled();
  await page.evaluate(() => { Object.defineProperty(navigator, 'clipboard', { value: { writeText: async (text: string) => { (window as any).copied = text; } }, configurable: true }); });
  await page.getByRole('button', { name: '复制局域网代理命令', exact: true }).click(); expect(await page.evaluate(() => (window as any).copied)).toContain('http://192.168.10.8:7890');
  await expect(page.getByRole('button', { name: 'TUN mips', exact: true })).toBeEnabled();
  await page.evaluate(() => { const state = (window as any).testState; state.status.version = '1.19.30'; state.status.localLanAddress = null; state.status.providerRefresh = { running: true, done: 2, total: 5, failed: 0, error: null }; state.emit('popup-visibility', { visible: true, pinned: false }); });
  await expect(page.getByRole('button', { name: 'TUN mips', exact: true })).toBeDisabled(); await expect(page.getByRole('button', { name: '复制局域网代理命令', exact: true })).toBeDisabled();
  await expect(page.locator('.section-heading .provider-age')).toHaveText('2/5');
  await page.evaluate(() => { const state = (window as any).testState; state.status.ssidSnapshot = { currentSsid: null, status: 'permissionDenied', error: '无法读取当前 Wi-Fi 名称。请在 Windows 定位设置中允许访问后重试。' }; state.emit('popup-visibility', { visible: true, pinned: false }); });
  await page.getByRole('button', { name: /切换配置/ }).click(); await chooseMenu(page, '管理 Wi-Fi 绑定...'); await page.getByRole('button', { name: '打开定位设置', exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).testState.commands.some((call: any) => call.command === 'open_location_settings'))).toBe(true);
});

test('代理组图标只使用后端缓存，动作日志回读去重，英文后端错误保留端口信息', async ({ page }) => {
  await installBridge(page); await page.addInitScript(() => localStorage.setItem('clashbar-language', 'en')); await page.goto('/');
  await page.evaluate(() => {
    const state = (window as any).testState; const canvas = document.createElement('canvas'); canvas.width = canvas.height = 16;
    canvas.getContext('2d')!.fillRect(0, 0, 16, 16); state.iconData = canvas.toDataURL();
    state.snapshot.proxies.proxies['手动切换'].icon = 'https://icons.example.test/group.png'; state.emit('popup-visibility', { visible: true, pinned: false });
  });
  const group = page.getByRole('button', { name: 'Proxy group 手动切换', exact: true }); await expect(group.locator('.group-icon')).toHaveAttribute('src', /^data:image\/png/);
  await page.getByRole('button', { name: 'Global mode', exact: true }).click(); await page.getByRole('tab', { name: 'Logs', exact: true }).click();
  await page.getByRole('button', { name: 'ClashBar', exact: true }).click(); await expect(page.locator('.log-message').filter({ hasText: 'Change proxy mode completed' })).toHaveCount(1);
  await page.evaluate(() => { const state = (window as any).testState; state.status.lastError = '代理 TCP 端口 7890 已被占用，请在设置中更换。'; state.emit('popup-visibility', { visible: true, pinned: false }); });
  await expect(page.getByRole('alert')).toContainText('Proxy TCP port 7890 is in use. Change it in settings.');
});

test('日志复制保留完整元信息，连接显示规则值并搜索协议时间，失效策略回到全部', async ({ page }) => {
  await installBridge(page); await page.goto('/');
  await page.evaluate(() => { const state = (window as any).testState; state.logs = ['info payload message']; state.snapshot.connections.connections = [{ id: 'q', metadata: { host: 'quic.example', network: 'quic' }, rule: 'DOMAIN-SUFFIX', rulePayload: 'example.com', start: '2026-10-08T12:00:00Z' }, { id: 'unknown', metadata: { host: 'unknown.example' } }]; Object.defineProperty(navigator, 'clipboard', { value: { writeText: async (text: string) => { (window as any).copied = text; } }, configurable: true }); });
  await page.getByRole('tab', { name: '日志', exact: true }).click(); const row = page.locator('.log-row').filter({ hasText: 'payload message' }); await expect(row).toBeVisible();
  await row.press('Shift+F10'); await chooseMenu(page, '复制消息'); expect(await page.evaluate(() => (window as any).copied)).toBe('info payload message');
  await row.press('Shift+F10'); await chooseMenu(page, '复制完整记录'); expect(await page.evaluate(() => (window as any).copied)).toMatch(/^\[\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\] \[MIHOMO\] \[INFO\] info payload message$/);
  await page.getByRole('button', { name: '复制全部日志', exact: true }).click(); expect(await page.evaluate(() => (window as any).copied)).toContain('[MIHOMO] [INFO] info payload message');
  await page.getByRole('tab', { name: '连接', exact: true }).click(); await expect(page.locator('.rule-badge').first()).toHaveText('DOMAIN-SUFFIX · example.com');
  await page.getByRole('button', { name: '连接协议', exact: true }).click(); await chooseMenu(page, '其他协议'); await expect(page.locator('.connection-row')).toHaveCount(1);
  await page.getByLabel('过滤域名、IP...', { exact: true }).fill('2026-10-08T12:00:00Z'); await expect(page.locator('.connection-row')).toHaveCount(1); await page.getByLabel('过滤域名、IP...', { exact: true }).fill('quic'); await expect(page.locator('.connection-row')).toHaveCount(1);
  await page.getByRole('tab', { name: '分流', exact: true }).click(); await page.getByRole('button', { name: '筛选策略', exact: true }).click(); await chooseMenu(page, '手动切换');
  await page.evaluate(() => { const state = (window as any).testState; state.snapshot.rules.rules = [{ type: 'DOMAIN', payload: 'changed.example', proxy: 'DIRECT' }]; state.emit('popup-visibility', { visible: true, pinned: false }); });
  await expect(page.getByRole('button', { name: '筛选策略', exact: true })).toHaveText('全部策略'); await expect(page.getByText('changed.example', { exact: true })).toBeVisible();
  await page.getByRole('tab', { name: '节点', exact: true }).click(); await page.getByRole('button', { name: /切换配置/ }).click(); await chooseMenu(page, '在文件夹中显示当前配置');
  await expect.poll(() => page.evaluate(() => (window as any).testState.commands.some((call: any) => call.command === 'open_selected_profile'))).toBe(true);
});

test('文件同名导入确认与失败重试保留原选择，取消清理待导入路径', async ({ page }) => {
  await installBridge(page); await page.goto('/'); await page.evaluate(() => { (window as any).testState.importName = 'clashmini.yaml'; });
  await page.getByRole('button', { name: /切换配置/ }).click(); await chooseMenu(page, '导入配置文件...'); await expect(page.getByRole('dialog')).toContainText('覆盖配置「clashmini.yaml」');
  await expect(page.getByRole('button', { name: '取消', exact: true })).toBeFocused(); await page.getByRole('button', { name: '取消', exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).testState.commands.some((call: any) => call.command === 'cancel_config_import'))).toBe(true);
  await page.getByRole('button', { name: /切换配置/ }).click(); await chooseMenu(page, '导入配置文件...'); await page.evaluate(() => { (window as any).testState.failImport = true; }); await page.getByRole('button', { name: '覆盖配置', exact: true }).click(); await expect(page.getByRole('dialog')).toContainText('配置导入失败');
  await page.evaluate(() => { (window as any).testState.failImport = false; }); await page.getByRole('button', { name: '覆盖配置', exact: true }).click(); await expect(page.getByRole('dialog')).toHaveCount(0);
  expect(await page.evaluate(() => (window as any).testState.status.profiles.length)).toBe(2);
  await page.evaluate(() => { (window as any).testState.importName = 'new-profile.yaml'; }); await page.getByRole('button', { name: /切换配置/ }).click(); await chooseMenu(page, '导入配置文件...');
  await expect.poll(() => page.evaluate(() => (window as any).testState.status.profiles.length)).toBe(3); await expect(page.getByRole('button', { name: /切换配置/ })).toContainText('clashmini.yaml');
});

test('订阅规范名称冲突先确认覆盖，修改名称撤销确认，新增保留当前配置', async ({ page }) => {
  await installBridge(page); await page.goto('/'); await page.getByRole('button', { name: /切换配置/ }).click(); await chooseMenu(page, '导入订阅链接...');
  await page.getByLabel('配置名称（可选）').fill('clashmini'); await page.getByLabel('订阅链接', { exact: true }).fill('https://example.com/sub'); await page.getByRole('button', { name: '导入订阅', exact: true }).click();
  await expect(page.getByRole('dialog')).toContainText('配置「clashmini.yaml」已存在'); expect(await page.evaluate(() => (window as any).testState.commands.filter((call: any) => call.command === 'add_subscription').length)).toBe(0);
  await page.getByLabel('配置名称（可选）').fill('fresh'); await expect(page.getByRole('button', { name: '导入订阅', exact: true })).toBeVisible(); await page.getByRole('button', { name: '导入订阅', exact: true }).click(); await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(page.getByRole('button', { name: /切换配置/ })).toContainText('clashmini.yaml');
  await page.getByRole('button', { name: /切换配置/ }).click(); await chooseMenu(page, '导入订阅链接...'); await page.getByLabel('配置名称（可选）').fill('clashmini'); await page.getByLabel('订阅链接', { exact: true }).fill('https://example.com/sub'); await page.getByRole('button', { name: '导入订阅', exact: true }).click(); await page.getByRole('button', { name: '覆盖配置', exact: true }).click(); await expect(page.getByRole('dialog')).toHaveCount(0);
  expect(await page.evaluate(() => (window as any).testState.commands.filter((call: any) => call.command === 'add_subscription').at(-1).args.input.overwrite)).toBe(true);
  expect(await page.evaluate(() => (window as any).testState.status.profiles.filter((item: any) => item.name === 'clashmini.yaml').length)).toBe(1);
});
