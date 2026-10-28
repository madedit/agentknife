use leptos::prelude::*;

/// A box that groups related content, with an optional heading (which may
/// change).
#[component]
pub fn Card(
    #[prop(into)] ak_id: String,
    #[prop(optional, into)] title: Signal<Option<String>>,
    /// More classes after the component's own (layout such as `w-full`, `mt-4`;
    /// a colour that must win ends with `!`: `bg-slate-800!`).
    #[prop(optional, into)]
    class: String,
    children: Children,
) -> impl IntoView {
    let class = crate::classes(
        "ak-card rounded-xl border border-gray-200 bg-white p-4 shadow-sm dark:border-gray-700 dark:bg-gray-900",
        &class,
    );
    view! {
        <section data-ak-id=ak_id class=class>
            {move || {
                title
                    .get()
                    .map(|t| {
                        view! {
                            <h3 class="ak-card-title mb-3 text-base font-semibold text-gray-900 dark:text-gray-100">
                                {t}
                            </h3>
                        }
                    })
            }}
            {children()}
        </section>
    }
}
