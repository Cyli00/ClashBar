//! Event ordering for the tray shell, independent of native window APIs.
#[derive(Debug, Default)]
pub struct PopupStateMachine {
    pub visible: bool,
    pub pinned: bool,
    dialogs: usize,
    revision: u64,
    pressed_target: Option<bool>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum BlurAction {
    Keep,
    CloseMenu,
    CloseAll,
}

impl PopupStateMachine {
    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn show(&mut self) {
        self.visible = true;
        self.pressed_target = None;
        self.changed();
    }

    pub fn hide(&mut self) -> bool {
        if self.dialogs > 0 {
            return false;
        }
        self.visible = false;
        self.pressed_target = None;
        self.changed();
        true
    }

    pub fn pin(&mut self, pinned: bool) {
        self.pinned = pinned;
        self.changed();
    }
    pub fn focus_gained(&mut self) {
        self.changed();
    }
    pub fn begin_dialog(&mut self) {
        self.dialogs += 1;
        self.changed();
    }
    pub fn end_dialog(&mut self) -> bool {
        self.dialogs = self.dialogs.saturating_sub(1);
        self.changed();
        self.visible && self.dialogs == 0
    }

    /// Remember intent on mouse-down, before Windows may activate the taskbar.
    pub fn tray_pressed(&mut self) {
        if self.dialogs == 0 {
            self.pressed_target = Some(!self.visible);
        }
        self.changed();
    }

    pub fn tray_released(&mut self) -> Option<bool> {
        self.changed();
        if self.dialogs > 0 {
            self.pressed_target = None;
            return None;
        }
        Some(self.pressed_target.take().unwrap_or(!self.visible))
    }

    pub fn blur_ticket(&mut self) -> Option<u64> {
        if !self.visible || self.dialogs > 0 || self.pressed_target.is_some() {
            return None;
        }
        self.changed();
        Some(self.revision)
    }

    /// A delayed blur is valid only until focus, a dialog, or tray intent changes.
    pub fn resolve_blur(
        &mut self,
        ticket: u64,
        owned_focus: bool,
        cursor_on_tray: bool,
    ) -> BlurAction {
        if ticket != self.revision
            || !self.visible
            || self.dialogs > 0
            || self.pressed_target.is_some()
            || owned_focus
            || cursor_on_tray
        {
            return BlurAction::Keep;
        }
        if self.pinned {
            return BlurAction::CloseMenu;
        }
        self.hide();
        BlurAction::CloseAll
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blur_before_tray_click_cannot_reopen_the_visible_popup() {
        let mut state = PopupStateMachine::default();
        state.show();
        let blur = state.blur_ticket().unwrap();
        state.tray_pressed();
        assert_eq!(state.resolve_blur(blur, false, true), BlurAction::Keep);
        assert_eq!(state.tray_released(), Some(false));
        state.hide();
        assert!(!state.visible);
    }
    #[test]
    fn long_tray_press_and_late_blur_keep_original_toggle_intent() {
        let mut state = PopupStateMachine::default();
        state.show();
        state.tray_pressed();
        assert!(state.blur_ticket().is_none());
        assert_eq!(state.tray_released(), Some(false));
        state.hide();
        assert!(state.blur_ticket().is_none());
        state.tray_pressed();
        assert_eq!(state.tray_released(), Some(true));
    }
    #[test]
    fn owned_submenu_focus_and_native_dialogs_do_not_dismiss_parent() {
        let mut state = PopupStateMachine::default();
        state.show();
        let ticket = state.blur_ticket().unwrap();
        assert_eq!(state.resolve_blur(ticket, true, false), BlurAction::Keep);
        state.begin_dialog();
        assert!(!state.hide());
        assert!(state.blur_ticket().is_none());
        state.tray_pressed();
        assert_eq!(state.tray_released(), None);
        assert!(state.end_dialog());
        assert!(state.visible);
    }
    #[test]
    fn pin_only_prevents_outside_dismissal() {
        let mut state = PopupStateMachine::default();
        state.show();
        state.pin(true);
        let ticket = state.blur_ticket().unwrap();
        assert_eq!(
            state.resolve_blur(ticket, false, false),
            BlurAction::CloseMenu
        );
        assert!(state.visible);
        assert!(state.hide());
        assert!(!state.visible);
    }
    #[test]
    fn stale_blur_after_focus_or_reopen_is_ignored() {
        let mut state = PopupStateMachine::default();
        state.show();
        let first = state.blur_ticket().unwrap();
        state.focus_gained();
        assert_eq!(state.resolve_blur(first, false, false), BlurAction::Keep);
        let second = state.blur_ticket().unwrap();
        state.hide();
        state.show();
        assert_eq!(state.resolve_blur(second, false, false), BlurAction::Keep);
        let outside = state.blur_ticket().unwrap();
        assert_eq!(
            state.resolve_blur(outside, false, false),
            BlurAction::CloseAll
        );
        assert!(!state.visible);
    }
}
