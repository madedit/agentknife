//! Leptos + Tailwind component kit used by generated apps.
//!
//! Every component takes the caller's `ak_id` and renders it as `data-ak-id`, so
//! the user can point at it in the preview and the agent can find it in code.
//! Parts inside a component get derived ids (`<ak_id>-close`, `<ak_id>-<key>`).
//! Components contain no fixed text: labels come from the caller, in the app's
//! language.
//!
//! Styling is Tailwind v4 utility classes written out in full here; the app's
//! stylesheet must include these sources (`@source "…/ak-ui/src";`). Each part
//! also carries a fixed `ak-…` marker class (`ak-button`, `ak-button-primary`,
//! `ak-nav-tab`, …): a plain rule for it in the app's stylesheet restyles every
//! one, since unlayered CSS wins over Tailwind's utility layer.

#![forbid(unsafe_code)]

mod button;
mod card;
mod dialog;
mod form;
mod input;
mod list;
mod nav;
mod toast;
mod toggle;

pub use button::{Button, ButtonVariant};
pub use card::Card;
pub use dialog::Dialog;
pub use form::Form;
pub use input::Input;
pub use list::List;
pub use nav::{Nav, NavItem};
pub use toast::{ToastHost, ToastKind, Toaster, use_toaster};
pub use toggle::Toggle;

/// A component's own classes, then the caller's `class` prop.
fn classes(own: &str, extra: &str) -> String {
    if extra.trim().is_empty() {
        own.to_owned()
    } else {
        format!("{own} {}", extra.trim())
    }
}

/// `<base>-<part>`: the id of a part inside a component. Spaces and signs in
/// `part` become single `-` (`"New York"` → `New-York`, `-6` → `minus-6`), so
/// any key gives a readable id.
pub fn part_id(base: &str, part: impl std::fmt::Display) -> String {
    let raw = part.to_string();
    let mut clean = String::new();
    if raw.starts_with('-') && raw[1..].starts_with(|c: char| c.is_ascii_digit()) {
        clean.push_str("minus-");
    }
    for c in raw.chars() {
        if c.is_alphanumeric() || c == '_' {
            clean.push(c);
        } else if !clean.is_empty() && !clean.ends_with('-') {
            clean.push('-');
        }
    }
    let clean = clean.trim_end_matches('-');
    format!("{base}-{}", if clean.is_empty() { "item" } else { clean })
}

#[cfg(test)]
mod tests {
    use super::{classes, part_id};

    #[test]
    fn the_callers_classes_come_last() {
        assert_eq!(classes("rounded p-4", ""), "rounded p-4");
        assert_eq!(classes("rounded p-4", "  "), "rounded p-4");
        assert_eq!(
            classes("rounded p-4", " w-full bg-slate-800! "),
            "rounded p-4 w-full bg-slate-800!"
        );
    }

    #[test]
    fn keys_become_readable_ids() {
        assert_eq!(part_id("choose", -6), "choose-minus-6");
        assert_eq!(part_id("choose", 6), "choose-6");
        assert_eq!(part_id("city", "New York"), "city-New-York");
        assert_eq!(part_id("photo", "a.jpg"), "photo-a-jpg");
        assert_eq!(part_id("city", "台北"), "city-台北");
        assert_eq!(part_id("row", " / "), "row-item");
    }
}
