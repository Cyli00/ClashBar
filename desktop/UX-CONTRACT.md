# UX Contract

## Product context and sources

ClashBar 通过 Rust/Tauri 管理本机 mihomo 与保存的远程 HTTP(S) 控制器。用户要求复刻原版托盘弹窗；`DESIGN.md` 维护视觉，本文件维护行为。语言支持简体中文与英语，技术名称保持原文，时间使用系统时区。键盘语义、可访问名称、状态与可见焦点遵守共同约定，完整无障碍合规须单独运行验证。

| Domain | Authoritative source |
|---|---|
| Source hierarchy and labels | Git 历史中的 Swift 菜单栏界面；`docs/public/clashbar-light.png`、`clashbar-black.png` |
| Lifecycle, profiles and proxy recovery | `src-tauri/src/engine.rs`, `system_proxy.rs`, commands in `lib.rs` |
| Popup geometry, pin/focus and menus | `src-tauri/src/popup.rs`, `popup_state.rs`, native geometry implementation |
| Current data and IPC | `src/types.ts`, Rust return types, `src/main.ts` |
| Shared visual/UI owners | `DESIGN.md`, `src/style.css`, `src/ui.ts`, `src/menu.ts` |

The source UI is a fidelity reference, not a promise of complete Windows support. Subscription credentials never enter frontend storage or status messages. 远程控制器凭据由 Rust 保存；状态 IPC 仅返回 hasSecret，不回传密钥。没有账单、共享或应用账号登录流程。

## Canonical UI Map

| Capability | Canonical owner | Source of truth | Allowed variants | Verification |
|---|---|---|---|---|
| Select/Listbox | `showMenu`, `renderMenu`, `mountSubmenu` in `src/menu.ts` | this contract and native protocol | authored adjacent native menu; same-renderer browser fallback | keyboard, hover bridge, selection, focus, work-area placement |
| Form | `field`, `setFieldError`, `searchField` in `src/ui.ts`; shared validators | this contract and backend validation | subscription / ports / local search / remote machine editor | IME, validation and recovery |
| Scrollbar | global `src/style.css` | DESIGN.md | main body / menu body / modal overflow | computed styles and reachability |
| Toast | persistent `notice` live region in `src/main.ts` | this contract | pending / success / information | announcement and duplicate checks |
| CRUD | `mutate`, `refresh`, `ReadEpoch` | Rust commands and this contract | profile import/select, lifecycle, settings, connection close | complete flows and stale-read tests |
| Dialog | `presentDialog` in `src/ui.ts`; subscription/delete content in `src/main.ts`; remote manager in `src/remote.ts` | this contract | subscription import / profile deletion / remote manager | initial focus, trap, Escape, retry and restoration |
| Tabs | `activateTab` and persistent panel map | this contract | five source tabs | roving focus, selection, scroll/draft retention |

There is no native HTML `select` owner. Menu geometry/content are authored even though the surface is a native window. Its width need not match a small trigger. Date entry and row selection are inapplicable. Reuse these owners; unused legacy helpers do not define a competing contract.

## Popup and adjacent-menu protocol

Rust owns the undecorated tray window's monitor-aware placement, work-area clamping, visibility and pin state. The frontend requests desired height through `resize_popup`; header, modes, tabs and footer stay mounted while the body scrolls. `get_popup_pinned`, `set_popup_pinned`, `hide_popup` and `quit_app` own those native actions. Hiding does not stop the core. Pin is session-only and does not block explicit hide/tray toggle. Native outside-focus handling has a 120ms grace period and protects transitions between the owned main/menu windows and native file dialogs. Outside focus/click dismisses an unpinned popup; when pinned it dismisses the menu only.

