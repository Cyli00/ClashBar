# UX Contract

## Product context and sources

ClashBar Windows manages one local mihomo process. The repository README defines the original client workflows; `src/types.ts` and Rust Tauri commands define this port's API contract. Current task scope selects Rust/Tauri, local executable/configuration, Windows system proxy, groups, rules, connections, providers and bounded logs. TUN, remote machines, autostart and automatic core downloads are outside this increment. Locale is `zh-CN`; timestamps use the user's local OS timezone. Target WCAG 2.2 AA.

| Domain | Authoritative source | Reviewed |
|---|---|---|
| Lifecycle and system proxy | `src-tauri/src` command implementations | 2026-10-08 |
| Existing workflows | repository `README.md` and Swift views | 2026-10-08 |
| IPC shapes | `src/types.ts`, Rust command return types | 2026-10-08 |
| Visual intent | `DESIGN.md`, `src/style.css` | 2026-10-08 |

No billing, identity, remote sharing or legal workflows exist here. Credentials in subscription URLs must not enter persistent frontend storage or status messages.

## Canonical UI Map

| Capability | Canonical owner | Source of truth | Allowed variants | Verification |
|---|---|---|---|---|
| Select/Listbox | native `select` in `src/main.ts` | this contract | native Windows/WebView popup | browser keyboard checks |
| Form | `field`, `setFieldError` in `src/ui.ts` | this contract and validators | subscription / port settings | unit + browser validation |
| Scrollbar | global `src/style.css` | DESIGN.md | table/log geometry only | browser computed styles |
| Toast | persistent `notice` in `src/main.ts` | this contract | progress / success / info | browser live region |
| CRUD | `mutate`, `refresh` in `src/main.ts` | Rust commands | local import / settings / connection interrupt | browser complete flows |
| Dialog | `confirmAction` in `src/ui.ts` | this contract | connection interrupt / system proxy permission | browser focus + failure |

Table selection and date entry are not applicable. Browser native select popups are an intentional platform choice; the product does not own their geometry.

## Dataset navigation

Mihomo returns complete lists. Rules and connections filter locally with IME-aware input and paginate 50 items per page. Clamp after filtering or removal; show range and total. Filters and pages remain in memory across tabs. They are deliberately excluded from URLs and persistent storage because a local utility has no shareable navigation and hostnames can be sensitive. Empty, no results, loading and retained stale data are distinct. Log rendering keeps only the last 500 lines; filtering operates on this bounded view.

## Flow ledger

| Operation | Pending | Success | Failure and focus |
|---|---|---|---|
| Core/config/ports/subscription | all mutations disabled, status message | stay in settings; re-read status | retain inputs, persistent error; field validation focuses first invalid |
| Start/stop/system proxy | service button busy | authoritative status returned | never assert running/proxy state optimistically |
| Select node/mode/provider update | disable mutation controls | refresh snapshot in same tab | previous authoritative selection, inline error |
| Close connection | app-owned dialog, cancel initially focused | close dialog, refresh and clamp page | keep dialog open for retry; restore trigger or selected tab |
| Search | immediate local filtering outside composition | same tab and input focus | clear resets page immediately |

## Navigation and responsive behavior

Tabs use roving focus, Arrow Left/Right, Home/End, selected state and associated panels. Titles include the selected tab. Settings and inputs stay mounted while hidden; tab changes do not discard drafts and remask subscription links. A narrow `beforeunload` guard covers dirty ports or entered subscription URLs. Settings use document scrolling; tables have their own bounded scrolling and semantic headings. Full values wrap. There is no route, session, authentication or server pagination state.

## Overlays and feedback

`confirmAction` is the sole confirmation owner: native HTML `dialog.showModal()` with app-authored content, accessible title/description, inert background, focus containment, Escape when idle, cancel-first focus and restoration. Connection interruption, replacing an existing configuration and enabling system proxy require explicit confirmation. The dialog stays open during a mutation and on failure. No `window.alert`, `window.confirm` or `window.prompt` is used. Critical errors persist in one alert banner; successful actions use one polite status region. No fake Undo is offered for closed connections.

## Async and resilience

Mutations are pessimistic and serial. Starting a mutation invalidates every older read through `ReadEpoch`; superseded reads cannot overwrite state or clear a newer pending operation. Poll every three seconds only while visible and idle. Background refresh retains prior data and marks it stale on failure. Refresh is explicit as well as periodic. Native select content is not replaced while focused. Backend commands own request timeout, executable validation, settings persistence and proxy restoration; there is no browser-side retry of side effects. Input values survive recoverable failures.

## Validation and sensitive values

All forms use `noValidate`. Inline errors have `aria-invalid`, existing descriptions and first-invalid focus. Ports are distinct integers 1024–65535. Subscription URLs must use HTTPS on default port 443 and cannot embed userinfo or fragments. Subscription input is masked by default, with an accessible reveal control; it is cleared after import, remasked on tab navigation and never copied into frontend storage. URLs in backend error text are redacted. No untrusted strings enter HTML parsing sinks; all data uses text nodes.

## Verification

`npm run typecheck`, `npm test`, `npm run build`, `npm run test:e2e` and the premium strict static audit are required. Browser tests use a test-only mocked Tauri IPC bridge for success, failures, duplicate prevention, filtering/paging, stale reads, settings validation, tab keyboard behavior, dialog focus, browser-only limitations and a narrow viewport. This does not substitute for native Windows integration: executable launch, registry proxy restoration and packaged WebView2 require Windows CI/manual smoke evidence. No native-support claim is inferred from browser mocks.
