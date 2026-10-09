# UX Contract

## Product context and sources

ClashBar Windows manages one local mihomo core through Rust/Tauri. The explicit user direction selects a faithful original tray popup, superseding the earlier dashboard/native-select design. `DESIGN.md` owns visuals; this contract owns behavior. Locale is Simplified Chinese, technical names remain verbatim and times use the local OS timezone. Accessibility targets keyboard-operable semantics, explicit names/status, visible focus and WCAG 2.2 AA; runtime evidence is required before claiming compliance.

| Domain | Authoritative source |
|---|---|
| Source hierarchy and labels | `Sources/ClashBar/Views/MenuBar`, `Sources/ClashBar/Core/UI/MenuBarLayoutTokens.swift`, Chinese localization |
| Lifecycle, profiles and proxy recovery | `src-tauri/src/engine.rs`, `system_proxy.rs`, commands in `lib.rs` |
| Popup geometry, pin/focus and menus | `src-tauri/src/popup.rs`, `popup_state.rs`, native geometry implementation |
| Current data and IPC | `src/types.ts`, Rust return types, `src/main.ts` |
| Shared visual/UI owners | `DESIGN.md`, `src/style.css`, `src/ui.ts`, `src/menu.ts` |

The source UI is a fidelity reference, not a promise of complete Windows support. Subscription credentials never enter frontend storage or status messages. There are no billing, sharing, authentication or legal workflows.

## Canonical UI Map

| Capability | Canonical owner | Source of truth | Allowed variants | Verification |
|---|---|---|---|---|
| Select/Listbox | `showMenu`, `renderMenu`, `mountSubmenu` in `src/menu.ts` | this contract and native protocol | authored adjacent native menu; same-renderer browser fallback | keyboard, hover bridge, selection, focus, work-area placement |
| Form | `field`, `setFieldError`, `searchField` in `src/ui.ts`; shared validators | this contract and backend validation | subscription / ports / local search | IME, validation and recovery |
| Scrollbar | global `src/style.css` | DESIGN.md | main body / menu body / modal overflow | computed styles and reachability |
| Toast | persistent `notice` live region in `src/main.ts` | this contract | pending / success / information | announcement and duplicate checks |
| CRUD | `mutate`, `refresh`, `ReadEpoch` | Rust commands and this contract | profile import/select, lifecycle, settings, connection close | complete flows and stale-read tests |
| Dialog | subscription form in `src/main.ts`, shared field primitives in `src/ui.ts` | this contract | subscription import | initial focus, trap, Escape, retry and restoration |
| Tabs | `activateTab` and persistent panel map | this contract | five source tabs | roving focus, selection, scroll/draft retention |

There is no native HTML `select` owner. Menu geometry/content are authored even though the surface is a native window. Its width need not match a small trigger. Date entry and row selection are inapplicable. Reuse these owners; unused legacy helpers do not define a competing contract.

## Popup and adjacent-menu protocol

Rust owns the undecorated tray window's monitor-aware placement, work-area clamping, visibility and pin state. The frontend requests desired height through `resize_popup`; header, modes, tabs and footer stay mounted while the body scrolls. `get_popup_pinned`, `set_popup_pinned`, `hide_popup` and `quit_app` own those native actions. Hiding does not stop the core. Pin is session-only and does not block explicit hide/tray toggle. Native outside-focus handling has a 120ms grace period and protects transitions between the owned main/menu windows and native file dialogs. Outside focus/click dismisses an unpinned popup; when pinned it dismisses the menu only.

`show_attached_menu` receives the trigger's logical rectangle, desired dimensions, menu model and focus intent. `get_attached_menu` initializes the child; `menu-data` updates it and `menu-focus` requests focus. Child `attached_menu_action` relays `{menuId, actionId}` as `attached-menu-action` to the main window, where the registered callback owns the operation. `hide_attached_menu` emits `attached-menu-closed`; `attached_menu_hover`/`attached-menu-hover` bridge pointer presence. Reject obsolete menu IDs and disabled/separator actions. Primary actions close the menu; secondary latency tests keep it open.

