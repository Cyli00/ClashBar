//! Native tray-owned windows. Frontend content never owns placement or dismissal.
use crate::{
    panel_geometry::{self, Edge, Placement, Rect},
    popup_state::{BlurAction, PopupStateMachine},
};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, sync::Mutex, time::Duration};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

pub const TRAY_ID: &str = "clashbar";
const MAIN: &str = "main";
const MENU: &str = "submenu";

#[derive(Clone, Copy)]
struct MainPlacement {
    work: Rect,
    scale: f64,
    placement: Placement,
}

#[derive(Clone)]
struct ActiveMenu {
    data: AttachedMenu,
    anchor: Anchor,
    width: f64,
    height: f64,
}

struct Session {
    state: PopupStateMachine,
    anchor: Option<Rect>,
    requested_height: f64,
    placement: Option<MainPlacement>,
    menu: Option<ActiveMenu>,
}

pub struct PopupState(Mutex<Session>);

impl Default for PopupState {
    fn default() -> Self {
        Self(Mutex::new(Session {
            state: PopupStateMachine::default(),
            anchor: None,
            requested_height: 320.0,
            placement: None,
            menu: None,
        }))
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PopupSize {
    width: f64,
    height: f64,
    max_height: f64,
}

#[derive(Clone, Copy, Deserialize)]
pub struct Anchor {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MenuItem {
    id: String,
    label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    value: Option<String>,
    #[serde(default)]
    disabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    checked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secondary_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secondary_label: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AttachedMenu {
    id: String,
    title: String,
    items: Vec<MenuItem>,
}

#[derive(Clone, Copy, Serialize)]
pub struct MenuLayout {
    side: &'static str,
    width: f64,
    height: f64,
}

fn main_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    app.get_webview_window(MAIN)
        .ok_or("The tray popup is unavailable.".into())
}

fn emit_visibility(app: &AppHandle) {
    let state = app.state::<PopupState>();
    let data = {
        let session = state.0.lock().unwrap_or_else(|error| error.into_inner());
        serde_json::json!({"visible": session.state.visible, "pinned": session.state.pinned})
    };
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.emit("popup-visibility", data);
    }
}

fn from_native(rect: tauri::Rect) -> Rect {
    // TrayIconEvent documents physical coordinates, regardless of the popup's DPI.
    let position = rect.position.to_physical::<f64>(1.0);
    let size = rect.size.to_physical::<f64>(1.0);
    Rect {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    }
}

fn tray_anchor(app: &AppHandle) -> Option<Rect> {
    app.tray_by_id(TRAY_ID)
        .and_then(|tray| tray.rect().ok().flatten())
        .map(from_native)
}

fn monitor_context(app: &AppHandle, anchor: Option<Rect>) -> Result<(Rect, Rect, f64), String> {
    let monitor = if let Some(anchor) = anchor {
        let (x, y) = anchor.center();
        app.monitor_from_point(x, y)
            .map_err(|error| error.to_string())?
    } else {
        None
    }
    .or(app.primary_monitor().map_err(|error| error.to_string())?)
    .ok_or("No monitor is available for the tray popup.")?;
    let area = monitor.work_area();
    let work = Rect {
        x: f64::from(area.position.x),
        y: f64::from(area.position.y),
        width: f64::from(area.size.width),
        height: f64::from(area.size.height),
    };
    let scale = monitor.scale_factor();
    let anchor = anchor.unwrap_or(Rect {
        x: work.right() - 24.0 * scale,
        y: work.bottom(),
        width: 16.0 * scale,
        height: 16.0 * scale,
    });
    Ok((work, anchor, scale))
}

fn apply_main_layout(app: &AppHandle, preserve_x: bool) -> Result<PopupSize, String> {
    let state = app.state::<PopupState>();
    let fresh_anchor = tray_anchor(app);
    let (anchor, requested_height, previous) = {
        let session = state.0.lock().unwrap_or_else(|error| error.into_inner());
        (
            fresh_anchor.or(session.anchor),
            session.requested_height,
            session.placement,
        )
    };
    let (work, anchor, scale) = monitor_context(app, anchor)?;
    let locked_x = previous
        .filter(|old| preserve_x && old.work == work && old.scale == scale)
        .map(|old| old.placement.rect.x);
    let placement = panel_geometry::main_popup(work, anchor, scale, requested_height, locked_x);
    let window = main_window(app)?;
    let changed =
        previous.is_none_or(|old| old.placement.rect != placement.rect || old.scale != scale);
    {
        let mut session = state.0.lock().unwrap_or_else(|error| error.into_inner());
        session.anchor = Some(anchor);
        session.placement = Some(MainPlacement {
            work,
            scale,
            placement,
        });
    }
    if changed {
        // Move first so Windows applies the destination monitor's DPI before sizing.
        window
            .set_position(PhysicalPosition::new(
                placement.rect.x.round() as i32,
                placement.rect.y.round() as i32,
            ))
            .map_err(|e| e.to_string())?;
        window
            .set_size(PhysicalSize::new(
                placement.rect.width.round() as u32,
                placement.rect.height.round() as u32,
            ))
            .map_err(|e| e.to_string())?;
    }
    Ok(PopupSize {
        width: placement.rect.width / scale,
        height: placement.rect.height / scale,
        max_height: placement.max_height,
    })
}

pub fn show(app: &AppHandle) -> Result<(), String> {
    apply_main_layout(app, false)?;
    {
        let state = app.state::<PopupState>();
        state
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .state
            .show();
    }
    let window = main_window(app)?;
    window.show().map_err(|error| error.to_string())?;
    refresh_tool_style(&window)?;
    window.set_focus().map_err(|error| error.to_string())?;
    emit_visibility(app);
    Ok(())
}

pub fn hide(app: &AppHandle) -> Result<(), String> {
    let permitted = app
        .state::<PopupState>()
        .0
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .state
        .hide();
    if !permitted {
        return Ok(());
    }
    hide_menu(app, false)?;
    main_window(app)?
        .hide()
        .map_err(|error| error.to_string())?;
    emit_visibility(app);
    Ok(())
}

pub fn resize(app: &AppHandle, height: f64) -> Result<PopupSize, String> {
    if !height.is_finite() || !(1.0..=100_000.0).contains(&height) {
        return Err("Invalid popup content height.".into());
    }
    app.state::<PopupState>()
        .0
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .requested_height = height;
    let size = apply_main_layout(app, true)?;
    reposition_menu(app)?;
    Ok(size)
}

pub fn pinned(app: &AppHandle) -> bool {
    app.state::<PopupState>()
        .0
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .state
        .pinned
}

pub fn pin(app: &AppHandle, pinned: bool) {
    app.state::<PopupState>()
        .0
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .state
        .pin(pinned);
    emit_visibility(app);
}

pub fn refresh_position(app: &AppHandle) {
    let visible = app
        .state::<PopupState>()
        .0
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .state
        .visible;
    if visible {
        let _ = apply_main_layout(app, true);
        let _ = reposition_menu(app);
    }
}

pub fn on_tray_event(app: &AppHandle, event: tauri::tray::TrayIconEvent) {
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
    match event {
        TrayIconEvent::Click {
            rect,
            button: MouseButton::Left,
            button_state,
            ..
        } => {
            let action = {
                let state = app.state::<PopupState>();
                let mut session = state.0.lock().unwrap_or_else(|error| error.into_inner());
                session.anchor = Some(from_native(rect));
                match button_state {
                    MouseButtonState::Down => {
                        session.state.tray_pressed();
                        None
                    }
                    MouseButtonState::Up => session.state.tray_released(),
                }
            };
            if let Some(visible) = action {
                let _ = if visible { show(app) } else { hide(app) };
            }
        }
        TrayIconEvent::Enter { rect, .. } | TrayIconEvent::Move { rect, .. } => {
            app.state::<PopupState>()
                .0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .anchor = Some(from_native(rect));
        }
        TrayIconEvent::Leave { .. } => schedule_blur(app),
        _ => {}
    }
}

fn owned_focus(app: &AppHandle) -> bool {
    [MAIN, MENU]
        .iter()
        .filter_map(|label| app.get_webview_window(label))
        .any(|window| window.is_visible().unwrap_or(false) && window.is_focused().unwrap_or(true))
}

fn schedule_blur(app: &AppHandle) {
    let ticket = app
        .state::<PopupState>()
        .0
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .state
        .blur_ticket();
    let Some(ticket) = ticket else {
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Let a main→menu focus transfer or the tray mouse-down establish intent.
        tokio::time::sleep(Duration::from_millis(120)).await;
        let callback = app.clone();
        let _ = app.run_on_main_thread(move || {
            let focused = owned_focus(&callback);
            let cursor_on_tray = callback.cursor_position().ok().is_some_and(|point| {
                tray_anchor(&callback).is_some_and(|rect| rect.contains(point.x, point.y))
            });
            let action = callback
                .state::<PopupState>()
                .0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .state
                .resolve_blur(ticket, focused, cursor_on_tray);
            match action {
                BlurAction::CloseAll => {
                    let _ = hide(&callback);
                }
                BlurAction::CloseMenu => {
                    let _ = hide_menu(&callback, false);
                }
                BlurAction::Keep => {}
            }
        });
    });
}

pub fn on_window_event(app: &AppHandle, label: &str, event: &tauri::WindowEvent) {
    if ![MAIN, MENU].contains(&label) {
        return;
    }
    match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let _ = if label == MAIN {
                hide(app)
            } else {
                hide_menu(app, true)
            };
        }
        tauri::WindowEvent::Focused(true) => {
            app.state::<PopupState>()
                .0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .state
                .focus_gained();
        }
        tauri::WindowEvent::Focused(false) => schedule_blur(app),
        tauri::WindowEvent::ScaleFactorChanged { .. } if label == MAIN => refresh_position(app),
        _ => {}
    }
}

