<div align="center">
<img src="./docs/public/clashbar-logo.png" width="160" alt="ClashBar Logo" />

# ClashBar

**Windows 客户端迁移预览 · Rust + Tauri 2 · mihomo**

[Windows 开发与使用](desktop/README.md) · [架构选型](desktop/ARCHITECTURE.md) · [原版 macOS 说明](README.macos.md)
</div>

此 fork 在 `desktop/` 中实现 Windows 托盘代理客户端，将界面与客户端服务迁移到 TypeScript + Rust，继续使用独立的 [mihomo](https://github.com/MetaCubeX/mihomo) 内核。

支持配置导入、内核验证/启动/停止、模式与节点切换、测速、Provider 更新、规则与连接查看，以及 Windows 当前用户系统代理管理。关闭窗口会隐藏到托盘，退出时恢复原代理设置。

**这是分阶段迁移，尚未达到原 macOS 版本的功能等价。** TUN/管理员服务、SSID 策略、远程机器、自动更新、多配置与中英切换尚未迁移。完整范围、配置覆盖行为与崩溃恢复限制见 [Windows README](desktop/README.md)。现有 SwiftUI/AppKit 源码继续保留，Windows 构建不依赖它。

## 快速开始

环境：Windows 10/11 x64、Node.js 22+、Rust MSVC 工具链、Visual Studio C++ Build Tools 与 WebView2。详见 [Tauri Windows prerequisites](https://v2.tauri.app/start/prerequisites/#windows)。

```powershell
cd desktop
npm ci
npm run tauri dev
```

应用内选择可信的 `mihomo.exe`，导入本地 YAML 或 HTTPS 订阅，启动内核后再开启系统代理。安装包不内置内核。

```powershell
npm run tauri build -- --bundles nsis
```

构建产物位于 `desktop/src-tauri/target/release/bundle/nsis/`，也可在本仓库 GitHub Actions 的 `Windows Desktop` 工作流中获取。预览版安装器未签名。

## 项目结构

| 路径 | 用途 |
| --- | --- |
| `desktop/src/` | Windows 客户端界面 |
| `desktop/src-tauri/` | Rust 服务、进程生命周期、Windows 系统集成 |
| `Sources/`、`Package.swift` | 原版 macOS 客户端 |
| `docs/` | 原版文档站 |

感谢 [Sitoi/ClashBar](https://github.com/Sitoi/ClashBar) 原项目和 [MetaCubeX/mihomo](https://github.com/MetaCubeX/mihomo) 内核。保留上游 [LICENSE](LICENSE) 与署名。
