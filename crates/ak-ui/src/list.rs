use std::hash::Hash;

use leptos::prelude::*;

use crate::part_id;

/// A list of `items`. Each row is an `<li>` with `data-ak-id` `<ak_id>-<key>`, so
/// "the second item" can be pointed at. `empty` shows when there are no items.
#[component]
pub fn List<T, K, KF, RF, V>(
    #[prop(into)] ak_id: String,
    #[prop(into)] items: Signal<Vec<T>>,
    /// A stable, unique key per item (its id, not its position).
    key: KF,
    /// Renders one row's content.
    row: RF,
    #[prop(optional, into)] empty: Option<String>,
    /// More classes after the component's own (layout such as `w-full`, `mt-4`;
    /// a colour that must win ends with `!`: `bg-slate-800!`).
    #[prop(optional, into)]
    class: String,
) -> impl IntoView
where
    T: Clone + Send + Sync + 'static,
    K: Eq + Hash + std::fmt::Display + Send + Sync + 'static,
    KF: Fn(&T) -> K + Clone + Send + Sync + 'static,
    RF: Fn(T) -> V + Clone + Send + Sync + 'static,
    V: IntoView + 'static,
{
    let base = ak_id.clone();
    let row_key = key.clone();
    let empty_id = part_id(&ak_id, "empty");
    let class = crate::classes(
        "ak-list divide-y divide-gray-200 rounded-xl border border-gray-200 bg-white dark:divide-gray-700 dark:border-gray-700 dark:bg-gray-900",
        &class,
    );
    view! {
        <ul data-ak-id=ak_id class=class>
            <For
                each=move || items.get()
                key=move |item| key(item)
                children=move |item| {
                    let id = part_id(&base, row_key(&item));
                    view! {
                        <li
                            data-ak-id=id
                            class="ak-list-row flex items-center gap-3 px-4 py-3 text-sm text-gray-900 dark:text-gray-100"
                        >
                            {row(item)}
                        </li>
                    }
                }
            />
            <Show when=move || items.with(Vec::is_empty)>
                <li data-ak-id=empty_id.clone() class="ak-list-empty px-4 py-6 text-center text-sm text-gray-500">
                    {empty.clone()}
                </li>
            </Show>
        </ul>
    }
}
