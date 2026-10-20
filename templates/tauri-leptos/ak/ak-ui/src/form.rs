use leptos::prelude::*;

/// A form whose submit (Enter in a field, or a submit [`crate::Button`]) calls
/// `on_submit` without reloading the page.
#[component]
pub fn Form(
    #[prop(into)] ak_id: String,
    #[prop(into)] on_submit: Callback<()>,
    children: Children,
) -> impl IntoView {
    view! {
        <form
            data-ak-id=ak_id
            class="flex flex-col gap-3 sm:flex-row sm:items-end"
            on:submit=move |ev| {
                ev.prevent_default();
                on_submit.run(());
            }
        >
            {children()}
        </form>
    }
}
