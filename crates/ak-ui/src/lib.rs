//! Leptos + Tailwind component kit used by generated apps.
//!
//! Every component takes the caller's `ak_id` and renders it as `data-ak-id`, so
//! the user can point at it in the preview and the agent can find it in code.
//! Parts inside a component get derived ids (`<ak_id>-close`, `<ak_id>-<key>`).
//! Components contain no fixed text: labels come from the caller, in the app's
//! language.
//!
//! Styling is Tailwind v4 utility classes written out in full here; the app's
//! stylesheet must include these sources (`@source "…/ak-ui/src";`).

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

/// `<base>-<part>`: the id of a part inside a component.
pub fn part_id(base: &str, part: impl std::fmt::Display) -> String {
    format!("{base}-{part}")
}
