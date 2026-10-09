import test from 'node:test';
import assert from 'node:assert/strict';
import { localizeBackendError } from '../src/errors.ts';

test('英文错误翻译保留端口、HTTP状态、配置名称和底层原因', () => {
  assert.equal(localizeBackendError('代理 TCP 端口 7890 已被占用，请在设置中更换。', 'en'), 'Proxy TCP port 7890 is in use. Change it in settings.');
  assert.equal(localizeBackendError('控制器返回 HTTP 401。', 'en'), 'Controller returned HTTP 401.');
  assert.equal(localizeBackendError('当前 Wi-Fi 绑定的配置“工作配置.yaml”已不存在。', 'en'), 'The profile “工作配置.yaml” bound to the current Wi-Fi no longer exists.');
  assert.equal(localizeBackendError('保存 TUN 设置失败，已恢复运行状态：Access denied (os error 5)', 'en'), 'Saving TUN settings failed; the previous runtime state was restored: Access denied (os error 5)');
  assert.equal(localizeBackendError('raw core message 中文', 'en'), 'raw core message 中文');
  assert.equal(localizeBackendError('控制端口 9090 已被占用。', 'zh-CN'), '控制端口 9090 已被占用。');
});
