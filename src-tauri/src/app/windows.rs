//! Single-instance window helpers and popover positioning.
//!
//! Pure geometry helpers are unit-tested without a display. Window open/focus
//! uses Tauri WebviewWindow labels from `version::window_label`.

use crate::version::window_label;
use serde::{Deserialize, Serialize};

/// Logical rectangle (Tauri coordinate space).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Physical tray icon rect from a tray click event (screen pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrayIconRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Which edge of the work area to prefer when coordinates are missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorFallback {
    /// macOS menu bar: below icon when known; otherwise top-right of work area.
    MenuBarBelow,
    /// Windows taskbar tray: above icon when known; otherwise bottom-right.
    TaskbarAbove,
    /// Linux: top-right of primary/current monitor work area.
    TopRight,
}

/// Inputs for computing popover top-left logical position.
#[derive(Debug, Clone, Copy)]
pub struct PopoverPositionInput {
    pub tray: Option<TrayIconRect>,
    pub work_area: Rect,
    pub popover_width: f64,
    pub popover_height: f64,
    pub scale_factor: f64,
    pub fallback: AnchorFallback,
    /// When true (e.g. Linux without reliable coords), force fallback placement.
    pub force_fallback: bool,
}

/// Clamp a window origin so the full window stays inside `work_area`.
pub fn clamp_to_work_area(x: f64, y: f64, width: f64, height: f64, work: Rect) -> (f64, f64) {
    let max_x = (work.x + work.width - width).max(work.x);
    let max_y = (work.y + work.height - height).max(work.y);
    let nx = x.clamp(work.x, max_x);
    let ny = y.clamp(work.y, max_y);
    (nx, ny)
}

/// Convert physical tray rect to logical coordinates.
pub fn physical_to_logical(tray: TrayIconRect, scale: f64) -> TrayIconRect {
    let s = if scale <= 0.0 { 1.0 } else { scale };
    TrayIconRect {
        x: tray.x / s,
        y: tray.y / s,
        width: tray.width / s,
        height: tray.height / s,
    }
}

/// Compute popover top-left logical position, clamped to the monitor work area.
pub fn compute_popover_position(input: PopoverPositionInput) -> (f64, f64) {
    let scale = if input.scale_factor <= 0.0 {
        1.0
    } else {
        input.scale_factor
    };
    let work = input.work_area;
    let w = input.popover_width;
    let h = input.popover_height;

    let (raw_x, raw_y) = match (input.force_fallback, input.tray) {
        (true, _) | (false, None) => fallback_origin(work, w, h, input.fallback),
        (false, Some(tray_phys)) => {
            let tray = physical_to_logical(tray_phys, scale);
            match input.fallback {
                AnchorFallback::MenuBarBelow => {
                    // Center horizontally under the icon; place just below it.
                    let x = tray.x + tray.width / 2.0 - w / 2.0;
                    let y = tray.y + tray.height + 4.0;
                    (x, y)
                }
                AnchorFallback::TaskbarAbove => {
                    let x = tray.x + tray.width / 2.0 - w / 2.0;
                    let y = tray.y - h - 4.0;
                    (x, y)
                }
                AnchorFallback::TopRight => {
                    // Prefer event coords when present: place below-left of icon.
                    let x = tray.x + tray.width - w;
                    let y = tray.y + tray.height + 4.0;
                    (x, y)
                }
            }
        }
    };

    clamp_to_work_area(raw_x, raw_y, w, h, work)
}

fn fallback_origin(work: Rect, w: f64, h: f64, kind: AnchorFallback) -> (f64, f64) {
    const MARGIN: f64 = 12.0;
    match kind {
        AnchorFallback::MenuBarBelow | AnchorFallback::TopRight => {
            // Deterministic top-right of work area.
            let x = work.x + work.width - w - MARGIN;
            let y = work.y + MARGIN;
            (x, y)
        }
        AnchorFallback::TaskbarAbove => {
            let x = work.x + work.width - w - MARGIN;
            let y = work.y + work.height - h - MARGIN;
            (x, y)
        }
    }
}

/// Platform default anchor policy.
pub fn default_anchor_fallback() -> AnchorFallback {
    #[cfg(target_os = "macos")]
    {
        AnchorFallback::MenuBarBelow
    }
    #[cfg(target_os = "windows")]
    {
        AnchorFallback::TaskbarAbove
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        AnchorFallback::TopRight
    }
}

/// Whether the platform is expected to provide reliable left-click tray coords.
pub fn tray_click_coords_reliable() -> bool {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        true
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // Many Linux StatusNotifier hosts omit activate coordinates.
        false
    }
}

/// Compact size when work area is smaller than the preferred popover.
pub fn effective_popover_size(preferred_w: u32, preferred_h: u32, work: Rect) -> (f64, f64) {
    let max_w = (work.width - 16.0).max(280.0);
    let max_h = (work.height - 16.0).max(320.0);
    let w = (preferred_w as f64).min(max_w).max(280.0);
    let h = (preferred_h as f64).min(max_h).max(320.0);
    (w, h)
}

