---
version: alpha
name: ClashBar Windows
description: The original ClashBar tray popup, adapted to Windows with Rust and Tauri.
colors:
  primary: "#4779b9"
  canvas: "#e9e9ee"
  surface: "#ededf1"
  control: "#e3e3e8"
  ink: "#262627"
  muted: "#78787c"
  tertiary: "#a7a7ac"
  border: "#d9d9df"
  hover: "#d9dfe8"
  selected: "#dfe5ed"
  accent: "#4779b9"
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

Reproduce the original ClashBar menu-bar popup: its compact silhouette, logo, control order, five tabs, colored utility icons, dense rows and adjacent menus. The user's explicit source-fidelity direction supersedes the earlier wide Windows dashboard design. This is a tray utility for managing a local mihomo core, not a new visual identity.

### Product context and register

This product tool serves existing ClashBar users moving to Windows. The reference is `Sources/ClashBar/Views/MenuBar`, `Core/UI/MenuBarLayoutTokens.swift`, and the Chinese localization. Preserve `节点 / 分流 / 连接 / 日志 / 设置` and the source's information hierarchy. Windows lifecycle and capabilities come from Rust; a visible macOS feature is not evidence of Windows support.

Runtime tokens in `src/style.css` are canonical; this frontmatter mirrors the light palette and geometry. There is no token generator or separate theme adapter. Map `canvas → --panel`, `primary/accent → --accent`, `success → --green`, `warning → --orange`; other color names map to same-named variables. Radius maps to `--control-radius` and `--panel-radius`; menu radius and spacing live in shared component declarations. Update the document and runtime owner together.

## Colors

The light popup uses a pale gray panel, closely related control surfaces and restrained blue selection. Purple, teal, green, orange and blue identify familiar utility functions, provider information and traffic direction. Upload is blue; download is green. Pair state tones with labels, icons or accessible names.

`外观模式` offers `跟随系统 / 浅色 / 深色` and persists the preference. Explicit choices override the OS. Dark tokens begin with panel `#28292b`, surface `#2d2e30`, ink `#e1e1e4`, border `#393a3d` and accent `#8ab9e7`, without changing geometry. Both native windows must share appearance. Global scrollbar thumb/track/hover/active tokens cover every owned scroller; forced colors use system colors.

## Typography

Use the system monospace stack above throughout, with Microsoft YaHei UI as the Chinese fallback. The scale is 15px for the product name, 13px for tabs and prominent row labels, 11px for settings/menu/connection labels, 10px for captions and logs, and 9px for secondary metadata. Tiny 8px tags are supplementary only. Base line height is 1.35; logs use 1.4. Measurements use tabular numerals. No remote fonts are loaded.

Keep configuration/node names and identifiers verbatim. Compact rows may truncate, but accessible names, opened menus or copy actions must retain full values. Paths and logs wrap. Owned action labels and feedback use Simplified Chinese; numbers use local formatting and the existing compact byte display.

## Layout

The main native window is a 360 logical-pixel, undecorated tray popup with 8px horizontal inset, 10px outer radius and 0.65px hairlines. The native host positions it against the monitor work area. The shell is capped to the available viewport. Header, modes, tabs and footer stay fixed while `.content-scroll` owns vertical scrolling for all tabs, including long settings. Rules/connections/logs fill the available height; nodes/settings request natural content height subject to the same work-area cap.

The 56px header contains the 40px original logo, name, controller/status line, pin, restart, start/stop and quit. A 40px segmented control shows `规则 / 全局 / 直连`; tabs follow. The footer keeps core version/local picker left and app version right. Native resizing must not push this chrome out of view.

| Surface | Composition |
|---|---|
| 节点 | Real traffic strip; `切换配置`, `系统代理`, disabled `TUN 模式`, terminal command; `代理提供者`, then compact `代理组` rows and adjacent node menus. |
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

The subscription dialog uses `showModal()`, inert background, explicit labels, contained focus and restoration. System-proxy toggling, connection closure and log clearing are direct source-style actions, without added confirmation dialogs. Ports autosave after 750ms idle with Enter to commit/retry; no Save button changes the source row layout. Log source/severity chips permit multiple selections and there is no pause control. One alert banner holds persistent errors; one polite status region announces actions. Unsupported source features are disabled with a concrete explanation. Missing telemetry is `—`, never fabricated data or an enabled no-op. The footer's download-looking icon is labelled as a local executable picker, not an updater.

Traffic charts use observed total-byte deltas only. Quota bars appear only when supplied by the core. No decorative charts, remote images or remote icon packs are loaded. Reduced-motion preferences suppress cosmetic animation.

## Do's and Don'ts

- Preserve the original tray hierarchy and adjacent-menu interaction.
- Keep header/footer, port drafts and active menu anchors stable during refresh.
- Distinguish stopped, loading, empty, no-match, unsupported and explicitly stale states.
- Tie lifecycle, proxy changes and connection interruption to backend acknowledgements.
- Do not imply full macOS parity or make browser preview appear able to manage Windows.
