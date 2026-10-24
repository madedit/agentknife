use leptos::prelude::*;

use crate::part_id;

/// A modal dialog shown while `open` is true. Escape, the close button and a
/// click on the backdrop close it, then `on_close` runs (clear what the dialog
/// was editing there). Parts: `<ak_id>-close`, `<ak_id>-backdrop`.
#[component]
pub fn Dialog(
    #[prop(into)] ak_id: String,
    open: RwSignal<bool>,
    /// May change (a signal or `Signal::derive`).
    #[prop(into)]
    title: Signal<String>,
    /// What a screen reader says for the × button (in the app's language).
    #[prop(into)]
    close_label: String,
    /// Runs when the dialog closes itself (Escape, ×, backdrop).
    #[prop(optional, into)]
    on_close: Option<Callback<()>>,
    children: ChildrenFn,
) -> impl IntoView {
    let close = move || {
        open.set(false);
        if let Some(cb) = on_close {
            cb.run(());
        }
    };
    #[cfg(target_arch = "wasm32")]
    {
        let handle = window_event_listener(leptos::ev::keydown, move |ev| {
            if ev.key() == "Escape" && open.get_untracked() {
                close();
            }
        });
        on_cleanup(move || handle.remove());
    }
    let close_id = part_id(&ak_id, "close");
    let backdrop_id = part_id(&ak_id, "backdrop");
    view! {
        <Show when=move || open.get()>
            <div
                data-ak-id=backdrop_id.clone()
                class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
                on:click=move |_| close()
            >
                <div
                    role="dialog"
                    aria-modal="true"
                    aria-label=move || title.get()
                    data-ak-id=ak_id.clone()
                    class="relative w-full max-w-md rounded-2xl bg-white p-6 shadow-xl dark:bg-gray-900"
                    on:click=|ev| ev.stop_propagation()
                >
                    <h2 class="mb-4 pr-8 text-lg font-semibold text-gray-900 dark:text-gray-100">
                        {move || title.get()}
                    </h2>
                    {children()}
                    <button
                        type="button"
                        data-ak-id=close_id.clone()
                        aria-label=close_label.clone()
                        class="absolute right-3 top-3 rounded-lg p-2 leading-none text-gray-500 hover:bg-gray-100 dark:hover:bg-gray-800"
                        on:click=move |_| close()
                    >
                        "×"
                    </button>
                </div>
            </div>
        </Show>
    }
}
