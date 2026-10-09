<div align="center">
<img src="./docs/public/clashbar-logo.png" width="160" alt="ClashBar Logo" />

# ClashBar

**Rust + Tauri 2 托盘客户端 · mihomo**

[开发与使用](desktop/README.md) · [架构](desktop/ARCHITECTURE.md) · [迁移对照](desktop/PARITY.md) · [验证](desktop/VALIDATION.md)
</div>

本仓库只维护 `desktop/` 中的 Rust + Tauri 客户端。SwiftUI、AppKit、Swift Package、macOS Helper 和原 DMG 构建流程已移除。原实现保留在 Git 历史 `62e81b1` 中，作为功能和设计的核对依据。

界面沿用原版 360px 菜单面板：托盘开关、固定面板、独立侧边菜单，以及节点、分流、连接、日志、设置五个标签。Rust 负责内核生命周期、配置与订阅、远程控制器、系统代理、TUN 权限、SSID 策略和实时监控；TypeScript 负责界面及中英切换。代理引擎继续使用独立的 [mihomo](https://github.com/MetaCubeX/mihomo)。

当前安装目标为 Windows 10/11 x64。macOS/Linux 的 Rust 平台集成与安装包尚未验证，不能将移除旧端理解为已经发布新的 macOS/Linux 客户端。功能对应关系、平台差异和验证范围见[迁移对照](desktop/PARITY.md)。

## 开发与构建

需要 Node.js 22+、Rust MSVC 工具链、Visual Studio C++ Build Tools、Windows SDK 和 WebView2。

```bash
cd desktop
npm ci
npm run tauri dev
```

在应用中选择可信的 mihomo 可执行文件，导入配置或订阅，再启动内核。TUN 首次启动会请求系统权限。系统代理在退出时恢复原设置，配置和凭据只保存在本机应用数据目录。

```bash
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features
npm run test:e2e
npm run tauri build -- --bundles nsis -- --locked
```

安装包位于 `desktop/src-tauri/target/release/bundle/nsis/`。GitHub Actions 执行前端、Rust、安装包及原生弹窗检查。默认构建不包含 mihomo；也可按开发文档准备可信内核，生成随包版本。当前未配置代码签名。

## 项目结构

| 路径 | 用途 |
| --- | --- |
| `desktop/src/` | Tauri 界面、语言、菜单与交互 |
| `desktop/src-tauri/` | Rust 服务、系统集成、内核进程与资源 |
| `desktop/tests/` | 前端单元、浏览器与原生弹窗检查 |
| `docs/` | 文档站及原版界面参考图片 |

感谢 [Sitoi/ClashBar](https://github.com/Sitoi/ClashBar) 原项目和 [MetaCubeX/mihomo](https://github.com/MetaCubeX/mihomo) 内核。保留上游 [LICENSE](LICENSE) 与署名。