Proxy-group hover opens after 150ms without stealing parent focus. Leaving starts a 100ms grace period, cancelled when entering the other surface. Click/keyboard opening requests focus. Arrow Up/Down and Home/End move among enabled actions; Enter/Space activates them. Escape closes a menu before the main popup; dialogs retain their own Escape behavior. Native close restores parent/trigger focus when appropriate. Explicit dismiss/replacement must not focus a stale or disconnected anchor. Delay-result updates preserve the active anchor and focus intent. Both windows share appearance and density. Browser fallback is a preview, not proof of native behavior.

`popup-visibility` synchronizes visibility/pin state. Only a hidden-to-visible transition resets searches, rule type/policy, log filters, expanded policies and scroll positions, then reads authoritative state. Repeated visible events, including menu focus transitions, preserve them. `popup-tab` can open settings. Port drafts survive tab switches and ordinary hiding. Opening a panel never triggers a mutation.

## Dataset navigation and preferences

Mihomo returns complete snapshots. This popup has no numbered paging or shareable route/query URL. Sensitive searches and connection names remain in memory.

| Dataset | Filtering and rendering contract |
|---|---|
| Rules | Retain all returned rules. Search/policy precede type-chip counts and the selected type. Each virtual list renders at most 50 rows with 32px geometry and overscan; 50 is a rendering window, not a data cap. Policy groups use the same lazy rows and count-descending/name ordering. |
| Connections | Cap to the first 120 returned connections before local search, transport filtering and sorting. Badge denominator is `min(total, 120)`. Close-all affects all core connections, including undisplayed ones. |
| Logs | Typed backend entries and local action records merge by timestamp in chronological order into a combined buffer capped at `LOG_LIMIT = 500`. Individual raw app/core buffers are also bounded to 500. Display the newest 120 combined entries before source/severity/search filtering. Badge uses the combined retained total; copy-all copies that combined buffer, not only visible rows. |
| Traffic | Rates derive from successive real byte totals; retain at most 60 samples. Missing rate/memory/history displays `—` or an empty chart. Clear samples when stopped; never synthesize history. |

Search is local and IME-aware; clear acts immediately and returns input focus. Filtering must not strand the virtual view beyond accessible results. Rules distinguish `暂无规则数据`/`无匹配规则`; other tabs likewise distinguish empty, no-match, stopped and loading states. Rule `统计` remains `—` without real rule-provider data. `规则集` currently counts RuleSet routing entries; refresh re-reads rules, not rule-provider contents.

Persist only non-sensitive preferences: provider collapse, latency sort/history, hidden-group visibility, rule grouping, connection transport/sort and appearance. Queries, rule policy/type, log filters and expanded policies remain transient. Per-tab scroll is retained during an open session. Appearance offers `跟随系统 / 浅色 / 深色`; fixed Simplified Chinese language remains a disabled source setting.

## Available features and truthful affordances

| Area | Available behavior | Unavailable behavior |
|---|---|---|
| Core/profiles | Trusted local executable; YAML/HTTPS import; retained profile list/select; start/stop/restart; mixed/controller ports; version | automatic download/upgrade, remote machines, WebUI, login/core autostart |
| Routing | rule/global/direct modes; node selection; node/group delays; proxy-provider refresh and returned quota metadata | TUN and rule-provider update/statistics |
| System | Windows system-proxy enable/restore; PowerShell command copy; pin and appearance | bypass editor, LAN/IPv6/TCP toggles, separate HTTP/SOCKS/redirection/TProxy ports |
| Connections/logs | individual/all close; protocol/sort/search; log source/severity/search, copy/clear; runtime log level | pause control, persistent log history or remote-controller state |
| Maintenance | local executable picker in footer/settings | FakeIP/DNS clearing, Geo update, open-core-directory |

Unsupported source controls are actually disabled and explain why; never enable no-ops. Unknown values stay unknown. Browser-only mode disables desktop mutations and explains the desktop requirement. Application log entries represent real app actions; `get_log_entries` supplies typed backend `{timestamp, source, message}` records. Source and severity filters are independent multi-select Sets; an empty Set means all and `全部` clears that dimension. Core log level is runtime-only and may reset on restart; both selectors read the same snapshot. Successful log clearing does not append its own success record, so the cleared view stays empty until a genuinely new entry arrives.

## Flow ledger

