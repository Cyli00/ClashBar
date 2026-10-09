# ClashBar for Windows

Rust + Tauri 2 的 Windows 客户端，沿用独立 mihomo 内核。当前为迁移预览版，目标平台为 Windows 10/11 x64。SwiftUI/AppKit 源码保留在仓库中作为 macOS 实现与迁移参照，Windows 构建不依赖 Xcode 或 Swift。

## 开发与打包

安装 [Tauri Windows 构建依赖](https://v2.tauri.app/start/prerequisites/#windows)：Rust MSVC 工具链、Visual Studio C++ Build Tools、Windows SDK、WebView2 Runtime，以及 Node.js 22+。

```powershell
cd desktop
npm ci
npm run tauri dev
```

```powershell
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features
npm run tauri build -- --bundles nsis
```

安装器位于 `src-tauri/target/release/bundle/nsis/`。GitHub Actions 的 `Windows Desktop` 工作流会执行检查、构建并上传安装器；此预览版没有代码签名，未发布自动更新渠道。安装包不包含 mihomo，避免隐式下载或执行未知内核。

## 首次使用

1. 从 [MetaCubeX/mihomo 官方 Releases](https://github.com/MetaCubeX/mihomo/releases) 下载适合 Windows x64 的内核，解压至固定目录。
2. 启动 ClashBar，点击系统托盘中的图标展开面板，在「设置」中选择 `mihomo.exe`。只选择自己信任的内核；选择后应用会在验证和启动时执行它。
3. 导入本地 YAML 配置或 HTTPS 订阅，按需设置 mixed-port 与控制端口。
4. 点击启动。ClashBar 会先生成运行配置并执行 `mihomo -t`，API 就绪后才显示运行中。
5. 选择 Rule / Global / Direct 和节点，确认配置可用后开启系统代理。
6. 再次点击托盘图标或按 Esc 收起面板；未固定时点击外部也会收起。顶部固定按钮保留面板，退出按钮会恢复代理设置并停止自有内核。

订阅导入是一次性获取，重新导入可更新配置。订阅须为公共地址上的 HTTPS 443 URL，不跟随重定向；需要时填写最终地址。导入的配置保存在配置库中，通过「切换配置」侧边菜单选择。运行中切换会先用 mihomo 验证候选配置，启动成功后才保存新的活动配置；失败时尝试恢复原配置与代理状态。旧预览版的单配置槽会自动迁移。更换内核与端口前仍需停止内核。文件型 Provider 与入站代理认证暂不支持，导入时会明确拒绝。导入文件保留为源配置，应用为运行生成独立副本，原始用户文件不会被修改。

## Windows 行为与边界

- 控制器固定在 `127.0.0.1`，使用每次运行生成的 secret；前端不能读取 secret，也没有任意 shell 或通用 HTTP 转发命令。
- 此版本管理本地显式代理：运行配置覆盖控制器、监听地址和代理端口，关闭 TUN、LAN 暴露及额外入站监听。需要这些能力的配置不代表在此版本中已获支持。
- 系统代理作用于当前用户默认 WinINet/LAN 配置。它不修改机器级 WinHTTP、命名拨号/VPN 连接，也不能保证所有应用遵守代理设置。
- 修改系统代理前保存原设置，停用时先检查所有权。其他客户端已接管时不会覆盖它的新设置；旧快照会归档，下次显式启用将以当前设置为基线。
- 内核异常退出会尝试恢复代理。应用被强制终止或系统崩溃后，Windows Job Object 负责清理自有子进程，代理快照在下次启动时恢复。若仍无法联网，请先重新打开 ClashBar，必要时在 Windows 代理设置中检查手动代理/PAC。
- 源配置、订阅内容可能含节点凭据，保存在当前用户应用数据目录。程序没有遥测，不上传配置；请勿把整个数据目录或含凭据日志公开到 Issue。

## 迁移范围

| 能力 | Windows 预览版 |
| --- | --- |
| 本地内核、YAML/HTTPS 导入、配置验证 | 已实现 |
| 模式、代理组选择、节点测速、Provider 更新 | 已实现 |
| 规则与连接查看、单连接/全部连接关闭 | 已实现 |
| 内核输出日志、托盘、系统代理恢复 | 已实现 |
| 多配置归档、运行中切换与失败恢复 | 已实现 |
| 订阅定时更新、远程机器 | 尚未迁移 |
| TUN、管理员服务、Wi-Fi/SSID 策略 | 尚未迁移 |
| 开机启动、内核/客户端自动更新 | 尚未迁移 |
| 流量与内存 | 累计流量差值采样曲线、内核内存；未采用原版 WebSocket 流 |
| 中英切换 | 尚未迁移 |
| Rust 客户端在 macOS/Linux 的发布支持 | 尚未验证；系统代理明确仅支持 Windows |

面板的视觉、交互与已知差异见 [原版对照表](PARITY.md)。选型与后续工作见 [架构决策](ARCHITECTURE.md)。原版 macOS 说明见 [README.macos.md](../README.macos.md)。