pub struct DialogGuard {
    app: AppHandle,
}

impl DialogGuard {
    pub fn new(app: &AppHandle) -> Self {
        app.state::<PopupState>()
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .state
            .begin_dialog();
        let _ = hide_menu(app, false);
        Self { app: app.clone() }
    }
}

impl Drop for DialogGuard {
    fn drop(&mut self) {
        let restore = self
            .app
            .state::<PopupState>()
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .state
            .end_dialog();
        if restore {
            let app = self.app.clone();
            let _ = self.app.run_on_main_thread(move || {
                let _ = show(&app);
            });
        }
    }
}

fn validate_menu(menu: &AttachedMenu) -> Result<(), String> {
    if menu.id.is_empty()
        || menu.id.len() > 1024
        || menu.title.len() > 4096
        || menu.items.len() > 2000
        || serde_json::to_vec(menu)
            .map_err(|error| error.to_string())?
            .len()
            > 128 * 1024
    {
        return Err("The attached menu is too large.".into());
    }
    let mut identifiers = HashSet::new();
    for item in &menu.items {
        if !matches!(item.kind.as_deref(), None | Some("item" | "separator"))
            || item.label.len() > 4096
            || item.detail.as_ref().is_some_and(|s| s.len() > 4096)
            || item.value.as_ref().is_some_and(|s| s.len() > 4096)
        {
            return Err("Invalid attached menu item.".into());
        }
        if item.kind.as_deref() == Some("separator") {
            continue;
        }
        for id in std::iter::once(&item.id).chain(item.secondary_id.iter()) {
            if id.is_empty() || id.len() > 2048 || !identifiers.insert(id) {
                return Err("Attached menu actions must have unique identifiers.".into());
            }
        }
    }
    Ok(())
}

