use leptos::prelude::*;

/// A text field bound to `value`. The `data-ak-id` is on the `<input>` itself
/// (that is what the user points at and the agent types into). `label` and
/// `placeholder` may change (a signal or `Signal::derive`).
#[component]
pub fn Input(
    #[prop(into)] ak_id: String,
    value: RwSignal<String>,
    #[prop(optional, into)] label: Signal<Option<String>>,
    #[prop(optional, into)] placeholder: Signal<Option<String>>,
    /// `text` (default), `email`, `number`, `password`, `search`, `date`…
    #[prop(optional, into)]
    kind: Option<String>,
    #[prop(optional, into)] disabled: Signal<bool>,
    /// More classes for the field itself (after its own; `!` to win: `bg-slate-800!`).
    #[prop(optional, into)]
    class: String,
) -> impl IntoView {
    let class = crate::classes(
        "ak-input block w-full rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm text-gray-900 shadow-sm placeholder:text-gray-400 focus:border-blue-600 focus:outline-none focus:ring-2 focus:ring-blue-600/20 disabled:cursor-not-allowed disabled:bg-gray-100 dark:border-gray-600 dark:bg-gray-800 dark:text-gray-100",
        &class,
    );
    let field = view! {
        <input
            data-ak-id=ak_id
            type=kind.unwrap_or_else(|| "text".to_owned())
            placeholder=move || placeholder.get()
            disabled=move || disabled.get()
            class=class
            prop:value=move || value.get()
            on:input=move |ev| value.set(event_target_value(&ev))
        />
    };
    view! {
        <label class="ak-input-label flex flex-1 flex-col gap-1 text-sm font-medium text-gray-700 dark:text-gray-300">
            {move || label.get().map(|l| view! { <span>{l}</span> })}
            {field}
        </label>
    }
}