| Operation | Pending | Success | Failure and focus |
|---|---|---|---|
| Core picker / ports | stopped core required; ports autosave after 750ms idle and submit/retry on Enter | returned status; retain current tab; no Save button | preserve draft; inline validation; Enter retries a failed save |
| Profile import / select | serialize; backend owns any live-core transition | retain profiles and acknowledge active profile | retain recoverable input/authoritative state; explain failure |
| Start / stop / restart / proxy | busy controls; pessimistic state | returned status and re-read data | no unacknowledged running/proxy claim |
| Node / mode / provider / log level | disable mutation controls | refresh same tab; both level selectors agree | error and last authoritative choice |
| Single/all connection close | direct source-style action; serialize mutation | refresh connections in the same tab | persistent error; retry through the action |
| Clear logs | direct source-style action; serialize mutation | backend and combined buffers empty; no self-log | persistent error; retain data where possible and retry |
| Search / preferences | local operation | same tab/focus, updated view | no backend success implied |

Imports add to the profile library rather than replacing it; remove the former blanket replacement warning. No rename/delete or retained subscription-refresh URL is implied. Backend validates a live profile change, manages restart/proxy preservation and attempts recovery on failure. Core replacement and port edits remain unavailable while running.

## Navigation, overlays and feedback

Tabs expose selection, associated panels, Arrow Left/Right and Home/End roving focus. Titles include the active tab. Settings/port inputs stay mounted. `.content-scroll` is the only main-panel vertical scroller, keeping all settings reachable in short windows while header/footer stay fixed. Menus have bounded body scrolling; long modals scroll within the viewport.

The user's explicit source-fidelity instruction selects direct actions for system-proxy toggling, individual/all connection closure and log clearing: no confirmation dialog is inserted. Mutations remain serial, error-aware and backend-acknowledged. Subscription import uses `dialog.showModal()` with app-authored content, an accessible title, inert background, contained focus, idle Escape and focus restoration; pending/failure keeps it open. No `window.alert`, `window.confirm`, `window.prompt` or fake Undo. Routine preferences and profile additions need no extra confirmation.

Critical errors persist in one alert banner; progress/success use one polite status region. Icon-only controls have localized names. Hover close actions also reveal on keyboard focus. Full values remain accessible. A narrow unload guard protects dirty port drafts. Subscription input survives recoverable failure and is discarded on explicit cancel/close.

## Async, validation and sensitive values

Mutations are pessimistic and serial. `ReadEpoch` invalidates older reads when a mutation/new read begins; stale responses cannot overwrite newer state or pending feedback. Poll every three seconds only while visible and idle; fetch typed logs only on their tab. Reopening revalidates status. Keep open-menu triggers stable. Backend owns timeouts, executable validation, persistence and proxy recovery; never automatically retry side effects.

Read failure may retain the last genuine snapshot only with an explicit stale-data error and retry. It cannot masquerade as fresh success. Stop clears live snapshot/traffic; a profile/core transition must not present previous-instance values as current. No demo providers, nodes, statistics or logs fill missing data.

Forms use `noValidate`, labels, associated errors, `aria-invalid` and first-invalid focus on explicit submission. Ports are distinct integers 1024–65535 and autosave after 750ms without further input; Enter commits/retries, with no Save button. Subscription requires default-port HTTPS without userinfo/fragments. The input starts masked, offers accessible reveal, is removed after import/close and never enters browser persistence. Redact URLs in errors. Use text nodes for all untrusted data, never HTML parsing sinks.

## Verification

Run typecheck, unit tests, production build, browser tests, DESIGN lint and the premium strict audit after implementation settles. Browser coverage must include 360px source layout/all tabs, short/narrow viewport, rules beyond row 50, 120-entry prefilter caps, supported/disabled features, profiles, failures, stale reads, menus/keyboard, modal focus, themes and preview limitations. Tests for the prior dashboard/native select/numbered pagination must migrate with the owner.

Native Windows evidence is separate: tray/work-area placement, pin/outside focus, two-window hover crossing, Escape order, focus restoration, dynamic height, DPI/monitor edges, real lifecycle/profile restart and registry proxy recovery. Browser mocks/static lint cannot prove these or full accessibility compliance. Report executed checks and unresolved risks honestly.