fn menu_geometry(
    app: &AppHandle,
    menu: &ActiveMenu,
) -> Result<(panel_geometry::AttachedPlacement, f64), String> {
    let host = main_window(app)?;
    let position = host.outer_position().map_err(|error| error.to_string())?;
    let size = host.inner_size().map_err(|error| error.to_string())?;
    let scale = host.scale_factor().map_err(|error| error.to_string())?;
    let monitor = host
        .current_monitor()
        .map_err(|error| error.to_string())?
        .ok_or("No popup monitor is available.")?;
    let area = monitor.work_area();
    let work = Rect {
        x: f64::from(area.position.x),
        y: f64::from(area.position.y),
        width: f64::from(area.size.width),
        height: f64::from(area.size.height),
    };
    let host = Rect {
        x: f64::from(position.x),
        y: f64::from(position.y),
        width: f64::from(size.width),
        height: f64::from(size.height),
    };
    let anchor = Rect {
        x: host.x + menu.anchor.x * scale,
        y: host.y + menu.anchor.y * scale,
        width: menu.anchor.width * scale,
        height: menu.anchor.height * scale,
    };
    Ok((
        panel_geometry::attached_menu(work, host, anchor, scale, menu.width, menu.height),
        scale,
    ))
}

fn place_menu(app: &AppHandle, menu: &ActiveMenu) -> Result<MenuLayout, String> {
    let (placement, scale) = menu_geometry(app, menu)?;
    let window = app
        .get_webview_window(MENU)
        .ok_or("The attached menu window is unavailable.")?;
    window
        .set_position(PhysicalPosition::new(
            placement.rect.x.round() as i32,
            placement.rect.y.round() as i32,
        ))
        .map_err(|error| error.to_string())?;
    window
        .set_size(PhysicalSize::new(
            placement.rect.width.round() as u32,
            placement.rect.height.round() as u32,
        ))
        .map_err(|error| error.to_string())?;
    Ok(MenuLayout {
        side: if placement.side == Edge::Left {
            "left"
        } else {
            "right"
        },
        width: placement.rect.width / scale,
        height: placement.rect.height / scale,
    })
}

