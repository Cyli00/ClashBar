//! Popup placement in physical desktop coordinates, including negative monitor origins.
//! Requested sizes and padding are logical pixels and are converted using the anchor's scale.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn right(self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(self) -> f64 {
        self.y + self.height
    }

    pub fn center(self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    pub fn contains(self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.right() && y >= self.y && y <= self.bottom()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub rect: Rect,
    /// Maximum content height in logical pixels, for the webview's layout limit.
    pub max_height: f64,
    pub edge: Edge,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttachedPlacement {
    pub rect: Rect,
    pub side: Edge,
}

fn valid_scale(scale: f64) -> f64 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

fn positive_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        fallback
    }
}

// Retain at least one physical pixel even on an unusually small work area.
fn inset(length: f64, preferred: f64) -> f64 {
    preferred.min(((length - 1.0) / 2.0).max(0.0))
}

fn clamp_origin(value: f64, start: f64, end: f64) -> f64 {
    value.max(start).min(end.max(start))
}

fn taskbar_edge(work: Rect, anchor: Rect) -> Edge {
    let (x, y) = anchor.center();
    // A tray in a reserved taskbar lies outside the work area. Checking its center
    // also accommodates the small overlaps reported by some Windows shell themes.
    if y <= work.y {
        return Edge::Top;
    }
    if y >= work.bottom() {
        return Edge::Bottom;
    }
    if x <= work.x {
        return Edge::Left;
    }
    if x >= work.right() {
        return Edge::Right;
    }

    // Auto-hidden taskbars and the overflow tray can report an anchor inside it.
    let distances = [
        (y - work.y, Edge::Top),
        (work.bottom() - y, Edge::Bottom),
        (x - work.x, Edge::Left),
        (work.right() - x, Edge::Right),
    ];
    distances
        .into_iter()
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, edge)| edge)
        .unwrap_or(Edge::Bottom)
}

/// Place the main tray popup against the taskbar-facing edge of the work area.
/// `locked_x`, when present, preserves an existing physical horizontal position.
pub fn main_popup(
    work: Rect,
    anchor: Rect,
    scale: f64,
    requested_height: f64,
    locked_x: Option<f64>,
) -> Placement {
    let scale = valid_scale(scale);
    let edge = taskbar_edge(work, anchor);
    let horizontal_padding = inset(work.width, 8.0 * scale);
    let width = (360.0 * scale)
        .min(work.width - 2.0 * horizontal_padding)
        .max(1.0);
    let side_taskbar = matches!(edge, Edge::Left | Edge::Right);
    let vertical_padding = if side_taskbar {
        inset(work.height, 10.0 * scale)
    } else {
        (10.0 * scale).min((work.height - 1.0).max(0.0))
    };
    let available_height =
        (work.height - vertical_padding * if side_taskbar { 2.0 } else { 1.0 }).max(1.0);
    let max_height = available_height / scale;
    let height =
        (positive_or(requested_height, 320.0).ceil().max(280.0) * scale).min(available_height);
    let (anchor_x, anchor_y) = anchor.center();

    let (preferred_x, min_x, max_x) = match edge {
        Edge::Top | Edge::Bottom => (
            anchor_x - width / 2.0,
            work.x + horizontal_padding,
            work.right() - horizontal_padding - width,
        ),
        Edge::Left => (work.x, work.x, work.right() - width),
        Edge::Right => (work.right() - width, work.x, work.right() - width),
    };
    let x = clamp_origin(
        locked_x.filter(|x| x.is_finite()).unwrap_or(preferred_x),
        min_x,
        max_x,
    );
    let y = match edge {
        Edge::Top => work.y,
        Edge::Bottom => work.bottom() - height,
        Edge::Left | Edge::Right => clamp_origin(
            anchor_y - height / 2.0,
            work.y + vertical_padding,
            work.bottom() - vertical_padding - height,
        ),
    };

    Placement {
        rect: Rect {
            x,
            y,
            width,
            height,
        },
        max_height,
        edge,
    }
}

