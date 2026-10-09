import test from 'node:test';
import assert from 'node:assert/strict';
import { formatBytes, matchesQuery, pageSlice, ReadEpoch, safeError, terminalProxyCommand, validateLocalPorts, validatePorts, validateRemotePorts, validateSubscription } from '../src/utils.ts';

test('local pagination clamps after deletion and filters, including empty results', () => {
  const rows = Array.from({ length: 101 }, (_, index) => index);
  assert.deepEqual(pageSlice(rows, 3).items, [100]);
  assert.equal(pageSlice(rows.slice(0, 50), 3).page, 1);
  assert.deepEqual(pageSlice([], 20), { items: [], page: 1, pages: 1, total: 0, start: 0, end: 0 });
});

test('ports are distinct non-privileged integer ports', () => {
  assert.equal(validatePorts('7890', '9090'), null);
  for (const pair of [['0', '9090'], ['80', '9090'], ['65536', '9090'], ['7890.1', '9090'], ['7890', '7890']]) assert.ok(validatePorts(...pair));
});

test('subscription validator rejects credential, scheme, fragment and port ambiguity', () => {
  assert.equal(validateSubscription('https://example.com/sub?token=private'), null);
  assert.equal(validateSubscription('https://example.com:443/sub'), null);
  for (const url of ['http://example.com', 'javascript:alert(1)', 'https://user:pass@example.com', 'https://example.com/#secret', 'https://example.com:8443/sub', 'wrong']) assert.ok(validateSubscription(url));
});

test('error feedback redacts complete subscription URLs', () => {
  const result = safeError(new Error('Failed https://example.com/sub?token=secret and http://local/a'));
  assert.equal(result, 'Failed [链接已隐藏] and [链接已隐藏]');
  assert.equal(safeError('x'.repeat(600)).length, 500);
});

test('an intervening mutation invalidates old read completion', () => {
  const epoch = new ReadEpoch();
  const old = epoch.next();
  epoch.next();
  const latest = epoch.next();
  assert.equal(epoch.current(old), false);
  assert.equal(epoch.current(latest), true);
});

test('search and measurements handle missing and multilingual values honestly', () => {
  assert.equal(matchesQuery([undefined, '香港 Example'], 'EXAMPLE'), true);
  assert.equal(matchesQuery(['香港'], '东京'), false);
  assert.equal(formatBytes(undefined), '—');
  assert.equal(formatBytes(1024), '1 KiB');
  assert.equal(formatBytes(0), '0 B');
});

test('远程端口支持关闭监听，并拒绝重复与非法整数', () => {
  assert.equal(validateRemotePorts(['0', '0', '7890', '0', '0']), null);
  assert.equal(validateRemotePorts(['80', '1080', '0', '0', '0']), null);
  for (const values of [['-1', '0'], ['65536', '0'], ['', '0'], ['1.5', '0'], ['7890', '7890']]) assert.ok(validateRemotePorts(values));
});

test('终端命令使用目标端口和 IPv6 括号，拒绝命令注入字符', () => {
  assert.equal(terminalProxyCommand('2001:db8::1', 8080, 1080), '$env:HTTP_PROXY="http://[2001:db8::1]:8080"; $env:HTTPS_PROXY=$env:HTTP_PROXY; $env:ALL_PROXY="socks5://[2001:db8::1]:1080"');
  assert.ok(terminalProxyCommand('router.local', undefined, 1080).includes('$env:HTTP_PROXY=""'));
  for (const host of ['router";bad', 'router$env', 'https://router', 'router`bad', 'router;bad']) assert.equal(terminalProxyCommand(host, 7890, 7890), '');
  assert.equal(terminalProxyCommand('router.local', 0, 0), '');
});

test('本机独立代理端口支持mixed关闭和低端口，控制器仍需独立非特权端口', () => {
  assert.equal(validateLocalPorts(['80', '1080', '0', '0', '0'], '9090'), null);
  assert.equal(validateLocalPorts(['0', '0', '0', '0', '0'], '9090'), null);
  assert.ok(validateLocalPorts(['9090', '0', '0', '0', '0'], '9090'));
  assert.ok(validateLocalPorts(['80', '0', '0', '0', '0'], '80'));
  assert.ok(validateLocalPorts(['80', '80', '0', '0', '0'], '9090'));
});
