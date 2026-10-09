---
version: alpha
name: ClashBar Windows
description: The original ClashBar tray popup, adapted to Windows with Rust and Tauri.
colors:
  primary: "#37679d"
  canvas: "#e9e9ee"
  surface: "#ededf1"
  control: "#e3e3e8"
  ink: "#262627"
  muted: "#646468"
  tertiary: "#a7a7ac"
  border: "#d9d9df"
  hover: "#d9dfe8"
  selected: "#dfe5ed"
  accent: "#37679d"
  blue: "#007aff"
  success: "#2bc957"
  warning: "#ff8b27"
  purple: "#8b58ff"
  teal: "#00bbce"
  danger: "#eb4656"
typography:
  display:
    fontFamily: 'ui-monospace, "Cascadia Mono", "SFMono-Regular", Consolas, "Microsoft YaHei UI", monospace'
    fontSize: "15px"
  sans:
    fontFamily: 'ui-monospace, "Cascadia Mono", "SFMono-Regular", Consolas, "Microsoft YaHei UI", monospace'
    fontSize: "13px"
    lineHeight: "1.35"
  mono:
    fontFamily: 'ui-monospace, "Cascadia Mono", "SFMono-Regular", Consolas, "Microsoft YaHei UI", monospace'
    fontSize: "10px"
rounded:
  DEFAULT: "6px"
  panel: "10px"
  menu: "8px"
spacing:
  panel-width: "360px"
  panel-inset: "8px"
  row-inset: "4px"
  section-gap: "6px"
  rule-row-height: "32px"
  hairline: "0.65px"
components:
  button: { backgroundColor: "{colors.canvas}", textColor: "{colors.ink}" }
  dialog: { width: "328px", backgroundColor: "{colors.canvas}", textColor: "{colors.ink}" }
  menu: { width: "300px", backgroundColor: "{colors.canvas}", textColor: "{colors.ink}" }
  table: { backgroundColor: "{colors.canvas}", textColor: "{colors.ink}" }
  canvas: { backgroundColor: "{colors.canvas}", textColor: "{colors.ink}" }
  divider: { backgroundColor: "{colors.border}" }
  selected-tab: { textColor: "{colors.ink}" }
  button-hover: { backgroundColor: "{colors.hover}" }
  badge: { backgroundColor: "{colors.control}", textColor: "{colors.muted}" }
  selected-mode: { backgroundColor: "{colors.selected}", textColor: "{colors.accent}" }
  upload-icon: { textColor: "{colors.blue}" }
  running-status: { textColor: "{colors.success}" }
  warning-message: { textColor: "{colors.warning}" }
  proxy-icon: { textColor: "{colors.purple}" }
  memory-icon: { textColor: "{colors.teal}" }
  error-message: { textColor: "{colors.danger}" }
---

# ClashBar Windows Design System

## Overview

### Creative North Star

Reproduce the original ClashBar menu-bar popup: its compact silhouette, logo, control order, five tabs, colored utility icons, dense rows and adjacent menus. The user's explicit source-fidelity direction supersedes the earlier wide Windows dashboard design. This is a tray utility for managing local and remote mihomo cores, not a new visual identity.

### Product context and register

这个工具通过 Rust/Tauri 维护本机与远程 mihomo。保留 `节点 / 分流 / 连接 / 日志 / 设置`、原版紧凑层级与 `docs/public/clashbar-light.png`、`clashbar-black.png` 的布局。Swift 界面仅保留在 Git 历史中作为迁移参照；运行时功能、持久化和系统集成由 Rust 负责。

Runtime tokens in `src/style.css` are canonical; this frontmatter mirrors the light palette and geometry. There is no token generator or separate theme adapter. Map `canvas → --panel`, `primary/accent → --accent`, `success → --green`, `warning → --orange`; other color names map to same-named variables. Radius maps to `--control-radius` and `--panel-radius`; menu radius and spacing live in shared component declarations. Update the document and runtime owner together.

## Colors

The light popup uses a pale gray panel, closely related control surfaces and restrained blue selection. Purple, teal, green, orange and blue identify familiar utility functions, provider information and traffic direction. Upload is blue; download is green. Pair state tones with labels, icons or accessible names.

为保持紧凑小字可读，浅色 muted 调整为 #646468，accent 调整为 #37679d；在对应 badge/selected 背景上的对比度分别为 4.61:1 与 4.62:1。几何、层级与其他语义色保持原版结构。

`外观模式` offers `跟随系统 / 浅色 / 深色` and persists the preference. Explicit choices override the OS. Dark tokens begin with panel `#28292b`, surface `#2d2e30`, ink `#e1e1e4`, border `#393a3d` and accent `#8ab9e7`, without changing geometry. Both native windows must share appearance. Global scrollbar thumb/track/hover/active tokens cover every owned scroller; forced colors use system colors.

## Typography

Use the system monospace stack above throughout, with Microsoft YaHei UI as the Chinese fallback. The scale is 15px for the product name, 13px for tabs and prominent row labels, 11px for settings/menu/connection labels, 10px for captions and logs, and 9px for secondary metadata. Tiny 8px tags are supplementary only. Base line height is 1.35; logs use 1.4. Measurements use tabular numerals. No remote fonts are loaded.

