use leptos::ev::MouseEvent;
use leptos::prelude::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ButtonVariant {
    /// The main action on a screen.
    #[default]
    Primary,
    Secondary,
    /// Deleting or other actions that cannot be undone.
    Danger,
    /// Low-emphasis actions (icons, "Cancel").
    Ghost,
}

impl ButtonVariant {
    /// The marker classes a stylesheet can restyle every such button with.
    fn marker(self) -> &'static str {
        match self {
            ButtonVariant::Primary => "ak-button ak-button-primary",
            ButtonVariant::Secondary => "ak-button ak-button-secondary",
            ButtonVariant::Danger => "ak-button ak-button-danger",
            ButtonVariant::Ghost => "ak-button ak-button-ghost",
        }
    }

    fn class(self) -> &'static str {
        match self {
            ButtonVariant::Primary => {
                "inline-flex items-center justify-center gap-2 rounded-lg px-4 py-2 text-sm font-medium bg-blue-600 text-white shadow-sm hover:bg-blue-700 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600 disabled:cursor-not-allowed disabled:opacity-50"
            }
            ButtonVariant::Secondary => {
                "inline-flex items-center justify-center gap-2 rounded-lg px-4 py-2 text-sm font-medium bg-white text-gray-900 ring-1 ring-gray-300 ring-inset shadow-sm hover:bg-gray-50 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600 disabled:cursor-not-allowed disabled:opacity-50 dark:bg-gray-800 dark:text-gray-100 dark:ring-gray-600 dark:hover:bg-gray-700"
            }
            ButtonVariant::Danger => {
                "inline-flex items-center justify-center gap-2 rounded-lg px-4 py-2 text-sm font-medium bg-red-600 text-white shadow-sm hover:bg-red-700 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-red-600 disabled:cursor-not-allowed disabled:opacity-50"
            }
            ButtonVariant::Ghost => {
                "inline-flex items-center justify-center gap-2 rounded-lg px-3 py-2 text-sm font-medium text-gray-700 hover:bg-gray-100 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-600 disabled:cursor-not-allowed disabled:opacity-50 dark:text-gray-200 dark:hover:bg-gray-800"
            }
        }
    }
}

/// The classes of a button (`small`: less padding, smaller text).
fn button_class(variant: ButtonVariant, small: bool) -> String {
    if small {
        variant
            .class()
            .replace("px-4 py-2 text-sm", "px-2 py-1 text-xs")
            .replace("px-3 py-2 text-sm", "px-2 py-1 text-xs")
    } else {
        variant.class().to_owned()
    }
}

/// A button. `submit` makes it submit its [`crate::Form`].
#[component]
pub fn Button(
    #[prop(into)] ak_id: String,
    #[prop(optional)] variant: ButtonVariant,
    #[prop(optional, into)] disabled: Signal<bool>,
    #[prop(optional)] submit: bool,
    /// Smaller text and padding (toolbars, a row's own buttons).
    #[prop(optional)]
    small: bool,
    /// For icon-only buttons: what a screen reader says.
    #[prop(optional, into)]
    label: Option<String>,
    #[prop(optional, into)] on_click: Option<Callback<MouseEvent>>,
    /// More classes after the component's own (layout such as `w-full`, `mt-4`;
    /// a colour that must win ends with `!`: `bg-slate-800!`).
    #[prop(optional, into)]
    class: String,
    children: Children,
) -> impl IntoView {
    let own = format!("{} {}", variant.marker(), button_class(variant, small));
    let class = crate::classes(&own, &class);
    view! {
        <button
            type=if submit { "submit" } else { "button" }
            data-ak-id=ak_id
            class=class
            aria-label=label
            disabled=move || disabled.get()
            on:click=move |ev| {
                if let Some(cb) = on_click {
                    cb.run(ev);
                }
            }
        >
            {children()}
        </button>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_buttons_are_small_in_every_variant() {
        use ButtonVariant::*;
        for v in [Primary, Secondary, Danger, Ghost] {
            let small = button_class(v, true);
            assert!(small.contains("px-2 py-1 text-xs"), "{v:?}: {small}");
            assert!(!small.contains("text-sm"), "{v:?}: {small}");
            assert_eq!(button_class(v, false), v.class());
        }
    }
}