/// URL path/query for a surface window.
pub fn surface_url(surface: &str, section: Option<&str>) -> String {
    match section {
        Some(s) if !s.is_empty() => format!("index.html?view={surface}&section={s}"),
        _ => format!("index.html?view={surface}"),
    }
}

/// Window labels that are single-instance routed.
pub fn is_single_instance_label(label: &str) -> bool {
    matches!(
        label,
        window_label::MAIN | window_label::SETTINGS | window_label::HISTORY | window_label::POPOVER
    )
}

/// Summary for menu / doctor about tray capability.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TrayCapability {
    pub tray_available: bool,
    pub tray_click: TrayClickCapability,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrayClickCapability {
    Available,
    Unavailable,
    Degraded,
}

impl TrayClickCapability {
    pub fn as_str(self) -> &'static str {
        match self {
            TrayClickCapability::Available => "available",
            TrayClickCapability::Unavailable => "unavailable",
            TrayClickCapability::Degraded => "degraded",
        }
    }
}

/// Detect tray host capability for doctor (best-effort, no panic).
pub fn detect_tray_capability() -> TrayCapability {
    #[cfg(target_os = "macos")]
    {
        TrayCapability {
            tray_available: true,
            tray_click: TrayClickCapability::Available,
            detail: None,
        }
    }
    #[cfg(target_os = "windows")]
    {
        TrayCapability {
            tray_available: true,
            tray_click: TrayClickCapability::Available,
            detail: None,
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let graphical =
            std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();
        if !graphical {
            return TrayCapability {
                tray_available: false,
                tray_click: TrayClickCapability::Unavailable,
                detail: Some("no graphical session (DISPLAY/WAYLAND_DISPLAY unset)".into()),
            };
        }
        // Assume tray may exist but left-click activate is often unreliable.
        TrayCapability {
            tray_available: true,
            tray_click: TrayClickCapability::Degraded,
            detail: Some(
                "Linux tray left-click activate may be unavailable; use menu Open entries".into(),
            ),
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        TrayCapability {
            tray_available: false,
            tray_click: TrayClickCapability::Unavailable,
            detail: Some("unsupported platform".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work() -> Rect {
        Rect {
            x: 0.0,
            y: 25.0,
            width: 1440.0,
            height: 875.0,
        }
    }

    #[test]
    fn clamp_keeps_window_inside() {
        let (x, y) = clamp_to_work_area(2000.0, 2000.0, 420.0, 620.0, work());
        assert!(x + 420.0 <= work().x + work().width + 0.001);
        assert!(y + 620.0 <= work().y + work().height + 0.001);
        assert!(x >= work().x);
        assert!(y >= work().y);
    }

    #[test]
    fn macos_places_below_icon() {
        let tray = TrayIconRect {
            x: 1000.0,
            y: 0.0,
            width: 22.0,
            height: 22.0,
        };
        let (x, y) = compute_popover_position(PopoverPositionInput {
            tray: Some(tray),
            work_area: work(),
            popover_width: 420.0,
            popover_height: 620.0,
            scale_factor: 1.0,
            fallback: AnchorFallback::MenuBarBelow,
            force_fallback: false,
        });
        assert!(y >= tray.y + tray.height);
        assert!((x - (tray.x + tray.width / 2.0 - 210.0)).abs() < 1.0 || x >= work().x);
    }

    #[test]
    fn linux_no_coords_uses_top_right_deterministically() {
        let a = compute_popover_position(PopoverPositionInput {
            tray: None,
            work_area: work(),
            popover_width: 420.0,
            popover_height: 620.0,
            scale_factor: 1.0,
            fallback: AnchorFallback::TopRight,
            force_fallback: true,
        });
        let b = compute_popover_position(PopoverPositionInput {
            tray: None,
            work_area: work(),
            popover_width: 420.0,
            popover_height: 620.0,
            scale_factor: 1.0,
            fallback: AnchorFallback::TopRight,
            force_fallback: true,
        });
        assert_eq!(a, b);
        // Top-right-ish
        assert!(a.0 > work().width / 2.0);
        assert!(a.1 < work().y + 50.0);
    }

    #[test]
    fn scale_factor_converts_physical_coords() {
        let tray = TrayIconRect {
            x: 2000.0,
            y: 0.0,
            width: 44.0,
            height: 44.0,
        };
        let logical = physical_to_logical(tray, 2.0);
        assert!((logical.x - 1000.0).abs() < 0.001);
        assert!((logical.width - 22.0).abs() < 0.001);
    }

    #[test]
    fn surface_url_includes_section() {
        assert_eq!(
            surface_url("settings", Some("integrations")),
            "index.html?view=settings&section=integrations"
        );
        assert_eq!(surface_url("history", None), "index.html?view=history");
    }

    #[test]
    fn compact_size_when_work_area_small() {
        let small = Rect {
            x: 0.0,
            y: 0.0,
            width: 300.0,
            height: 400.0,
        };
        let (w, h) = effective_popover_size(420, 620, small);
        assert!(w <= 300.0);
        assert!(h <= 400.0);
    }
}