fn reposition_menu(app: &AppHandle) -> Result<(), String> {
    let menu = app
        .state::<PopupState>()
        .0
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .menu
        .clone();
    if let Some(menu) = menu {
        place_menu(app, &menu)?;
    }
    Ok(())
}

pub fn show_menu(
    app: &AppHandle,
    anchor: Anchor,
    width: f64,
    height: f64,
    menu: AttachedMenu,
    focus: bool,
) -> Result<MenuLayout, String> {
    validate_menu(&menu)?;
    if ![
        anchor.x,
        anchor.y,
        anchor.width,
        anchor.height,
        width,
        height,
    ]
    .iter()
    .all(|value| value.is_finite())
        || !(1.0..=2048.0).contains(&width)
        || !(1.0..=100_000.0).contains(&height)
        || anchor.width <= 0.0
        || anchor.height <= 0.0
    {
        return Err("Invalid attached menu dimensions.".into());
    }
    let host = main_window(app)?;
    let host_size = host
        .inner_size()
        .map_err(|error| error.to_string())?
        .to_logical::<f64>(host.scale_factor().map_err(|error| error.to_string())?);
    if anchor.x < -10.0
        || anchor.y < -10.0
        || anchor.x > host_size.width + 10.0
        || anchor.y > host_size.height + 10.0
        || anchor.width > host_size.width + 20.0
        || anchor.height > host_size.height + 20.0
    {
        return Err("The menu anchor is outside the tray popup.".into());
    }
    let active = ActiveMenu {
        data: menu,
        anchor,
        width,
        height,
    };
    let was_open = {
        let state = app.state::<PopupState>();
        let session = state.0.lock().unwrap_or_else(|error| error.into_inner());
        if !session.state.visible {
            return Err("Open the tray popup before opening a menu.".into());
        }
        session.menu.is_some()
    };
    let layout = place_menu(app, &active)?;
    {
        let state = app.state::<PopupState>();
        let mut session = state.0.lock().unwrap_or_else(|error| error.into_inner());
        session.menu = Some(active.clone());
        session.state.focus_gained();
    }
    let window = app
        .get_webview_window(MENU)
        .ok_or("The attached menu window is unavailable.")?;
    window
        .emit("menu-data", Some(&active.data))
        .map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())?;
    refresh_tool_style(&window)?;
    // Hovering a row preserves the parent's keyboard focus. A later menu click may focus the owned child.
    if focus {
        window.set_focus().map_err(|error| error.to_string())?;
        let _ = window.emit("menu-focus", ());
    } else if !was_open {
        let _ = host.set_focus();
    }
    Ok(layout)
}