/// Attach a secondary menu to the full host window, centered on its anchor row.
/// Prefer the right side when it fits, then the left, then whichever has more room.
pub fn attached_menu(
    work: Rect,
    host: Rect,
    anchor: Rect,
    scale: f64,
    width: f64,
    height: f64,
) -> AttachedPlacement {
    let scale = valid_scale(scale);
    let horizontal_padding = inset(work.width, 6.0 * scale);
    let vertical_padding = inset(work.height, 6.0 * scale);
    let preferred_width = (positive_or(width, 1.0).max(1.0) * scale)
        .min(work.width - 2.0 * horizontal_padding)
        .max(1.0);
    let height = (positive_or(height, 40.0).max(40.0) * scale)
        .min(work.height - 2.0 * vertical_padding)
        .max(1.0);
    let right_available = (work.right() - host.right()).max(0.0);
    let left_available = (host.x - work.x).max(0.0);
    let side = if right_available >= preferred_width {
        Edge::Right
    } else if left_available >= preferred_width {
        Edge::Left
    } else if right_available >= left_available {
        Edge::Right
    } else {
        Edge::Left
    };
    let available = match side {
        Edge::Right => right_available,
        _ => left_available,
    };
    // Windows high-DPI collision fallback intentionally differs from AppKit's
    // side-width clipping: overlap the host if fewer than 160 logical pixels fit
    // beside it, retaining a usable menu width within the padded work area.
    let width = if available < 160.0 * scale {
        preferred_width
    } else {
        preferred_width.min(available).max(1.0)
    };
    let raw_x = match side {
        Edge::Right => host.right() - scale,
        _ => host.x - width + scale,
    };
    let x = clamp_origin(
        raw_x,
        work.x + horizontal_padding,
        work.right() - horizontal_padding - width,
    );
    let y = clamp_origin(
        anchor.center().1 - height / 2.0,
        work.y + vertical_padding,
        work.bottom() - vertical_padding - height,
    );
    AttachedPlacement {
        rect: Rect {
            x,
            y,
            width,
            height,
        },
        side,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn assert_inside(work: Rect, panel: Rect) {
        assert!(work.contains(panel.x, panel.y), "{panel:?} in {work:?}");
        assert!(
            work.contains(panel.right(), panel.bottom()),
            "{panel:?} in {work:?}"
        );
    }

    #[test]
    fn rectangle_helpers_support_negative_coordinates_and_boundaries() {
        let area = rect(-200.0, -300.0, 120.0, 80.0);
        assert_eq!(area.right(), -80.0);
        assert_eq!(area.bottom(), -220.0);
        assert_eq!(area.center(), (-140.0, -260.0));
        assert!(area.contains(-200.0, -300.0));
        assert!(area.contains(-80.0, -220.0));
        assert!(!area.contains(-79.0, -220.0));
    }

    #[test]
    fn main_popup_attaches_to_every_taskbar_edge_at_common_dpi_scales() {
        for scale in [1.0, 1.5, 2.0] {
            let work = rect(0.0, 0.0, 1600.0 * scale, 900.0 * scale);
            for (anchor, edge) in [
                (rect(700.0 * scale, -40.0 * scale, 20.0, 20.0), Edge::Top),
                (rect(700.0 * scale, work.bottom(), 20.0, 20.0), Edge::Bottom),
                (rect(-40.0 * scale, 400.0 * scale, 20.0, 20.0), Edge::Left),
                (rect(work.right(), 400.0 * scale, 20.0, 20.0), Edge::Right),
            ] {
                let placement = main_popup(work, anchor, scale, 320.0, None);
                assert_eq!(placement.edge, edge);
                assert_eq!(placement.rect.width, 360.0 * scale);
                assert_eq!(placement.rect.height, 320.0 * scale);
                assert_inside(work, placement.rect);
                match edge {
                    Edge::Top => assert_eq!(placement.rect.y, work.y),
                    Edge::Bottom => assert_eq!(placement.rect.bottom(), work.bottom()),
                    Edge::Left => assert_eq!(placement.rect.x, work.x),
                    Edge::Right => assert_eq!(placement.rect.right(), work.right()),
                }
            }
        }
    }

    #[test]
    fn main_popup_handles_a_monitor_left_and_above_the_primary_display() {
        let work = rect(-2560.0, -1440.0, 2560.0, 1360.0);
        let placement = main_popup(work, rect(-40.0, -80.0, 30.0, 30.0), 2.0, 400.0, None);
        assert_eq!(placement.edge, Edge::Bottom);
        assert_eq!(placement.rect, rect(-736.0, -880.0, 720.0, 800.0));
        assert_eq!(placement.max_height, 670.0);
        assert_inside(work, placement.rect);
    }

    #[test]
    fn oversized_main_popup_preserves_opposite_edge_padding() {
        let work = rect(100.0, 50.0, 900.0, 600.0);
        for (anchor, edge, expected_height) in [
            (rect(500.0, 20.0, 20.0, 20.0), Edge::Top, 580.0),
            (rect(500.0, 650.0, 20.0, 20.0), Edge::Bottom, 580.0),
            (rect(70.0, 500.0, 20.0, 20.0), Edge::Left, 560.0),
            (rect(1000.0, 500.0, 20.0, 20.0), Edge::Right, 560.0),
        ] {
            let placement = main_popup(work, anchor, 2.0, 10000.0, None);
            assert_eq!(placement.edge, edge);
            assert_eq!(placement.rect.height, expected_height);
            assert_eq!(placement.max_height, expected_height / 2.0);
            assert_inside(work, placement.rect);
            match edge {
                Edge::Top => assert_eq!(placement.rect.bottom(), work.bottom() - 20.0),
                Edge::Bottom => assert_eq!(placement.rect.y, work.y + 20.0),
                _ => {
                    assert_eq!(placement.rect.y, work.y + 20.0);
                    assert_eq!(placement.rect.bottom(), work.bottom() - 20.0);
                }
            }
        }
    }

    #[test]
    fn main_height_rounds_up_and_respects_minimum_and_initial_height() {
        let work = rect(0.0, 0.0, 1920.0, 1040.0);
        let anchor = rect(1800.0, 1040.0, 24.0, 24.0);
        for (requested, expected) in [(100.0, 280.0), (320.1, 321.0), (0.0, 320.0)] {
            assert_eq!(
                main_popup(work, anchor, 1.0, requested, None).rect.height,
                expected
            );
        }
    }

    #[test]
    fn main_width_clips_to_narrow_work_area_with_horizontal_padding() {
        let work = rect(-300.0, 0.0, 300.0, 900.0);
        let placement = main_popup(work, rect(-50.0, 900.0, 20.0, 20.0), 1.5, 320.0, None);
        assert_eq!(placement.rect.x, -288.0);
        assert_eq!(placement.rect.width, 276.0);
        assert_inside(work, placement.rect);
    }

    #[test]
    fn resized_main_popup_keeps_locked_horizontal_position_and_clamps_it() {
        let work = rect(0.0, 0.0, 1600.0, 900.0);
        let original = main_popup(work, rect(800.0, 900.0, 20.0, 20.0), 1.0, 320.0, None);
        let resized = main_popup(
            work,
            rect(1400.0, 900.0, 20.0, 20.0),
            1.0,
            700.0,
            Some(original.rect.x),
        );
        assert_eq!(resized.rect.x, original.rect.x);
        assert_eq!(resized.rect.bottom(), original.rect.bottom());
        assert_eq!(
            main_popup(work, work, 1.0, 320.0, Some(-1000.0)).rect.x,
            8.0
        );
        assert_eq!(
            main_popup(work, work, 1.0, 320.0, Some(2000.0)).rect.x,
            1232.0
        );
    }

    #[test]
    fn anchors_inside_work_area_choose_the_nearest_edge() {
        let work = rect(0.0, 0.0, 1600.0, 900.0);
        for (anchor, expected) in [
            (rect(800.0, 0.0, 20.0, 20.0), Edge::Top),
            (rect(800.0, 880.0, 20.0, 20.0), Edge::Bottom),
            (rect(0.0, 400.0, 20.0, 20.0), Edge::Left),
            (rect(1580.0, 400.0, 20.0, 20.0), Edge::Right),
        ] {
            assert_eq!(main_popup(work, anchor, 1.0, 320.0, None).edge, expected);
        }
    }

    #[test]
    fn attached_menu_prefers_right_when_both_sides_fit_and_centers_on_row() {
        let placement = attached_menu(
            rect(0.0, 0.0, 1600.0, 900.0),
            rect(600.0, 300.0, 360.0, 500.0),
            rect(620.0, 500.0, 320.0, 40.0),
            1.0,
            250.0,
            200.0,
        );
        assert_eq!(placement.side, Edge::Right);
        assert_eq!(placement.rect, rect(959.0, 420.0, 250.0, 200.0));
    }

    #[test]
    fn attached_menu_uses_full_host_width_and_falls_back_to_left() {
        let placement = attached_menu(
            rect(0.0, 0.0, 1600.0, 900.0),
            rect(1200.0, 300.0, 360.0, 500.0),
            rect(1220.0, 500.0, 30.0, 40.0),
            1.0,
            250.0,
            200.0,
        );
        assert_eq!(placement.side, Edge::Left);
        assert_eq!(placement.rect, rect(951.0, 420.0, 250.0, 200.0));
    }

    #[test]
    fn attached_menu_clips_to_larger_side_when_neither_side_fits() {
        let work = rect(0.0, 0.0, 720.0, 500.0);
        for (host_x, expected_side, expected_x, expected_width) in [
            (160.0, Edge::Right, 514.0, 200.0),
            (200.0, Edge::Left, 6.0, 200.0),
            (180.0, Edge::Right, 534.0, 180.0),
        ] {
            let placement = attached_menu(
                work,
                rect(host_x, 50.0, 360.0, 400.0),
                rect(host_x, 100.0, 300.0, 40.0),
                1.0,
                250.0,
                200.0,
            );
            assert_eq!(placement.side, expected_side);
            assert_eq!(placement.rect.x, expected_x);
            assert_eq!(placement.rect.width, expected_width);
            assert_inside(work, placement.rect);
        }
    }

    #[test]
    fn cramped_side_space_overlaps_host_without_compressing_menu_controls() {
        for scale in [1.0, 1.5, 2.0] {
            let work = rect(0.0, 0.0, 600.0 * scale, 500.0 * scale);
            for (host_x, expected_side, expected_x) in [
                (90.0, Edge::Right, 344.0),
                (150.0, Edge::Left, 6.0),
                (120.0, Edge::Right, 344.0),
            ] {
                let placement = attached_menu(
                    work,
                    rect(host_x * scale, 50.0 * scale, 360.0 * scale, 400.0 * scale),
                    rect(host_x * scale, 100.0 * scale, 300.0 * scale, 40.0 * scale),
                    scale,
                    250.0,
                    200.0,
                );
                assert_eq!(placement.side, expected_side);
                assert_eq!(placement.rect.x, expected_x * scale);
                assert_eq!(placement.rect.width, 250.0 * scale);
                assert_inside(work, placement.rect);
            }
        }
    }

    #[test]
    fn attached_menu_scales_padding_overlap_and_minimum_height() {
        for scale in [1.5, 2.0] {
            let work = rect(
                -1600.0 * scale,
                -900.0 * scale,
                1600.0 * scale,
                900.0 * scale,
            );
            let host = rect(
                -1000.0 * scale,
                -500.0 * scale,
                360.0 * scale,
                400.0 * scale,
            );
            let placement = attached_menu(
                work,
                host,
                rect(-980.0 * scale, -890.0 * scale, 320.0 * scale, 20.0 * scale),
                scale,
                250.0,
                10.0,
            );
            assert_eq!(placement.side, Edge::Right);
            assert_eq!(placement.rect.x, host.right() - scale);
            assert_eq!(placement.rect.width, 250.0 * scale);
            assert_eq!(placement.rect.height, 40.0 * scale);
            assert_eq!(placement.rect.y, work.y + 6.0 * scale);
            assert_inside(work, placement.rect);
        }
    }

    #[test]
    fn attached_menu_clamps_tall_content_and_bottom_anchor_to_work_area() {
        let work = rect(0.0, 0.0, 1200.0, 900.0);
        let host = rect(200.0, 400.0, 360.0, 400.0);
        let anchor = rect(210.0, 860.0, 320.0, 40.0);
        let tall = attached_menu(work, host, anchor, 2.0, 200.0, 10000.0);
        assert_eq!(tall.rect.height, 876.0);
        assert_eq!(tall.rect.y, 12.0);
        assert_inside(work, tall.rect);
        let short = attached_menu(work, host, anchor, 2.0, 200.0, 100.0);
        assert_eq!(short.rect.bottom(), 888.0);
        assert_inside(work, short.rect);
    }

    #[test]
    fn menus_remain_usable_when_host_spans_the_work_area() {
        for scale in [1.0, 1.5, 2.0] {
            let work = rect(0.0, 0.0, 360.0 * scale, 640.0 * scale);
            let placement = attached_menu(work, work, work, scale, 200.0, 200.0);
            assert_eq!(placement.side, Edge::Right);
            assert_eq!(placement.rect.width, 200.0 * scale);
            assert_eq!(placement.rect.right(), work.right() - 6.0 * scale);
            assert_inside(work, placement.rect);
            let oversized = attached_menu(work, work, work, scale, 1000.0, 200.0);
            assert_eq!(oversized.rect.width, 348.0 * scale);
            assert_eq!(oversized.rect.x, 6.0 * scale);
            assert_inside(work, oversized.rect);
        }
    }
}
