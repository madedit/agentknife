use leptos::prelude::*;

/// A form whose submit (Enter in a field, or a submit [`crate::Button`]) calls
/// `on_submit` without reloading the page.
#[component]
pub fn Form(
    #[prop(into)] ak_id: String,
    #[prop(into)] on_submit: Callback<()>,
    /// More classes after the component's own (layout such as `w-full`, `mt-4`;
    /// a colour that must win ends with `!`: `bg-slate-800!`).
    #[prop(optional, into)]
    class: String,
    children: Children,
) -> impl IntoView {
    let class = crate::classes(
        "ak-form flex flex-col gap-3 sm:flex-row sm:items-end",
        &class,
    );
    view! {
        <form
            data-ak-id=ak_id
            class=class
            on:submit=move |ev| {
                ev.prevent_default();
                on_submit.run(());
            }
        >
            {children()}
        </form>
    }
}