配置、节点名称及标识保持原文。紧凑行可省略，但菜单、可访问名称和复制操作保留完整值；路径和日志换行。`src/i18n.ts` 统一提供简体中文与英语的标签、菜单、校验及反馈；数字和时间使用当前语言与系统时区。英语标签页使用 10px 字号、快捷行标题使用 11px，保持 360px 面板及 320px 预览中的控件顺序。

## Layout

The main native window is a 360 logical-pixel, undecorated tray popup with 8px horizontal inset, 10px outer radius and 0.65px hairlines. The native host positions it against the monitor work area. The shell is capped to the available viewport. Header, modes, tabs and footer stay fixed while `.content-scroll` owns vertical scrolling for all tabs, including long settings. Rules/connections/logs fill the available height; nodes/settings request natural content height subject to the same work-area cap.

The 56px header contains the 40px original logo, name, controller/status line, pin, restart, start/stop and quit. A 40px segmented control shows `规则 / 全局 / 直连`; tabs follow. The footer keeps core version/local picker left and app version right. Native resizing must not push this chrome out of view.

| Surface | Composition |
|---|---|
| 节点 | 实测流量；`切换配置`、`系统代理`、`TUN 模式`（system/gvisor/mixed/mips）、本机与局域网终端命令；`代理提供者`与紧凑`代理组`，相邻节点菜单。 |
| 分流 | Counts and group/refresh controls; type chips; search/policy; `目标 / 类型`, `策略`, `统计` with 32px rows. |
| 连接 | Protocol/sort, fraction and close-all; search; dense host/rule, metrics and chain rows. |
| 日志 | Source/core-level controls, severity/actions, search; icon, metadata and wrapping message. |
| 设置 | `基础设置`, `内核设置`, `代理端口`, `系统维护`; compact label/control rows and paired maintenance buttons. |

Use 4px row insets, 2–6px internal gaps, 14–16px leading icons and 28px settings rows. A narrow browser preview may contract below 360px while preserving control order. Do not restore a wide layout, oversized cards or numbered pagination to the native popup.

## Elevation & Depth

Quiet surfaces and separators define the panel. The OS owns native window elevation. Adjacent menus and dialogs receive subtle shadows; routine rows do not. Menus never reflow the main panel and are not clipped to its viewport.

## Shapes

Use 10px panel/segmented-control corners, 6px shared control corners and 8px menu corners. Capsules carry compact values/counts. Preserve the original logo rather than an invented lettermark. Small icon controls follow the source's pointer-first density with explicit keyboard focus.

## Components

`src/menu.ts` owns authored menu content and behavior. Desktop renders it in a separate native Tauri window beside the trigger; it is not an OS-owned `select` popup. Default geometry is 300px wide and at most 480px high, with a fixed heading and scrolling body. Rust chooses the available side and clamps to the work area. This source-compatible adjacent-menu variant deliberately exceeds the trigger width. Browser fallback uses the same renderer inside its viewport.

Menus expose checked choices, disabled options, type/latency metadata and independent delay-test actions. Proxy-group hover opens after 150ms; leaving starts a 100ms dismissal grace period, cancelled on entry to the menu. Click/keyboard opening requests focus; hover preserves main-window focus. Keep the active trigger stable across refreshes. Shared controls define hover, visible focus, pressed, disabled and busy states.

订阅、配置删除、代理绕过、SSID 管理、应用更新与远程机器对话框复用 `src/ui.ts` 的 `presentDialog`，保持模态背景、明确标题、焦点约束和关闭后恢复。系统代理、断开连接与清空日志保持直接操作。端口输入空闲 750ms 自动保存，Enter 提交或重试。日志来源与级别支持多选；统一错误横幅和状态播报。缺失遥测显示 `—`。页脚内核按钮提供本机选择、运行内核更新和目录入口；应用版本按钮检查发布版本并打开官方发布页。

流量图优先使用内核 WebSocket 的实时速率，缺失时以真实总量差分计算；配额条仅显示内核返回的数据。代理组图标由 Rust 按组名下载、验证并缓存，以 data URI 返回，加载失败保留名称。隐藏名为 default 或 vehicleType 为 Compatible 的提供者。减少动态效果偏好关闭装饰性动画。

远程机器面板由 `src/remote.ts` 和 `.machine-manager` 样式负责，沿用原版 360×500 的目标尺寸，窄窗口保留 8px 外边距。16px 内边距、10px 圆角机器行、左侧设备图标、右侧编辑/删除按钮与选中标识复用现有颜色和字体。顶部在列表显示关闭、编辑时显示返回；列表底部为添加机器，编辑底部为通栏保存。`.machine-body` 独立滚动，标题和底部动作固定，使用全局滚动条。主机/端口同行，密钥默认遮罩，浅深主题保持相同尺寸。主窗口在管理面板打开时请求 540px 高度并停止按背景标签缩小，关闭后恢复内容驱动尺寸。

## Do's and Don'ts

- Preserve the original tray hierarchy and adjacent-menu interaction.
- Keep header/footer, port drafts and active menu anchors stable during refresh.
- Distinguish stopped, loading, empty, no-match, unsupported and explicitly stale states.
- Tie lifecycle, proxy changes and connection interruption to backend acknowledgements.
- Do not imply full macOS parity or make browser preview appear able to manage Windows.