pub fn hide_menu(app: &AppHandle, restore_focus: bool) -> Result<(), String> {
    let (menu, parent_visible) = {
        let state = app.state::<PopupState>();
        let mut session = state.0.lock().unwrap_or_else(|error| error.into_inner());
        (session.menu.take(), session.state.visible)
    };
    if let Some(window) = app.get_webview_window(MENU) {
        let focused = window.is_focused().unwrap_or(false);
        window.hide().map_err(|error| error.to_string())?;
        refresh_tool_style(&window)?;
        let _ = window.emit("menu-data", Option::<AttachedMenu>::None);
        if restore_focus && parent_visible && focused {
            let _ = main_window(app)?.set_focus();
        }
    }
    if let (Some(menu), Some(window)) = (menu, app.get_webview_window(MAIN)) {
        let _ = window.emit(
            "attached-menu-closed",
            serde_json::json!({"menuId":menu.data.id}),
        );
    }
    Ok(())
}

pub fn current_menu(app: &AppHandle) -> Option<AttachedMenu> {
    app.state::<PopupState>()
        .0
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .menu
        .as_ref()
        .map(|menu| menu.data.clone())
}

pub fn menu_action(app: &AppHandle, menu_id: &str, action_id: &str) -> Result<(), String> {
    let menu = current_menu(app).ok_or("The attached menu is closed.")?;
    if menu.id != menu_id {
        return Err("The attached menu has changed.".into());
    }
    let mut secondary = false;
    let valid = menu.items.iter().any(|item| {
        if item.disabled || item.kind.as_deref() == Some("separator") {
            return false;
        }
        if item.id == action_id {
            return true;
        }
        if item.secondary_id.as_deref() == Some(action_id) {
            secondary = true;
            return true;
        }
        false
    });
    if !valid {
        return Err("This menu action is unavailable.".into());
    }
    main_window(app)?
        .emit(
            "attached-menu-action",
            serde_json::json!({"menuId":menu_id,"actionId":action_id}),
        )
        .map_err(|error| error.to_string())?;
    if !secondary {
        hide_menu(app, true)?;
    }
    Ok(())
}

pub fn menu_hover(app: &AppHandle, menu_id: &str, hovered: bool) -> Result<(), String> {
    if current_menu(app).is_none_or(|menu| menu.id != menu_id) {
        return Ok(());
    }
    main_window(app)?
        .emit(
            "attached-menu-hover",
            serde_json::json!({"menuId":menu_id,"hovered":hovered}),
        )
        .map_err(|error| error.to_string())
}

pub fn configure_windows(app: &AppHandle) -> Result<(), String> {
    for label in [MAIN, MENU] {
        let window = app
            .get_webview_window(label)
            .ok_or_else(|| format!("Missing {label} popup window."))?;
        window
            .set_skip_taskbar(true)
            .map_err(|error| error.to_string())?;
        restore_tool_style(&window)?;
        #[cfg(windows)]
        if label == MENU {
            use windows_sys::Win32::{
                Foundation::{GetLastError, SetLastError},
                UI::WindowsAndMessaging::{SetWindowLongPtrW, GWLP_HWNDPARENT},
            };
            let main = main_window(app)?
                .hwnd()
                .map_err(|error| error.to_string())?;
            let child = window.hwnd().map_err(|error| error.to_string())?;
            // Both are top-level windows owned by this process; assign an owner, not a child style.
            unsafe {
                SetLastError(0);
                if SetWindowLongPtrW(child.0 as _, GWLP_HWNDPARENT, main.0 as isize) == 0
                    && GetLastError() != 0
                {
                    return Err(format!(
                        "Cannot attach the menu window: {}",
                        std::io::Error::last_os_error()
                    ));
                }
            }
        }
    }
    Ok(())
}

