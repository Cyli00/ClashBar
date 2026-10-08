import { expect, test, type Page } from '@playwright/test';

// Every fixture is isolated to the test bridge. Production never invents core data.
async function installBridge(page: Page, running = true, submenu = false) {
  await page.addInitScript(({ running, submenu }) => {
    const callbacks = new Map<number, (event: unknown) => void>();
    const listeners = new Map<string, number[]>(); let nextCallback = 0;
    const groups = ['广告拦截', '苹果服务', '手动切换', '自动选择', '节点选择', '瓦工节点', '美国节点', '美国 AN', '日本节点', '狮城节点', '韩国节点', '欧洲节点'];
    const proxies: Record<string, any> = Object.fromEntries(groups.map((name, index) => [name, { type: 'Selector', now: index === 0 ? 'REJECT' : index === 1 ? 'DIRECT' : '香港 01', all: ['香港 01', '东京 02', '新加坡 03'], history: [{ delay: 92 + index * 13 }] }]));
    Object.assign(proxies, { '香港 01': { type: 'Shadowsocks', history: [{ delay: 156 }] }, '东京 02': { type: 'Trojan', history: [{ delay: 84 }] }, '新加坡 03': { type: 'VLESS', history: [{ delay: 64 }] } });
    const state: any = {
      status: { running, corePath: 'C:\\mihomo.exe', configName: 'clashmini.yaml', profiles: [{ id: 'one', name: 'clashmini.yaml' }, { id: 'two', name: '备用配置.yaml' }], activeProfileId: 'one', mixedPort: 7890, controllerPort: 9090, systemProxy: false, version: '1.19.31', lastError: null },
      snapshot: {
        proxies: { proxies }, configs: { mode: 'rule', 'log-level': 'info' }, memory: 107 * 1024 * 1024,
        rules: { rules: Array.from({ length: 112 }, (_, index) => ({ type: 'DOMAIN-SUFFIX', payload: `host-${index}.example`, proxy: index % 2 ? '手动切换' : '自动选择' })) },
        connections: { connections: Array.from({ length: 61 }, (_, index) => ({ id: `connection-${index}`, metadata: { host: `host-${index}.example`, destinationPort: '443', network: index % 2 ? 'udp' : 'tcp', process: 'browser.exe' }, rule: 'DOMAIN', start: '2026-10-08T12:00:00Z', chains: ['香港 01', '手动切换'], upload: 1024, download: 2048 })), uploadTotal: 641800, downloadTotal: 170100000 },
        providers: { providers: { ccsub: { vehicleType: 'HTTP', updatedAt: new Date().toISOString(), proxies: Array(119).fill({ type: 'VLESS' }), subscriptionInfo: { upload: 0, download: 28800000000000, total: 34100000000000, expire: 1810000000 } }, jmsub: { vehicleType: 'HTTP', updatedAt: new Date().toISOString(), proxies: Array(6).fill({ type: 'Shadowsocks' }) } } },
      },
      logs: Array.from({ length: 600 }, (_, i) => `12:01:00 info [TCP] log line ${i}`),
      commands: [], failClose: false, deferSnapshot: false, resolveSnapshot: null, pinned: false,
      menu: submenu ? { id: 'sample-menu', title: '手动切换  3', items: [{ id: 'hongkong', label: '香港 01', detail: 'Shadowsocks', value: '156', checked: true, secondaryId: 'hongkong-test', secondaryLabel: '测试 香港 01 延迟' }, { id: 'tokyo', label: '东京 02', detail: 'Trojan', value: '84', checked: false }, { id: 'singapore', label: '新加坡 03', detail: 'VLESS', value: '64', checked: false }] } : null,
    };
    const emit = (event: string, payload: unknown) => { for (const id of listeners.get(event) || []) callbacks.get(id)?.({ event, payload, id }); };
    state.emit = emit;
    const call = async (command: string, args: any = {}) => {
      state.commands.push({ command, args });
      if (command === 'plugin:event|listen') { listeners.set(args.event, [...(listeners.get(args.event) || []), args.handler]); return args.handler; }
      if (command === 'plugin:event|unlisten') return;
      if (command === 'get_status') return structuredClone(state.status);
      if (command === 'get_snapshot') { const captured = structuredClone(state.snapshot); if (state.deferSnapshot) { state.deferSnapshot = false; await new Promise(resolve => { state.resolveSnapshot = resolve; }); } return captured; }
      if (command === 'get_logs') return state.logs;
      if (command === 'get_log_entries') return state.logs.map((message: string, index: number) => ({ timestamp: Date.parse('2026-10-08T12:01:00Z') + index, source: 'Mihomo', message }));
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
      if (command === 'start_core' || command === 'restart_core') state.status.running = true;
      if (command === 'stop_core') { state.status.running = false; state.status.systemProxy = false; }
      if (command === 'set_system_proxy') state.status.systemProxy = args.enabled;
      if (command === 'save_settings') Object.assign(state.status, args);
      if (command === 'select_profile') { state.status.activeProfileId = args.id; state.status.configName = state.status.profiles.find((profile: any) => profile.id === args.id).name; }
      if (command === 'import_subscription' || command === 'import_config') state.status.configName = 'imported.yaml';
      if (command === 'clear_logs') state.logs = [];
      if (command === 'set_log_level') state.snapshot.configs['log-level'] = args.level;
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

test('source-style populated popup renders at 360px in light and dark', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 360, height: 900 }); await installBridge(page); await page.goto('/');
  await expect(page.getByRole('button', { name: '代理组 手动切换', exact: true })).toBeVisible();
  await expect(page.getByRole('tab')).toHaveText(['节点', '分流', '连接', '日志', '设置']);
  await expect(page.getByText('MIHOMO v1.19.31')).toBeVisible();
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