`show_attached_menu` receives the trigger's logical rectangle, desired dimensions, menu model and focus intent. `get_attached_menu` initializes the child; `menu-data` updates it and `menu-focus` requests focus. Child `attached_menu_action` relays `{menuId, actionId}` as `attached-menu-action` to the main window, where the registered callback owns the operation. `hide_attached_menu` emits `attached-menu-closed`; `attached_menu_hover`/`attached-menu-hover` bridge pointer presence. Reject obsolete menu IDs and disabled/separator actions. Primary actions close the menu; secondary latency tests keep it open. `secondaryKind=context` is the profile management variant: right-click, Shift+F10 and the labelled management button dispatch the same action and open the profile action menu.

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

Search is local and IME-aware; clear acts immediately and returns input focus. Filtering must not strand the virtual view beyond accessible results. Rules distinguish `暂无规则数据`/`无匹配规则`; other tabs likewise distinguish empty, no-match, stopped and loading states. 规则页的 `规则集` 显示规则提供者总数。`统计` 按 payload 与提供者键或 name 的大小写无关匹配显示 ruleCount 和更新时间；没有对应提供者时为 0，数据缺失时为 `—`。刷新先读取最新提供者列表，按名称顺序逐个更新；单项失败仍继续，结束后重新读取快照并报告成功/失败数量。

浏览器只持久化非敏感偏好：提供者折叠、延迟排序/历史、隐藏组、规则分组、连接协议/排序、外观与语言。查询、规则策略/类型、日志筛选与展开分组保留于内存。外观提供跟随系统、浅色、深色；语言切换在保存端口草稿后调用 set_ui_language 同步 Rust 与原生菜单，再重载并恢复标签。首次无浏览器语言偏好时采用 Status.uiLanguage。迁移旧版以可见文案保存的筛选和外观。状态栏三种样式保存到 Rust，并以 Status.statusBarStyle 为准。

## Available features and truthful affordances

| Area | Available behavior |
|---|---|
| Core/profiles | 本机内核选择、YAML/HTTPS 导入、配置切换/删除、订阅源保存/单项和全部刷新/定时、SSID 自动切换、启停/重启、五种代理端口与控制器端口。 |
| Routing | rule/global/direct、节点选择/测速、代理与规则提供者刷新、统计与配额、TUN 四栈；mips 需要 mihomo ≥1.19.31。 |
| System | 系统代理快照与恢复、绕过编辑/恢复默认、loopback/局域网/远程终端命令、固定、三种状态栏样式、中英语言、浅深主题、LAN/IPv6/TCP 开关。 |
| Connections/logs | 单项/全部断开、协议/排序/搜索、日志来源/级别/搜索/复制/清空、运行时日志级别。 |
| Maintenance | 内核选择/更新、内核与配置目录、FakeIP/DNS 缓存、Geo 更新、WebUI、应用版本检查与官方发布页。 |

不能执行的控件禁用并解释原因；未知值保持未知。浏览器预览禁用桌面操作并说明桌面依赖。应用日志记录真实动作，get_log_entries 返回 timestamp/source/message。来源和级别独立多选，空集合表示全部。日志级别通过 Rust 持久化，本机重启继续使用，两个选择器读取同一运行快照。成功清空日志不记录自身成功消息，直到产生新事件才重新显示记录。

## Flow ledger

| Operation | Pending | Success | Failure and focus |
|---|---|---|---|
| Core picker / ports | stopped core required; ports autosave after 750ms idle and submit/retry on Enter | returned status; retain current tab; no Save button | preserve draft; inline validation; Enter retries a failed save |
| Profile import / select | serialize; backend owns any live-core transition | retain profiles and acknowledge active profile | retain recoverable input/authoritative state; explain failure |
| Start / stop / restart / proxy | busy controls; pessimistic state | returned status and re-read data | no unacknowledged running/proxy claim |
| Node / mode / provider / log level | disable mutation controls | refresh same tab; both level selectors agree | error and last authoritative choice |
| Single/all connection close | direct source-style action; serialize mutation | refresh connections in the same tab | persistent error; retry through the action |
| Clear logs | direct source-style action; serialize mutation | backend and combined buffers empty; no self-log | persistent error; retain data where possible and retry |
| LAN / IPv6 / TCP concurrency | read the current core value; serialize changes through `set_core_boolean` | persist acknowledged value and apply it after restart/profile switch | restore previous runtime value on write failure; retain error and re-read actual state |
| Rule-provider refresh / maintenance | reuse serial `mutate`; disable controls while pending; Geo request timeout 60 seconds | stay on the same tab and re-read snapshot after completion | persistent actionable error; partial rule refresh retains successful updates and allows explicit retry |
| Search / preferences | local operation | same tab/focus, updated view | no backend success implied |

