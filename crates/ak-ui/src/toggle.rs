use leptos::prelude::*;

/// An on/off switch bound to `checked` (a `role="switch"` button). The label may
/// change.
#[component]
pub fn Toggle(
    #[prop(into)] ak_id: String,
    checked: RwSignal<bool>,
    #[prop(into)] label: Signal<String>,
    #[prop(optional, into)] disabled: Signal<bool>,
) -> impl IntoView {
    view! {
        <label class="inline-flex cursor-pointer items-center gap-3 text-sm text-gray-900 dark:text-gray-100">
            <button
                type="button"
                role="switch"
                data-ak-id=ak_id
                aria-checked=move || checked.get().to_string()
                disabled=move || disabled.get()
                class=move || {
                    if checked.get() {
                        "relative inline-flex h-6 w-11 shrink-0 items-center rounded-full bg-blue-600 transition-colors disabled:opacity-50"
                    } else {
                        "relative inline-flex h-6 w-11 shrink-0 items-center rounded-full bg-gray-300 transition-colors disabled:opacity-50 dark:bg-gray-600"
                    }
                }
                on:click=move |_| checked.update(|c| *c = !*c)
            >
                <span class=move || {
                    if checked.get() {
                        "inline-block size-5 translate-x-5 rounded-full bg-white shadow transition-transform"
                    } else {
                        "inline-block size-5 translate-x-0.5 rounded-full bg-white shadow transition-transform"
                    }
                }></span>
            </button>
            <span>{move || label.get()}</span>
        </label>
    }
}
