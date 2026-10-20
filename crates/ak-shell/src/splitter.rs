//! The handle between two panes: drag it, use the arrow keys, or double-click
//! it to go back to the default width.

use leptos::ev;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::layout::{Bounds, KEY_STEP};

/// Which side of the handle the pane is on: the files pane grows when the
/// handle moves right, the chat when it moves left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grows {
    Right,
    Left,
}

impl Grows {
    fn sign(self) -> f64 {
        match self {
            Grows::Right => 1.0,
            Grows::Left => -1.0,
        }
    }
}

#[component]
pub fn Splitter(
    /// Accessible name ("Resize the chat panel").
    label: Signal<String>,
    /// The pane's width in pixels.
    width: Signal<f64>,
    bounds: Bounds,
    grows: Grows,
    set_width: Callback<f64>,
) -> impl IntoView {
    // (pointer x, width) when the drag started.
    let drag = StoredValue::new(None::<(f64, f64)>);
    let dragging = RwSignal::new(false);

    let down = move |e: ev::PointerEvent| {
        if e.button() != 0 {
            return;
        }
        e.prevent_default();
        // Keeps the moves coming even over the preview's frame.
        if let Some(el) = e
            .current_target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        {
            let _ = el.set_pointer_capture(e.pointer_id());
        }
        drag.set_value(Some((f64::from(e.client_x()), width.get_untracked())));
        dragging.set(true);
    };
    let moved = move |e: ev::PointerEvent| {
        if let Some((x0, w0)) = drag.get_value() {
            let dx = f64::from(e.client_x()) - x0;
            set_width.run(bounds.clamp(w0 + grows.sign() * dx));
        }
    };
    let up = move |_: ev::PointerEvent| {
        drag.set_value(None);
        dragging.set(false);
    };
    let key = move |e: ev::KeyboardEvent| {
        let step = match e.key().as_str() {
            "ArrowRight" => KEY_STEP,
            "ArrowLeft" => -KEY_STEP,
            "Home" | "Enter" => {
                e.prevent_default();
                set_width.run(bounds.default);
                return;
            }
            _ => return,
        };
        e.prevent_default();
        set_width.run(bounds.clamp(width.get_untracked() + grows.sign() * step));
    };

    view! {
        <div
            role="separator"
            aria-orientation="vertical"
            aria-label=move || label.get()
            aria-valuemin=bounds.min
            aria-valuemax=bounds.max
            aria-valuenow=move || width.get().round()
            tabindex="0"
            class=move || {
                if dragging.get() {
                    "w-1 shrink-0 cursor-col-resize touch-none select-none bg-blue-500 outline-none"
                } else {
                    "w-1 shrink-0 cursor-col-resize touch-none select-none bg-gray-200 outline-none hover:bg-blue-400 focus-visible:bg-blue-500 dark:bg-gray-800"
                }
            }
            on:pointerdown=down
            on:pointermove=moved
            on:pointerup=up
            on:pointercancel=up
            on:dblclick=move |_| set_width.run(bounds.default)
            on:keydown=key
        ></div>
    }
}