打开目录只接受命名 IPC 命令：内核目录从已选择的可执行文件解析，配置目录由 Rust 固定，不接受前端路径或 shell 参数。

导入增加配置，不替换整个库。通过右键、Shift+F10 或管理按钮进入配置动作菜单，删除确认初始聚焦取消。删除当前配置先验证并切换下一项；删除最后配置停止内核并恢复系统代理。文件移至 deleted-profiles，不改动原始导入文件；归档或保存失败恢复文件、选择与运行状态。订阅源和调度由 Rust 保存，状态只返回主机及时间/错误摘要，不回传完整链接；复制链接由 Rust 直接写系统剪贴板。编辑链接留空保留原 URL，失败保留表单输入。自动更新时间为 1–8760 小时，默认启用且每 6 小时检查。运行中禁止替换本机可执行文件和本机端口；内核自身升级单独调用 upgrade_core。

## Navigation, overlays and feedback

Tabs expose selection, associated panels, Arrow Left/Right and Home/End roving focus. Titles include the active tab. Settings/port inputs stay mounted. `.content-scroll` is the only main-panel vertical scroller, keeping all settings reachable in short windows while header/footer stay fixed. Menus have bounded body scrolling; long modals scroll within the viewport.

The user's explicit source-fidelity instruction selects direct actions for system-proxy toggling, individual/all connection closure and log clearing: no confirmation dialog is inserted. Mutations remain serial, error-aware and backend-acknowledged. Subscription import uses `dialog.showModal()` with app-authored content, an accessible title, inert background, contained focus, idle Escape and focus restoration; pending/failure keeps it open. No `window.alert`, `window.confirm`, `window.prompt` or fake Undo. Routine preferences and profile additions need no extra confirmation.

Critical errors persist in one alert banner; progress/success use one polite status region. Icon-only controls have localized names. Hover close actions also reveal on keyboard focus. Full values remain accessible. A narrow unload guard protects dirty port drafts. Subscription input survives recoverable failure and is discarded on explicit cancel/close.

## Async, validation and sensitive values

Mutations are pessimistic and serial. `ReadEpoch` invalidates older reads when a mutation/new read begins; stale responses cannot overwrite newer state or pending feedback. Poll every three seconds only while visible and idle; fetch typed logs only on their tab. Reopening revalidates status. Keep open-menu triggers stable. LAN 开关只修改代理监听地址；控制器仍绑定 loopback 并使用随机密钥。IPv6/TCP 设置未被用户覆盖时继承配置，明确修改后持久化。内核没有返回某项布尔值时禁用该开关，不把缺失值视为支持。Backend owns timeouts, executable validation, persistence and proxy recovery; never automatically retry side effects.

Read failure may retain the last genuine snapshot only with an explicit stale-data error and retry. It cannot masquerade as fresh success. Stop clears live snapshot/traffic; a profile/core transition must not present previous-instance values as current. No demo providers, nodes, statistics or logs fill missing data.

表单使用 noValidate、真实标签、关联错误与 aria-invalid；提交时聚焦错误字段。本机与远程五种代理端口均为 0–65535 的整数，0 关闭监听，启用端口不能重复。本机控制器端口为 1024–65535，不能与任意代理端口重复。端口空闲 750ms 自动保存，Enter 提交或重试。订阅使用默认端口 HTTPS，不含 userinfo 或片段；链接默认遮罩，可显隐，成功或关闭后移除，不进入浏览器存储。错误中的 URL 脱敏。所有不可信数据通过文本节点显示。