fn restore_tool_style(window: &WebviewWindow) -> Result<(), String> {
    #[cfg(not(windows))]
    let _ = window;
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::{GetLastError, SetLastError},
            UI::WindowsAndMessaging::{
                GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, GWL_STYLE,
                SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_CAPTION,
                WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_THICKFRAME,
            },
        };
        let hwnd = window.hwnd().map_err(|error| error.to_string())?.0 as _;
        // Tauri skip_taskbar removes a taskbar tab; the explicit tool style also excludes Alt+Tab.
        unsafe {
            let old = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let style = (old | WS_EX_TOOLWINDOW as isize) & !(WS_EX_APPWINDOW as isize);
            SetLastError(0);
            if SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style) == 0 && GetLastError() != 0 {
                return Err(format!(
                    "Cannot configure the tray tool window: {}",
                    std::io::Error::last_os_error()
                ));
            }
            let old = GetWindowLongPtrW(hwnd, GWL_STYLE);
            let style = old & !((WS_CAPTION | WS_THICKFRAME) as isize);
            SetLastError(0);
            if SetWindowLongPtrW(hwnd, GWL_STYLE, style) == 0 && GetLastError() != 0 {
                return Err(format!(
                    "Cannot remove the native window frame: {}",
                    std::io::Error::last_os_error()
                ));
            }
            if SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
            ) == 0
            {
                return Err(format!(
                    "Cannot apply the tray window style: {}",
                    std::io::Error::last_os_error()
                ));
            }
        }
    }
    Ok(())
}

fn refresh_tool_style(window: &WebviewWindow) -> Result<(), String> {
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            if let Err(error) = restore_tool_style(&target) {
                eprintln!("{error}");
            }
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn resize_popup(app: AppHandle, height: f64) -> Result<PopupSize, String> {
    resize(&app, height)
}
#[tauri::command]
pub fn hide_popup(app: AppHandle) -> Result<(), String> {
    hide(&app)
}
#[tauri::command]
pub fn set_popup_pinned(app: AppHandle, pinned: bool) {
    pin(&app, pinned);
}
#[tauri::command]
pub fn get_popup_pinned(app: AppHandle) -> bool {
    pinned(&app)
}
#[tauri::command]
pub fn show_attached_menu(
    app: AppHandle,
    anchor: Anchor,
    width: f64,
    height: f64,
    menu: AttachedMenu,
    focus: Option<bool>,
) -> Result<MenuLayout, String> {
    show_menu(&app, anchor, width, height, menu, focus.unwrap_or(false))
}
#[tauri::command]
pub fn hide_attached_menu(app: AppHandle) -> Result<(), String> {
    hide_menu(&app, true)
}
#[tauri::command]
pub fn get_attached_menu(app: AppHandle) -> Option<AttachedMenu> {
    current_menu(&app)
}
#[tauri::command]
pub fn attached_menu_action(
    app: AppHandle,
    menu_id: String,
    action_id: String,
) -> Result<(), String> {
    menu_action(&app, &menu_id, &action_id)
}
#[tauri::command]
pub fn attached_menu_hover(app: AppHandle, menu_id: String, hovered: bool) -> Result<(), String> {
    menu_hover(&app, &menu_id, hovered)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn attached_menu_rejects_ambiguous_primary_and_secondary_actions() {
        let mut menu: AttachedMenu = serde_json::from_value(serde_json::json!({"id":"nodes","title":"Nodes","items":[{"id":"select-a","label":"A","secondaryId":"test-a"},{"id":"select-b","label":"B"}]})).unwrap();
        assert!(validate_menu(&menu).is_ok());
        menu.items[1].id = "test-a".into();
        assert!(validate_menu(&menu).is_err());
    }
}
