//! The window layout: which panes show and how wide they are. Saved in the
//! browser storage of AgentKnife's window, so it survives restarts.

use serde::{Deserialize, Serialize};

/// Simple mode (the default) shows only the preview and the chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Simple,
    Full,
}

/// The center pane's tab (full mode only; simple mode always shows the preview).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CenterTab {
    #[default]
    Preview,
    Code,
}

/// The size the preview shows the app at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Device {
    /// The whole preview area.
    #[default]
    Computer,
    Tablet,
    Phone,
}

impl Device {
    pub const ALL: [Device; 3] = [Device::Computer, Device::Tablet, Device::Phone];

    /// The page's CSS size; `None` fills the preview area.
    pub fn size(self) -> Option<(f64, f64)> {
        match self {
            Device::Computer => None,
            Device::Tablet => Some((820.0, 1180.0)),
            Device::Phone => Some((390.0, 844.0)),
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Device::Computer => "computer",
            Device::Tablet => "tablet",
            Device::Phone => "phone",
        }
    }
}

/// How much a device frame of `size` must shrink to fit in `area` (never grows).
pub fn fit_scale(size: (f64, f64), area: (f64, f64)) -> f64 {
    let (w, h) = size;
    let (aw, ah) = area;
    if w <= 0.0 || h <= 0.0 || aw <= 0.0 || ah <= 0.0 {
        return 1.0;
    }
    (aw / w).min(ah / h).min(1.0)
}

/// Pixel limits of a resizable pane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: f64,
    pub default: f64,
    pub max: f64,
}

impl Bounds {
    pub fn clamp(self, px: f64) -> f64 {
        if px.is_finite() {
            px.clamp(self.min, self.max)
        } else {
            self.default
        }
    }
}

/// Files + timeline on the left.
pub const LEFT: Bounds = Bounds {
    min: 180.0,
    default: 260.0,
    max: 480.0,
};
/// The chat on the right.
pub const CHAT: Bounds = Bounds {
    min: 300.0,
    default: 380.0,
    max: 640.0,
};
/// Arrow keys on a splitter move it this far.
pub const KEY_STEP: f64 = 16.0;

pub const STORAGE_KEY: &str = "agentknife.layout.v1";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub mode: Mode,
    pub tab: CenterTab,
    pub device: Device,
    /// Widths in CSS pixels.
    pub left: f64,
    pub chat: f64,
}

impl Default for Layout {
    fn default() -> Self {
        Layout {
            mode: Mode::Simple,
            tab: CenterTab::Preview,
            device: Device::Computer,
            left: LEFT.default,
            chat: CHAT.default,
        }
    }
}

impl Layout {
    /// A saved layout; anything unreadable gives the defaults.
    pub fn from_json(text: &str) -> Layout {
        serde_json::from_str::<Layout>(text)
            .unwrap_or_default()
            .clamped()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn clamped(self) -> Layout {
        Layout {
            left: LEFT.clamp(self.left),
            chat: CHAT.clamp(self.chat),
            ..self
        }
    }
}

/// The saved layout (defaults outside the browser or when nothing is saved).
pub fn load() -> Layout {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(text) = storage().and_then(|s| s.get_item(STORAGE_KEY).ok().flatten()) {
            return Layout::from_json(&text);
        }
    }
    Layout::default()
}

/// Saves the layout. Best effort: private windows may refuse storage.
pub fn save(layout: &Layout) {
    #[cfg(target_arch = "wasm32")]
    if let Some(s) = storage() {
        let _ = s.set_item(STORAGE_KEY, &layout.to_json());
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = layout;
}

#[cfg(target_arch = "wasm32")]
fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_mode_is_the_default() {
        assert_eq!(Layout::default().mode, Mode::Simple);
        assert_eq!(Layout::from_json(""), Layout::default());
        assert_eq!(Layout::from_json("not json"), Layout::default());
    }

    #[test]
    fn saved_layouts_round_trip_and_are_clamped() {
        let l = Layout {
            mode: Mode::Full,
            tab: CenterTab::Code,
            device: Device::Phone,
            left: 300.0,
            chat: 500.0,
        };
        assert_eq!(Layout::from_json(&l.to_json()), l);
        let wild = Layout::from_json(r#"{"mode":"full","left":5,"chat":99999}"#);
        assert_eq!(wild.mode, Mode::Full);
        assert_eq!(wild.tab, CenterTab::Preview);
        assert_eq!((wild.left, wild.chat), (LEFT.min, CHAT.max));
        // Unknown values fall back as a whole, never half-applied.
        assert_eq!(Layout::from_json(r#"{"mode":"tiny"}"#), Layout::default());
        assert_eq!(LEFT.clamp(f64::NAN), LEFT.default);
        // Saved before devices existed: the computer view.
        assert_eq!(
            Layout::from_json(r#"{"mode":"full"}"#).device,
            Device::Computer
        );
    }

    #[test]
    fn device_frames_shrink_to_fit_but_never_grow() {
        let phone = Device::Phone.size().unwrap_or_default();
        assert_eq!(fit_scale(phone, (1000.0, 2000.0)), 1.0);
        assert!((fit_scale(phone, (390.0, 422.0)) - 0.5).abs() < 1e-9);
        assert_eq!(fit_scale(phone, (0.0, 0.0)), 1.0);
        assert!(Device::Computer.size().is_none());
    }
}