## Verification

Run typecheck, unit tests, production build, browser tests, DESIGN lint and the premium strict audit after implementation settles. Browser coverage must include 360px source layout/all tabs, short/narrow viewport, rules beyond row 50, 120-entry prefilter caps, supported/disabled features, profiles, failures, stale reads, menus/keyboard, modal focus, themes and preview limitations. Tests for the prior dashboard/native select/numbered pagination must migrate with the owner.

Native Windows evidence is separate: tray/work-area placement, pin/outside focus, two-window hover crossing, Escape order, focus restoration, dynamic height, DPI/monitor edges, real lifecycle/profile restart and registry proxy recovery. Browser mocks/static lint cannot prove these or full accessibility compliance. Report executed checks and unresolved risks honestly.


## 远程机器与本机隔离

行为来源为 Git 历史中的 RemoteMachine 模型、存储、视图模型与机器管理界面，当前实现以 `remote.rs`、`remote.ts` 为准：

- 顶部控制器菜单可选择本机或远程目标，并打开机器管理面板。面板沿用列表 → 新增/编辑 → 返回列表的路径，支持名称、主机、端口、密钥和 HTTPS。每 5 秒检查列表连通性，旧的探测结果不能覆盖已编辑/删除的机器。
- 远程地址允许主机名、IPv4、IPv6；协议、端口、路径各有固定归属，拒绝 userinfo、控制字符和路径注入。连接不跟随重定向，TLS 使用证书校验。保存非活动机器可离线进行，选择目标或编辑活动机器需要先通过连接检查。
- 已保存密钥不进入状态响应或浏览器存储；编辑留空保留，明确勾选清除才删除。输入默认遮罩，离开有改动的编辑表单前显示放弃/继续编辑选择。
- 目标 ID 或会话版本变化时清空旧快照、测速结果、图表和日志；`ReadEpoch` 丢弃旧请求。未提交端口必须先修正保存，才可打开机器管理或切换目标。
- 本机进程持续运行，不因切换或删除远程目标而停止。远程视图禁用本地启停、配置文件、内核选择及目录操作，Rust 同时校验本地操作边界。节点、规则、连接、维护、日志级别、布尔设置均使用选中控制器。
- 远程五种代理端口在运行中通过 PATCH 更新，0 关闭监听，已启用端口不可重复。远程设置不写入本机偏好。修改失败尝试恢复旧端口，并显示恢复结果。
- 系统代理始终操作当前 Windows 用户，但可显式指向当前远程内核的混合端口，或独立 HTTP/SOCKS 端口；界面标记本地/远程范围。保留原有 WinINet 快照与所有权检查，本机内核退出不撤销远程代理，退出应用恢复所有自有代理。关闭全部远程代理端口前要求先关闭指向它的系统代理。
- 远程日志使用独立的 NDJSON 流，保留 500 条、单条最多 4096 个字符；输入行上限 64 KiB。连接失败自动重连，切换目标取消旧流。级别变化重订阅并保留当前目标的记录，silent 停止订阅。状态读取仅在日志标签呈现日志流错误。
- 「复制终端命令」保留本机入口，远程目标另有「复制当前端点命令」，使用该目标实际 HTTP/SOCKS/混合端口，支持 IPv6，不携带控制器密钥。

## SSID、TUN、更新与快捷键

配置菜单提供 SSID 自动切换与管理入口。仅在主动启用或刷新后读取当前 Wi-Fi；按配置绑定当前网络，重复绑定同一配置即解绑。管理界面可刷新状态、启停自动切换、查看和删除绑定；状态/权限失败显示后端原因。后台匹配由 Rust 执行，不依赖弹窗可见性。

