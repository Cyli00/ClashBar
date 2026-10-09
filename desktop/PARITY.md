# SwiftUI / Windows popup parity

This revision continues the existing Windows migration PR. The design reference is the repository's own SwiftUI/AppKit implementation, not a new dashboard design. “Replica” describes the intended layout and supported interaction patterns; it does not assert complete feature equivalence or pixel-identical macOS rendering.

## Authoritative references

- `Sources/ClashBar/Core/UI/MenuBarLayoutTokens.swift`, `AppFonts.swift` and `AppMaterialStyle.swift`.
- `Sources/ClashBar/Views/StatusBar/StatusItemController.swift`, `Core/Utils/PanelGeometry.swift`, and `ViewModels/PopoverLayoutModel.swift`.
- `Sources/ClashBar/Views/MenuBar/Root/MenuBarRootView.swift`, `MenuBarRoot+Layout.swift`, `MenuBarRoot+ModeTabs.swift` and the five tab views.
- `Sources/ClashBar/Core/UI/Components/AttachedPopoverMenu.swift`.
- `docs/public/clashbar-light.png` and `clashbar-black.png` (2× screenshots; the actual panel is 360 points wide).

## Layout and window behavior

| Original | Windows implementation / acceptance target |
| --- | --- |
| Accessory application without normal windows | Hidden startup; frameless tool window, no taskbar/Alt-Tab entry; tray is the normal entry point |
| Left-click toggles the panel | Left-click toggles; second instance opens the existing panel |
| Width 360; inner margin 8; radius 10 | Same logical pixel values; DPI scaling is owned by the native window |
| Minimum height 280; initial 320; work-area maximum | Content-driven popup height, bounded by the tray monitor's work area |
| Main panel anchored under the menu bar | Anchored beside the Windows tray rectangle, adapting to taskbar edges, monitor origin and scale |
| Outside click closes unless pinned | Native focus/owned-window state handles dismissal; explicit tray toggle and Escape still close |
| Attached menus are separate side panels | Separate owned WebView, right/left collision placement (overlap fallback on very narrow work areas), rather than an oversized invisible main window |
| Header and footer remain fixed while body scrolls | Same structure, with source-derived compact spacing |
| 40px logo, monospaced title and endpoint, pin/restart/stop/quit | Original logo asset and matching control order; equivalent vector icons |
| Rule / Global / Direct icon-over-label row (38px) | Same arrangement, with authoritative mode state and pending feedback |
| 节点 / 分流 / 连接 / 日志 / 设置 (26px) | Same labels/order, selected underline and retained active tab |
| MIHOMO version left, app version right | Same compact footer; version reflects this Windows build |

The native dialog guard keeps file selection from accidentally dismissing the parent. Delayed blur decisions are invalidated by new focus, tray intent or dialogs, preventing a tray click from closing and immediately reopening the popup.

## Supported workflows

| Workflow | Behavior |
| --- | --- |
| Core controls | Select trusted executable, start, stop, restart, and quit with system-proxy restoration |
| Config menu | Import local YAML or HTTPS subscription; retain and select previous profiles; validate before live switch and recover the previous profile after failed activation |
| System proxy | Compact toggle backed by the existing current-user WinINet transaction and recovery journal |
| Proxy groups | Compact group rows and authored node menu; selection and latency testing are separate actions |
| Providers | Collapse/expand, update, node counts and supplied subscription metadata |
| Traffic and memory | Real cumulative counters sampled over elapsed time; memory from the core; missing values are not fabricated |
| Rules | Search/filter and compact grouping rather than a wide paginated dashboard table |
| Connections | Search/filter/sort, individual close and close-all controls |
| Logs | Bounded core output, multi-select filtering, direct clearing and core log-level controls |
| Ports | Validated mixed/controller ports; 750ms autosave/Enter retry; changes require a stopped core |

## Explicit remaining differences

- **Platform appearance:** WebView2 cannot reproduce AppKit vibrancy, Apple fonts or SF Symbols byte-for-byte. The port reuses the original logo and geometry, follows light/dark appearance, and uses available monospaced fonts/equivalent icons. Native undecorated-window shadows are disabled because Tao reserves hidden non-client insets for them; both the HWND and usable content must remain 360 logical pixels wide. OS-specific text metrics and material effects still need visual acceptance on Windows.
- **Tray presentation:** Windows has a notification-area icon, not a macOS variable-width menu-bar item. Two-line traffic text in the tray is not reproduced; traffic remains in the panel.
- **Telemetry:** the original streams traffic/connections/logs. This port samples controller totals while visible and reads bounded process output; its chart timing and log content are not identical. Captured log entries carry real timestamps and source labels, and are merged chronologically.
- **Unmigrated services:** TUN/privileged helper, LAN listener exposure, remote machine management, SSID policy, scheduled subscription updates, autostart, automatic core/client updates and language switching remain unavailable. Associated controls must be clearly unavailable rather than report success.
- **Rule providers:** proxy-provider management is implemented, but rule-provider inventory/update and per-rule statistics are not yet available.
- **Settings coverage:** the port manages a mixed proxy port and a loopback controller. The original independent HTTP/SOCKS/redirect/tproxy listeners, helper maintenance and macOS-specific settings have no equivalent implementation in this revision.
- **Configuration management:** profile selection/import is implemented. Full original profile editing/deletion, saved subscription-source management and refresh scheduling remain follow-up work. Subscription URLs are not persisted.
- **Safety and recovery feedback:** unsupported configuration forms, validation failures and proxy ownership changes remain explicit, even where this requires additional feedback compared with the original.

## Evidence and limits

Rust tests cover geometry, popup event ordering, controller requests, profile migration/switching and process/proxy recovery. Browser tests cover the compact UI with mocked IPC and produce light/dark screenshots. The Windows workflow builds the actual installer and runs a native executable popup smoke, with a screenshot when the runner desktop permits capture.

These checks do not prove every tray gesture, mixed-DPI monitor transition, native picker focus sequence or real mihomo network flow. The remaining interactive acceptance sequence is in [VALIDATION.md](VALIDATION.md). Keep the PR in Draft until these platform checks and the desired feature scope are accepted.
