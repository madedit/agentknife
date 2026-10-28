use leptos::prelude::*;

use crate::part_id;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavItem {
    /// Stable id; the item's `data-ak-id` is `<nav ak_id>-<id>`.
    pub id: String,
    pub label: String,
}

impl NavItem {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

/// Tabs / top navigation. `active` holds the id of the current item.
#[component]
pub fn Nav(
    #[prop(into)] ak_id: String,
    items: Vec<NavItem>,
    active: RwSignal<String>,
    /// More classes after the component's own (layout such as `w-full`, `mt-4`;
    /// a colour that must win ends with `!`: `bg-slate-800!`).
    #[prop(optional, into)]
    class: String,
) -> impl IntoView {
    let buttons = items
        .into_iter()
        .map(|item| {
            let id = item.id.clone();
            let current = move || active.get() == id;
            let is_current = current.clone();
            let id = item.id.clone();
            view! {
                <li>
                    <button
                        type="button"
                        data-ak-id=part_id(&ak_id, &item.id)
                        aria-current=move || is_current().then_some("page")
                        class=move || {
                            if current() {
                                "ak-nav-tab rounded-lg px-3 py-2 text-sm font-medium bg-blue-600 text-white"
                            } else {
                                "ak-nav-tab rounded-lg px-3 py-2 text-sm font-medium text-gray-700 hover:bg-gray-100 dark:text-gray-200 dark:hover:bg-gray-800"
                            }
                        }
                        on:click=move |_| active.set(id.clone())
                    >
                        {item.label}
                    </button>
                </li>
            }
        })
        .collect_view();
    view! {
        <nav data-ak-id=ak_id.clone() class=crate::classes("ak-nav", &class)>
            <ul class="flex flex-wrap gap-1">{buttons}</ul>
        </nav>
    }
}
