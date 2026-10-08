import { expect, test, type Page } from '@playwright/test';

test.afterEach(async ({ page }, testInfo) => {
  await page.screenshot({ path: testInfo.outputPath('screen.png'), fullPage: true });
});

async function installBridge(page: Page, running = true) {
  await page.addInitScript(({ running }) => {
    const state = {
      status: { running, corePath: 'C:\\mihomo.exe', configName: 'local.yaml', mixedPort: 7890, controllerPort: 9090, systemProxy: false, version: 'test', lastError: null },
      snapshot: {
        proxies: { proxies: { '香港分组': { type: 'Selector', now: '香港 01', all: ['香港 01', '东京 02'] }, '香港 01': { type: 'Shadowsocks', history: [] } } },
        configs: { mode: 'rule' },
        rules: { rules: Array.from({ length: 112 }, (_, index) => ({ type: 'DOMAIN-SUFFIX', payload: `host-${index}.example`, proxy: '香港分组' })) },
        connections: { connections: Array.from({ length: 61 }, (_, index) => ({ id: `connection-${index}`, metadata: { host: `host-${index}.example`, destinationPort: '443', network: 'tcp', process: 'browser.exe' }, chains: ['香港 01'], upload: 1024, download: 2048 })), uploadTotal: 10000, downloadTotal: 20000 },
        providers: { providers: { '本地提供者': { vehicleType: 'HTTP', proxies: [] } } },
      },
      commands: [] as { command: string; args: Record<string, unknown> }[],
      failClose: false,
      deferSnapshot: false,
      resolveSnapshot: null as null | (() => void),
    };
    Object.assign(window, { isTauri: true, testState: state, __TAURI_INTERNALS__: {
      invoke: async (command: string, args: Record<string, unknown> = {}) => {
        state.commands.push({ command, args });
        if (command === 'get_status') return structuredClone(state.status);
        if (command === 'get_snapshot') {
          const captured = structuredClone(state.snapshot);
          if (state.deferSnapshot) { state.deferSnapshot = false; await new Promise<void>(resolve => { state.resolveSnapshot = resolve; }); }
          return captured;
        }
        if (command === 'get_logs') return Array.from({ length: 600 }, (_, i) => `log line ${i}`);
        if (command === 'set_mode') state.snapshot.configs.mode = String(args.mode);
        if (command === 'select_proxy') state.snapshot.proxies.proxies['香港分组'].now = String(args.name);
        if (command === 'test_delay') return { delay: 42 };
        if (command === 'close_connection') {
          await new Promise(resolve => setTimeout(resolve, 150));
          if (state.failClose) throw new Error('Test close failure');
          state.snapshot.connections.connections = state.snapshot.connections.connections.filter(c => c.id !== args.id);
        }
        if (command === 'start_core') state.status.running = true;
        if (command === 'stop_core') { state.status.running = false; state.status.systemProxy = false; }
        if (command === 'set_system_proxy') state.status.systemProxy = Boolean(args.enabled);
        if (command === 'save_settings') { state.status.mixedPort = Number(args.mixedPort); state.status.controllerPort = Number(args.controllerPort); }
        if (command === 'choose_core') state.status.corePath = 'C:\\new-mihomo.exe';
        if (command === 'import_config' || command === 'import_subscription') state.status.configName = 'imported.yaml';
        return structuredClone(state.status);
      },
    } });
  }, { running });
}

test('browser-only mode is explicit and cannot mutate desktop state', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByText('仅桌面可用')).toBeVisible();
  await expect(page.getByRole('button', { name: '启动内核' })).toBeDisabled();
  await page.getByRole('tab', { name: '设置' }).click();
  await expect(page.getByRole('button', { name: '选择 mihomo.exe' })).toBeDisabled();
});

test('settings validate, preserve drafts across tabs, import, save and launch', async ({ page }) => {
  await installBridge(page, false); await page.goto('/');
  await page.getByRole('tab', { name: '设置' }).click();
  await expect(page.getByLabel('HTTP / SOCKS 代理端口')).toHaveValue('7890');
  await page.getByLabel('HTTP / SOCKS 代理端口').fill('9090');
  await page.getByRole('button', { name: '保存端口' }).click();
  await expect(page.getByLabel('HTTP / SOCKS 代理端口')).toHaveAttribute('aria-invalid', 'true');
  await page.getByLabel('HTTP / SOCKS 代理端口').fill('7891');
  await page.getByRole('tab', { name: '规则' }).click();
  await page.getByRole('tab', { name: '设置' }).click();
  await expect(page.getByLabel('HTTP / SOCKS 代理端口')).toHaveValue('7891');
  await page.getByRole('button', { name: '保存端口' }).click();
  await expect(page.getByRole('status')).toContainText('保存端口已完成');
  await page.getByLabel('订阅链接').fill('http://example.com/private');
  await page.getByRole('button', { name: '导入订阅', exact: true }).click();
  await expect(page.getByLabel('订阅链接')).toHaveAttribute('aria-invalid', 'true');
  await page.getByLabel('订阅链接').fill('https://example.com/sub?token=private');
  await expect(page.getByLabel('订阅链接')).toHaveAttribute('type', 'password');
  await page.getByRole('button', { name: '导入订阅', exact: true }).click();
  await page.getByRole('button', { name: '导入并替换配置' }).click();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(page.getByLabel('订阅链接')).toHaveValue('');
  await page.getByRole('button', { name: '启动内核' }).click();
  await expect(page.getByText('运行中', { exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '保存端口' })).toBeDisabled();
});

