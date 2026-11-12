//! Placing the correction widget: centered on the monitor the cursor is on.
//!
//! On macOS, tao reports each monitor's rectangle in *its own* physical pixels (points times
//! that monitor's scale), so rectangles of a Retina and a non-Retina display don't share a
//! coordinate space and can overlap. The cursor and window positions are in points there, so
//! all math is done in points on macOS. Elsewhere, physical pixels are one consistent space.

use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, Monitor, PhysicalPosition, WebviewWindow,
};

pub const LABEL: &str = "widget";
pub const WIDTH: f64 = 640.0;
pub const HEIGHT: f64 = 160.0;
/// The widget never grows past this share of the monitor's usable height.
const MAX_HEIGHT_SHARE: f64 = 0.8;

const IN_POINTS: bool = cfg!(target_os = "macos");

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    fn distance_squared(&self, x: f64, y: f64) -> f64 {
        let dx = (self.x - x).max(0.0).max(x - (self.x + self.width));
        let dy = (self.y - y).max(0.0).max(y - (self.y + self.height));
        dx * dx + dy * dy
    }
}

/// Index of the area containing the point, or of the nearest one.
pub fn pick(areas: &[Rect], x: f64, y: f64) -> Option<usize> {
    areas.iter().position(|a| a.contains(x, y)).or_else(|| {
        (0..areas.len()).min_by(|&i, &j| {
            areas[i]
                .distance_squared(x, y)
                .total_cmp(&areas[j].distance_squared(x, y))
        })
    })
}

/// Top-left corner that centers a `width`×`height` window in `area`, kept inside it.
pub fn center_in(area: Rect, width: f64, height: f64) -> (f64, f64) {
    let x = area.x + ((area.width - width) / 2.0).max(0.0);
    let y = area.y + ((area.height - height) / 2.0).max(0.0);
    (x.round(), y.round())
}

/// A monitor's usable area (without menu bar, Dock or taskbar) and scale factor, in the
/// coordinate space of the cursor and of window positions.
fn usable_area(monitor: &Monitor) -> (Rect, f64) {
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let rect = if IN_POINTS {
        let pos = area.position.to_logical::<f64>(scale);
        let size = area.size.to_logical::<f64>(scale);
        Rect {
            x: pos.x,
            y: pos.y,
            width: size.width,
            height: size.height,
        }
    } else {
        Rect {
            x: area.position.x as f64,
            y: area.position.y as f64,
            width: area.size.width as f64,
            height: area.size.height as f64,
        }
    };
    (rect, scale)
}

/// Where the mouse is, in points on macOS and physical pixels elsewhere.
pub fn cursor_position() -> Option<(f64, f64)> {
    use enigo::{Enigo, Mouse, Settings};
    let enigo = Enigo::new(&Settings::default()).ok()?;
    enigo.location().ok().map(|(x, y)| (x as f64, y as f64))
}

/// Sizes the window to `WIDTH`×`height` logical pixels and centers it in `area`.
fn place(window: &WebviewWindow, area: Rect, scale: f64, height: f64) -> tauri::Result<()> {
    let ratio = if IN_POINTS { 1.0 } else { scale };
    let max_height = (area.height / ratio * MAX_HEIGHT_SHARE).max(HEIGHT);
    let height = height.clamp(HEIGHT, max_height);
    let (x, y) = center_in(area, WIDTH * ratio, height * ratio);
    let move_to = |window: &WebviewWindow| {
        if IN_POINTS {
            window.set_position(LogicalPosition::new(x, y))
        } else {
            window.set_position(PhysicalPosition::new(x as i32, y as i32))
        }
    };
    // Move first, so the logical size is converted with the target monitor's scale; then move
    // again in case the platform anchored the resize elsewhere.
    move_to(window)?;
    window.set_size(LogicalSize::new(WIDTH, height))?;
    move_to(window)
}

/// Shows the widget at its default size, centered on the monitor under `cursor`.
pub fn show_centered(app: &AppHandle, cursor: Option<(f64, f64)>) -> Result<(), String> {
    let window = app
        .get_webview_window(LABEL)
        .ok_or_else(|| "widget window not found".to_string())?;
    let monitors = app.available_monitors().unwrap_or_default();
    let areas: Vec<(Rect, f64)> = monitors.iter().map(usable_area).collect();
    let rects: Vec<Rect> = areas.iter().map(|(r, _)| *r).collect();
    // The primary monitor holds the origin on every platform.
    let (x, y) = cursor.unwrap_or((0.0, 0.0));
    if let Some(i) = pick(&rects, x, y) {
        let (area, scale) = areas[i];
        place(&window, area, scale, HEIGHT).map_err(|e| e.to_string())?;
    }
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())
}

/// Resizes the widget to fit its content and keeps it centered on its monitor.
pub fn fit(window: &WebviewWindow, height: f64) -> Result<(), String> {
    let monitor = window
        .current_monitor()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "the widget is not on a monitor".to_string())?;
    let (area, scale) = usable_area(&monitor);
    place(window, area, scale, height).map_err(|e| e.to_string())
}

/// Hides the widget and puts it back to its default size.
pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
        let _ = window.set_size(LogicalSize::new(WIDTH, HEIGHT));
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

    #[test]
    fn picks_the_monitor_under_the_cursor() {
        // MacBook (points) with an external display to its right, as macOS reports them.
        let laptop = rect(0.0, 25.0, 1512.0, 920.0);
        let external = rect(1512.0, -300.0, 2560.0, 1415.0);
        let areas = [laptop, external];
        assert_eq!(pick(&areas, 700.0, 500.0), Some(0));
        assert_eq!(pick(&areas, 2000.0, 500.0), Some(1));
        assert_eq!(pick(&areas, 2000.0, -200.0), Some(1));
    }

    #[test]
    fn the_old_mixed_pixel_space_picked_the_wrong_monitor() {
        // What the previous code compared against: the laptop's rectangle in its own 2× pixels
        // overlaps the external display, so a cursor on the external matched the laptop.
        let laptop_physical = rect(0.0, 0.0, 3024.0, 1964.0);
        let external_physical = rect(1512.0, 0.0, 2560.0, 1440.0);
        assert_eq!(
            pick(&[laptop_physical, external_physical], 2000.0, 500.0),
            Some(0)
        );
        // In points, the same cursor lands on the external display.
        let laptop = rect(0.0, 0.0, 1512.0, 982.0);
        let external = rect(1512.0, 0.0, 2560.0, 1440.0);
        assert_eq!(pick(&[laptop, external], 2000.0, 500.0), Some(1));
    }

    #[test]
    fn falls_back_to_the_nearest_monitor() {
        let areas = [rect(0.0, 0.0, 100.0, 100.0), rect(200.0, 0.0, 100.0, 100.0)];
        assert_eq!(pick(&areas, 180.0, 50.0), Some(1));
        assert_eq!(pick(&areas, -50.0, 50.0), Some(0));
        assert_eq!(pick(&[], 0.0, 0.0), None);
    }

    #[test]
    fn centers_in_the_area() {
        assert_eq!(
            center_in(rect(1512.0, 25.0, 2560.0, 1415.0), 640.0, 160.0),
            (2472.0, 653.0)
        );
    }

    #[test]
    fn stays_inside_an_area_smaller_than_the_window() {
        assert_eq!(
            center_in(rect(100.0, 50.0, 300.0, 100.0), 640.0, 160.0),
            (100.0, 50.0)
        );
    }
}