定位权限被拒绝时显示「打开定位设置」，仅调用固定系统设置 URI。订阅更新时间与调度时间均为 Unix 毫秒；相对时间展示不得再次乘以 1000。

TUN 开关和协议栈胶囊复用串行操作与状态回读，失败不显示未经确认的开启状态。本机按需使用 Windows 授权，远程仅修改所选控制器；mips 不满足最低版本时禁用并解释版本要求。

WebUI 由 Rust 根据控制器元数据选择已部署界面或官方仪表板，连接参数与密钥不经过前端状态。应用版本按钮检查正式发布并给出当前/最新版本，打开官方发布页；不伪装为已安装更新。内核菜单分别提供停止时选择文件与运行时更新。

本机终端命令分别提供 loopback 与活动局域网 IPv4，使用实际 mixed 或独立 HTTP/SOCKS 端口。检测不到局域网地址时禁用对应复制入口并显示原因。代理组图标仅接收 Rust 受控缓存的 data URI；失败不移除组名。流量优先使用真实 WebSocket 速率，流过期时退回总量差分。

保留旧端快捷键语义：Ctrl（macOS 为 Command）+S 切换系统代理，+E 切换 TUN，+Shift+1/2/3 切换模式，+Alt+1…5 切换标签，+, 打开设置，+Shift+R 启动或重启，+Shift+. 停止，+Alt+C 复制当前端点命令。输入法组合、输入字段、模态对话框和按键重复期间不触发这些全局操作。

## 应用日志与错误语言

动作完成或失败通过 record_app_action 写入 Rust 应用日志，前端使用返回的时间、来源与脱敏消息即时展示。回读 get_log_entries 时按 timestamp/source/message 去重；归档写入失败仅保留当前会话内存记录，不覆盖原操作结果。成功清空日志不记录自身成功消息。

日志行通过右键或 Shift+F10 提供「复制消息」和「复制完整记录」。完整记录格式为 `[日期时间] [来源大写] [等级大写] 消息`；复制全部使用该格式连接全部保留记录。连接行同时显示规则类型和值，搜索包括网络协议和开始时间；「其他协议」排除缺失协议。规则策略筛选对应的策略从新快照消失时自动回到全部。

本机导入先由 prepare_config_import 在 Rust 暂存路径和规范名称，重名时由应用确认覆盖，取消调用 cancel_config_import。确认后 finish_config_import 保持已有配置 ID，失败保留待导入路径供重试。订阅由 prepare_subscription 返回规范名称及重名状态；第一步提示替换后果，第二次「覆盖配置」才传 overwrite:true。名称或链接改变立即撤销确认。新增配置不切换现有活动配置，仅空库第一项自动选中；覆盖活动配置才应用新内容。配置菜单同时提供打开目录和在文件夹中选中当前配置。

`src/errors.ts` 翻译应用拥有的中文错误与端口、HTTP 状态、SSID、订阅汇总中的固定文案；保留动态端口、配置名称与系统底层原因。内核原始日志不翻译。`safeError` 先遮蔽 URL，再处理语言与长度限制。订阅摘要、SSID、远程连接探测和启动项状态错误复用该入口。


## 启动偏好

`开机自启` 通过 Tauri autostart 的 Rust 接口读写当前用户登录启动项，每次操作后重新读取系统状态，失败时保持已确认状态并显示错误。不向 WebView 开放通用系统命令或插件启停权限。该设置始终针对本机应用，包括查看远程机器时。

`内核自启` 对应旧端 `clashbar.auto.start.core`，默认关闭，只保存偏好，不在切换开关时立即启动进程。应用初始化完成后，仅在选择本机且已有内核与配置时执行一次受管理启动；未配置或远程目标跳过，启动失败保留错误并展开弹窗。系统代理沿用用户保存的启用意图，本机启动成功后由 Rust 恢复，前端以返回状态为准。
