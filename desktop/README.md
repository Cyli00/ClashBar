# ClashBar Rust + Tauri 客户端

`desktop/` 是唯一维护的客户端，使用 Rust、Tauri 2、TypeScript 与独立 mihomo 内核。发布目标为 Windows 10/11 x64；macOS/Linux 系统集成与安装包尚未验证。

## 开发与打包

需要 Node.js 22+、Rust MSVC 工具链、Visual Studio C++ Build Tools、Windows SDK 和 WebView2 Runtime。环境安装见 [Tauri Windows prerequisites](https://v2.tauri.app/start/prerequisites/#windows)。

```bash
cd desktop
npm ci
npm run tauri dev
```

```bash
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features
npm run tauri build -- --bundles nsis
```

安装器输出到 `src-tauri/target/release/bundle/nsis/`；Windows Desktop 工作流会构建并上传产物。默认构建不含内核；可先运行 `npm run core:bundle -- <可信的 mihomo.exe 或 mihomo.gz 路径>`，再构建含内核安装包。该命令验证 Windows x64 PE、大小和 SHA-256，只准备资源，不下载或执行内核。具体二进制与校验清单不提交 Git。此构建尚未代码签名。

## 首次使用

1. 含内核构建会自动准备应用数据目录中的 `core/mihomo.exe`；已有手选内核路径或托管内核不会被覆盖。无内核构建需从 [MetaCubeX/mihomo Releases](https://github.com/MetaCubeX/mihomo/releases) 获取 Windows x64 内核，解压到固定目录。
2. 点击系统托盘中的 ClashBar 图标。需要自行选择或替换内核时，在「设置」中选择可信的 `mihomo.exe`；应用随后会执行它进行配置验证和启动。
3. 导入本地 YAML 或 HTTP(S) 订阅。首次启动会提供默认配置；它不包含你的订阅节点。
4. 点击启动。应用先执行 `mihomo -t`，确认控制器就绪后显示运行中。
5. 选择规则 / 全局 / 直连和代理节点，测试延迟，再开启系统代理或 TUN。
6. 再次点击托盘图标或按 Esc 收起面板；固定后点击外部保持显示。退出时恢复原系统代理并停止本应用启动的内核。

## 配置与订阅

配置菜单支持导入、切换、重载、删除、单项和批量订阅刷新。订阅可指定文件名，默认每 6 小时自动更新，最小间隔 1 小时；失败同样推进检查时间，避免每分钟重复请求失败地址。链接在 Rust 数据目录中保存，复制时通过后端剪贴板操作，普通状态数据只包含来源主机名。

同名导入会先确认覆盖，保留配置 ID、当前选择和 Wi-Fi 绑定；本地文件覆盖会解除原订阅来源。新增配置保留当前选择，仅没有活动配置时自动选中。可在配置菜单中定位当前配置文件。

下载接受 HTTP/HTTPS、私有网络地址和重定向；最多 10 次重定向、30 秒和 8 MiB。每次更新先校验配置；活动配置切换失败会尝试恢复原内核与代理状态。删除移入 `deleted-profiles/`，不会永久擦除配置。配置目录外部增删改会触发重新核对，活动配置变更通过相同验证与恢复流程处理。

文件型 Provider 会从导入配置所在目录复制到专用缓存；相对资源不能越出该目录。源配置保留，运行副本在 `runtime/` 中生成。运行时控制器固定在 loopback 并使用临时密钥；代理端口、TUN、LAN、IPv6、TCP 并发与日志级别按应用已保存的设置覆盖。

## 主要功能

| 功能 | 当前行为 |
| --- | --- |
| 紧凑面板 | 360 逻辑像素，固定头尾，5 个标签，独立侧边菜单，深浅色与中英语言 |
| 内核与启动 | 选择、验证、启动、停止、重启；登录启动和内核自启分别设置 |
| 代理与规则 | 模式、节点选择、单组/全部测速、历史、Provider 刷新、规则搜索与分组 |
| 后台 Provider | 启动、重启、配置切换后刷新代理/规则提供者；单项失败后继续 |
| 监控 | traffic、memory、connections WebSocket，断流重连；HTTP 快照兼容回退 |
| 日志 | 时间戳和来源、过滤、复制、清空；应用动作日志每 10 MiB 轮换，保留 5 个备份 |
| 组图标 | Rust 下载、2 MiB 上限、7 天磁盘缓存，前端仅接收 data URI |
| 系统代理 | 当前用户 WinINet 代理、绕过列表；原设置快照、所有权检查、退出恢复 |
| TUN | Windows UAC 提权助手启动内核；system/gvisor/mixed/mips 协议栈，mips 需要 mihomo 1.19.31+ |
| Wi-Fi | 原生 WLAN 查询、权限状态、SSID 绑定和自动切换本机配置 |
| 断网恢复 | 原生网卡状态检测；断网暂停原本运行的本机内核，恢复后重启；手动停止或改选配置取消恢复 |
| 远程机器 | HTTP(S)、IPv4/IPv6、增删改、探测、目标切换；远程状态与本机进程隔离 |
| 维护 | DNS/FakeIP 清理、Geo 更新、内核升级、客户端版本检查与发布页 |
| 端口与命令 | HTTP/SOCKS/混合/重定向/TProxy 编辑；本地、LAN、当前远程端点 PowerShell 代理命令 |

客户端更新沿用原版的「检查版本并打开发布页」，不自动安装。发布页指向 [Cyli00/ClashBar](https://github.com/Cyli00/ClashBar/releases)。

## Windows 行为

系统代理修改当前用户 WinINet/LAN 设置，不修改机器级 WinHTTP 或每个应用自己的代理。其它客户端接管后，ClashBar 不覆盖其新设置。崩溃恢复依靠代理快照和 Windows Job Object；强制结束后可重新打开应用恢复快照。

TUN 使用 UAC 提权助手，不安装 macOS helper、不设置 setuid。拒绝授权会返回失败；只有控制器实际返回请求状态后才显示启用。重定向和 TProxy 的可用性由所选内核与运行平台决定，填写端口不表示 Windows 内核支持相应透明代理协议。

选择远程机器不停止本机内核。远程控制写入所选控制器；系统代理开关仍修改这台 Windows 机器，可指向远程代理端口。本机文件管理与进程启停在远程目标下受限。密钥不回传 WebView。

数据位于 Tauri 当前用户应用数据目录（标识 `io.github.cyli00.clashbar`），包含 `settings.json`、`profiles/`、`runtime/`、`logs/`、`icons/` 与 `deleted-profiles/`。配置与保存的订阅可能含凭据，分享排障材料前请脱敏。

## 验证边界

[PARITY.md](PARITY.md) 记录原版功能映射；[VALIDATION.md](VALIDATION.md) 记录原生验收步骤。Rust 本地测试验证控制器、WebSocket、配置、日志、进程与恢复策略；浏览器测试使用模拟 IPC。它们不代替真实 Windows UAC、无线切换、系统代理流量、混合 DPI 和多显示器交互验收。