test('proxy selection, mode, delay, providers and keyboard tabs use the IPC contract', async ({ page }, testInfo) => {
  await installBridge(page); await page.goto('/');
  await page.getByLabel('香港分组', { exact: true }).selectOption('东京 02');
  await expect.poll(() => page.evaluate(() => (window as any).testState.snapshot.proxies.proxies['香港分组'].now)).toBe('东京 02');
  await page.getByLabel('代理模式', { exact: true }).selectOption('global');
  await page.getByRole('button', { name: '测试 香港分组 的当前节点延迟' }).click();
  await expect(page.getByText('42 ms')).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath('proxies.png'), fullPage: true });
  await page.getByRole('button', { name: '更新代理提供者 本地提供者' }).click();
  await expect.poll(() => page.evaluate(() => (window as any).testState.commands.some((c: any) => c.command === 'update_provider'))).toBeTruthy();
  await page.getByRole('tab', { name: '代理', exact: true }).focus();
  await page.keyboard.press('ArrowRight');
  await expect(page.getByRole('tab', { name: '规则' })).toBeFocused();
  await expect(page.getByRole('tab', { name: '规则' })).toHaveAttribute('aria-selected', 'true');
});

test('rules paginate 50 rows, filter with IME and clear without losing search focus', async ({ page }) => {
  await installBridge(page); await page.goto('/');
  await page.getByRole('tab', { name: '规则' }).click();
  await expect(page.locator('#panel-rules tbody tr')).toHaveCount(50);
  await page.getByRole('button', { name: '下一页' }).click();
  await expect(page.getByText('51–100 / 112 条')).toBeVisible();
  const search = page.getByRole('searchbox', { name: '筛选规则、内容或策略' });
  await search.dispatchEvent('compositionstart');
  await search.fill('host-111');
  await expect(page.locator('#panel-rules tbody tr')).toHaveCount(50);
  await search.dispatchEvent('compositionend');
  await expect(page.locator('#panel-rules tbody tr')).toHaveCount(1);
  await page.getByRole('button', { name: '清除筛选规则、内容或策略' }).click();
  await expect(search).toBeFocused();
  await expect(page.locator('#panel-rules tbody tr')).toHaveCount(50);
});

test('connection dialog traps focus, preserves failure, prevents duplicate close and restores focus', async ({ page }) => {
  await installBridge(page); await page.goto('/');
  await page.getByRole('tab', { name: '连接' }).click();
  await page.evaluate(() => { (window as any).testState.failClose = true; });
  await page.getByRole('button', { name: '关闭连接 host-0.example', exact: true }).click();
  const dialog = page.getByRole('dialog');
  await expect(dialog.getByRole('button', { name: '取消' })).toBeFocused();
  await page.keyboard.press('Shift+Tab');
  await expect(dialog.getByRole('button', { name: '关闭连接', exact: true })).toBeFocused();
  await dialog.getByRole('button', { name: '关闭连接', exact: true }).click();
  await expect(dialog.getByRole('button', { name: '关闭连接', exact: true })).toBeDisabled();
  await expect(dialog).toContainText('操作未完成');
  await page.evaluate(() => { (window as any).testState.failClose = false; });
  await dialog.getByRole('button', { name: '关闭连接', exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole('button', { name: '关闭连接 host-0.example', exact: true })).toHaveCount(0);
  await expect(page.getByRole('tab', { name: '连接' })).toBeFocused();
  await expect.poll(() => page.evaluate(() => (window as any).testState.commands.filter((c: any) => c.command === 'close_connection').length)).toBe(2);
});

test('a stale snapshot cannot replace state after stopping the core', async ({ page }) => {
  await installBridge(page); await page.goto('/');
  await expect(page.getByLabel('香港分组', { exact: true })).toBeVisible();
  await page.evaluate(() => { (window as any).testState.deferSnapshot = true; });
  await page.getByRole('button', { name: '刷新', exact: true }).click();
  await expect.poll(() => page.evaluate(() => Boolean((window as any).testState.resolveSnapshot))).toBe(true);
  await page.getByRole('button', { name: '停止内核' }).click();
  await expect(page.getByText('已停止', { exact: true })).toBeVisible();
  await page.evaluate(() => { (window as any).testState.resolveSnapshot(); });
  await expect(page.getByText('内核未启动', { exact: true })).toBeVisible();
  await expect(page.getByLabel('香港分组', { exact: true })).toHaveCount(0);
});

test('system proxy is confirmed, logs are bounded, narrow layout keeps settings reachable', async ({ page }) => {
  await installBridge(page); await page.goto('/');
  await page.getByLabel('系统代理', { exact: true }).click();
  await expect(page.getByRole('dialog')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByLabel('系统代理', { exact: true })).not.toBeChecked();
  await page.getByLabel('系统代理', { exact: true }).click();
  await page.getByRole('button', { name: '开启系统代理', exact: true }).click();
  await expect(page.getByLabel('系统代理', { exact: true })).toBeChecked();
  await page.getByRole('tab', { name: '日志' }).click();
  await expect(page.getByLabel('内核日志')).toContainText('log line 599');
  await expect(page.getByLabel('内核日志')).not.toContainText('log line 99\n');
  await page.setViewportSize({ width: 360, height: 640 });
  await page.emulateMedia({ colorScheme: 'dark', reducedMotion: 'reduce' });
  await page.getByRole('tab', { name: '设置' }).click();
  await page.getByRole('button', { name: '保存端口' }).scrollIntoViewIfNeeded();
  await expect(page.getByRole('button', { name: '保存端口' })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  expect(await page.evaluate(() => getComputedStyle(document.documentElement).scrollbarColor)).not.toBe('auto');
});
