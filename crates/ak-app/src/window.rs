//! Remembers AgentKnife's window between starts: maximized or not, and its
//! size and place as a normal window (`<data>/AgentKnife/window.json`).

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, Runtime, WindowEvent};

use crate::state::AppState;

/// The window `tauri.conf.json` opens (hidden until it is in place).
const MAIN: &str = "main";
const FILE: &str = "window.json";
/// How long the window stays still before its place is read.
const SETTLE: Duration = Duration::from_millis(300);

/// The window as a normal window (physical pixels: its outer corner and the
/// size of its page), and whether it was maximized.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

impl Place {
    pub fn load(dir: &Path) -> Option<Place> {
        let text = std::fs::read_to_string(dir.join(FILE)).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn save(&self, dir: &Path) -> anyhow::Result<()> {
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!("{FILE}.tmp"));
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, dir.join(FILE))?;
        Ok(())
    }

    /// Whether enough of the title bar is on one of `screens` (x, y, width,
    /// height) to grab it: a screen unplugged or a resolution lowered since
    /// must not leave the window out of reach. Windows' invisible borders put
    /// a window at the screen's edge a few pixels outside it.
    pub fn reachable(&self, screens: &[(i32, i32, u32, u32)]) -> bool {
        const GRAB_WIDTH: i64 = 100;
        const BAR: i64 = 30;
        const BORDER: i64 = 16;
        let (left, top) = (i64::from(self.x), i64::from(self.y));
        let right = left + i64::from(self.width);
        screens.iter().any(|&(x, y, w, h)| {
            let (sx, sy) = (i64::from(x), i64::from(y));
            let wide = right.min(sx + i64::from(w)) - left.max(sx);
            wide >= GRAB_WIDTH && top >= sy - BORDER && top + BAR <= sy + i64::from(h)
        })
    }
}

/// Puts the window where it was (when that is still on a screen), shows it,
/// and from then on remembers where it is, saving that when it closes.
pub fn restore<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(MAIN) else {
        return;
    };
    let dir = app.state::<AppState>().state_dir.clone();
    let screens: Vec<(i32, i32, u32, u32)> = window
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let area = m.work_area();
            let (p, s) = (area.position, area.size);
            (p.x, p.y, s.width, s.height)
        })
        .collect();
    let saved = Place::load(&dir);
    // Maximized comes back even when the normal place is off the screens.
    let normal = saved.filter(|p| p.reachable(&screens));
    if let Some(p) = normal {
        let _ = window.set_position(PhysicalPosition::new(p.x, p.y));
        let _ = window.set_size(PhysicalSize::new(p.width, p.height));
    }
    if saved.is_some_and(|p| p.maximized) {
        let _ = window.maximize();
    }
    let _ = window.show();

    // The normal place, kept while maximized (so it comes back as it was).
    let normal = Arc::new(Mutex::new(normal.or_else(|| normal_now(&window))));
    let moves = Arc::new(AtomicU64::new(0));
    let w = window.clone();
    window.on_window_event(move |event| match event {
        // While the window maximizes it moves and grows before it says it is
        // maximized: read the place once it has been still for a moment.
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
            let this = moves.fetch_add(1, Ordering::SeqCst) + 1;
            let (w, normal, moves) = (w.clone(), normal.clone(), moves.clone());
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(SETTLE).await;
                if moves.load(Ordering::SeqCst) == this
                    && let Some(now) = normal_now(&w)
                {
                    *normal.lock().unwrap_or_else(|e| e.into_inner()) = Some(now);
                }
            });
        }
        WindowEvent::CloseRequested { .. } => {
            let maximized = w.is_maximized().unwrap_or(false);
            let last = *normal.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(p) = last.or_else(|| current(&w)) {
                let _ = Place { maximized, ..p }.save(&dir);
            }
        }
        _ => {}
    });
}

/// Where the window is now, when it is a normal window (not maximized or
/// minimized).
fn normal_now<R: Runtime>(window: &tauri::WebviewWindow<R>) -> Option<Place> {
    let odd = window.is_maximized().unwrap_or(true) || window.is_minimized().unwrap_or(true);
    if odd { None } else { current(window) }
}

/// Where the window is now.
fn current<R: Runtime>(window: &tauri::WebviewWindow<R>) -> Option<Place> {
    let at = window.outer_position().ok()?;
    let size = window.inner_size().ok()?;
    (size.width > 0 && size.height > 0).then_some(Place {
        x: at.x,
        y: at.y,
        width: size.width,
        height: size.height,
        maximized: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLACE: Place = Place {
        x: 100,
        y: 80,
        width: 1400,
        height: 900,
        maximized: false,
    };

    #[test]
    fn a_window_comes_back_only_where_it_can_be_grabbed() {
        let laptop = (0, 0, 1920, 1040);
        let right = (1920, 0, 2560, 1400);
        assert!(PLACE.reachable(&[laptop]));
        // On the second screen, now unplugged.
        let there = Place { x: 2200, ..PLACE };
        assert!(there.reachable(&[laptop, right]));
        assert!(!there.reachable(&[laptop]));
        // Only its right edge still on screen, or its title bar above it.
        assert!(!Place { x: -1350, ..PLACE }.reachable(&[laptop]));
        assert!(!Place { y: -20, ..PLACE }.reachable(&[laptop]));
        // At the screen's edge, its invisible borders just outside.
        assert!(
            Place {
                x: -7,
                y: -7,
                ..PLACE
            }
            .reachable(&[laptop])
        );
        assert!(!PLACE.reachable(&[]));
    }

    #[test]
    fn the_place_is_saved_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Place::load(dir.path()), None);
        let p = Place {
            maximized: true,
            ..PLACE
        };
        p.save(dir.path()).unwrap();
        assert_eq!(Place::load(dir.path()), Some(p));
        std::fs::write(dir.path().join(FILE), "not json").unwrap();
        assert_eq!(Place::load(dir.path()), None);
    }
}
