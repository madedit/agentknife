use leptos::prelude::*;

/// A box that groups related content, with an optional heading (which may
/// change).
#[component]
pub fn Card(
    #[prop(into)] ak_id: String,
    #[prop(optional, into)] title: Signal<Option<String>>,
    children: Children,
) -> impl IntoView {
    view! {
        <section
            data-ak-id=ak_id
            class="rounded-xl border border-gray-200 bg-white p-4 shadow-sm dark:border-gray-700 dark:bg-gray-900"
        >
            {move || {
                title
                    .get()
                    .map(|t| {
                        view! {
                            <h3 class="mb-3 text-base font-semibold text-gray-900 dark:text-gray-100">
                                {t}
                            </h3>
                        }
                    })
            }}
            {children()}
        </section>
    }
}
