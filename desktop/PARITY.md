# 原版功能迁移对照

唯一维护的客户端位于 `desktop/`。本表将原版行为映射到 Rust + Tauri；「已实现」表示有对应代码和指定验证，不代表已经完成所有硬件、权限和窗口原生验收。

原版基线为 Git 提交 `62e81b1a074badd63530ffbfc125a4c6c2188416` 中的 `Sources/ClashBar/`。通过 `git show <提交>:<路径>` 可核对，无需保留 Swift 构建树。视觉基线为 `docs/public/clashbar-light.png`、`clashbar-black.png` 及已迁移的 [DESIGN.md](DESIGN.md)。

## 窗口与视图

| 原版行为 | 新实现 / 证据 |
| --- | --- |
| 菜单栏单击开合、后台常驻 | Tauri 托盘开合；无任务栏主窗口；第二实例打开现有面板 |
| 360 宽、8 内边距、10 圆角，固定头尾 | `style.css`、`popup.rs`；逻辑像素随 DPI 缩放 |
| 外部点击关闭、固定后保持、Esc关闭 | `popup.rs` 原生状态机和焦点测试 |
| 左右附属菜单与屏幕边界回退 | 独立子 WebView，碰撞定位和焦点恢复 |
| 节点 / 分流 / 连接 / 日志 / 设置 | 同样的五标签、顺序与紧凑布局 |
| 三种模式、代理组、提供者、流量曲线 | `main.ts`，实际控制器数据和有界列表 |
| 深浅色、中英语言、键盘命令 | 前端语言与主题状态、控件快捷键；文本输入期间保留编辑语义 |
| 图标/速度/图标与速度 | Windows 托盘位图和实际遥测速率；通知区域不能提供 macOS 可变宽状态项 |

## 服务与操作

| 原版入口或服务 | 新实现 |
| --- | --- |
| 本机内核启停、重启、验证 | `engine.rs`、`process.rs`，就绪检查和自有进程生命周期 |
| 可选随包 mihomo / gzip 内核 | `bundled_core.rs`、`core:bundle`，校验后首次复制到托管目录，不覆盖已有选择 |
| 登录启动、内核自启 | Tauri autostart 与独立保存的内核自启设置 |
| 默认配置与本地导入 | `engine/services.rs`，保留默认 YAML；独立源配置和运行副本 |
| 配置切换、删除、外部增删改 | `engine/services.rs` 的目录监测；候选校验、失败恢复、删除归档 |
| 同名导入覆盖确认、当前配置定位 | 两阶段导入；覆盖保留 ID 与 SSID 绑定，新增保留当前选择；资源管理器选中文件 |
| 订阅命名、来源复制、手动/批量/周期刷新 | `subscriptions.rs`、`subscription.rs`；默认6小时、最小1小时、失败推进检查时间 |
| 文件型 Provider | 导入同目录资源并生成隔离缓存路径 |
| 远程机器增删改、探测、切换 | `remote.rs`、控制器目标隔离；HTTP(S)/IPv4/IPv6、密钥留在后端 |
| LAN/IPv6/TCP/日志级别、5类代理端口 | `config.rs`、`engine/local.rs`、控制器patch与持久设置 |
| 系统代理与绕过列表 | WinINet快照/所有权/恢复；Windows原生用户代理，不依赖macOS helper |
| 启动后恢复已启用代理 | `desiredSystemProxy` 独立保存意图；退出恢复 OS，下一次内核就绪再应用；显式关闭取消意图 |
| TUN与协议栈 | UAC助手与控制器状态确认；mips版本门槛1.19.31 |
| SSID绑定、权限提示、自动配置切换 | `ssid.rs` 和 Windows WLAN适配；删除配置同步清理绑定 |
| 断网暂停、恢复后启动原配置 | `network.rs`、`engine/background.rs`；人工停止/改选取消恢复 |
| 流量、内存、连接、远程日志实时流 | WebSocket Text/Binary、Bearer鉴权、ping/pong、重连和HTTP兼容 |
| 启动/重启/切换后的Provider刷新 | `providers.rs` 后台更新两类资源，报告完成数与失败数 |
| 自定义代理组图标 | `group_icons.rs` 有界下载、静态SVG校验、7天磁盘缓存 |
| 日志检索、来源/级别、复制、清空 | 前端过滤；`app_logs.rs` 磁盘轮换与重启读取 |
| 连接排序、单条/全部关闭、复制 | 当前控制器连接操作与剪贴板 |
| WebUI、DNS/FakeIP缓存、Geo更新 | `controller.rs` 的命名操作与受限浏览器打开 |
| 内核更新 | `/upgrade`，区分成功/已经最新/失败 |
| 客户端更新 | fork发布检查与打开发布页；原版也不含自动安装器 |
| 本机/LAN/远程终端命令 | 根据真实地址与代理端口生成PowerShell会话命令 |

## 验证覆盖

Rust服务测试覆盖本地真实WebSocket握手、header鉴权、ping/pong、Text/Binary日志、3路遥测与取消、HTTP回退、订阅限制与脱敏、版本检查、日志轮换、图标缓存、Provider部分失败，以及使用测试内核的断网恢复和人工取消。

`src-tauri/tests/real_core.rs` 使用官方mihomo验证本地HTTP代理转发、流量/内存、重启后mode/log-level持久设置和停止后端口释放。原生网络状态的真实Wi-Fi断开、实际UAC授权、WinINet代理回合与混合DPI操作未由这些测试证明。

## 平台差异和验收

- Windows没有AppKit视觉材质、Apple字体与SF Symbols；保留几何和控件语义，使用等价图形。macOS原图仅作为布局参考。
- 系统代理采用WinINet绕过规则；不能把macOS的所有例外语法等同为Windows行为。
- Windows UAC助手替代root/setuid/XPC；实际授权提示、用户取消和父进程异常退出需在Windows桌面验证。
- 透明重定向/TProxy是否可用取决于内核与平台，端口编辑不构成功能可用性证明。
- macOS/Linux安装包与系统适配尚未验证。

完整原生验收步骤见 [VALIDATION.md](VALIDATION.md)。测试与文档不能替代这些平台验收。
