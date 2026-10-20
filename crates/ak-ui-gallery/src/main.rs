//! Every ak-ui component on one page, wired together like a small app.
//!
//! ```text
//! cd crates/ak-ui-gallery && trunk serve
//! ```

#![forbid(unsafe_code)]

use ak_ui::*;
use leptos::prelude::*;

#[derive(Debug, Clone, PartialEq)]
struct Item {
    id: u32,
    text: String,
}

fn main() {
    leptos::mount::mount_to_body(|| {
        view! {
            <ToastHost ak_id="toasts">
                <Gallery />
            </ToastHost>
        }
    });
}

#[component]
fn Gallery() -> impl IntoView {
    let toaster = use_toaster();
    let tab = RwSignal::new("components".to_owned());
    let text = RwSignal::new(String::new());
    let items = RwSignal::new(vec![
        Item {
            id: 1,
            text: "Buy milk".into(),
        },
        Item {
            id: 2,
            text: "Water the plants".into(),
        },
    ]);
    let next_id = RwSignal::new(3u32);
    let notify = RwSignal::new(true);
    let confirm_open = RwSignal::new(false);

    let add = move |()| {
        let t = text.get_untracked().trim().to_owned();
        if t.is_empty() {
            toaster.error("Type something first");
            return;
        }
        let id = next_id.get_untracked();
        next_id.set(id + 1);
        items.update(|list| list.push(Item { id, text: t }));
        text.set(String::new());
        if notify.get_untracked() {
            toaster.success("Added");
        }
    };
    let remove = move |id: u32| items.update(|list| list.retain(|i| i.id != id));

    view! {
        <main data-ak-id="gallery-screen" class="mx-auto flex max-w-2xl flex-col gap-6 p-6 text-gray-900 dark:text-gray-100">
            <header class="flex flex-wrap items-center justify-between gap-4">
                <h1 data-ak-id="gallery-title" class="text-2xl font-bold">
                    "ak-ui gallery"
                </h1>
                <Nav
                    ak_id="gallery-tabs"
                    items=vec![
                        NavItem::new("components", "Components"),
                        NavItem::new("about", "About"),
                    ]
                    active=tab
                />
            </header>
            <Show
                when=move || tab.get() == "components"
                fallback=|| {
                    view! {
                        <Card ak_id="about-card" title="About">
                            <p class="text-sm">
                                "Button, Input, Form, List, Dialog, Nav, Card, Toggle and Toast, each with a data-ak-id."
                            </p>
                        </Card>
                    }
                }
            >
                <Card ak_id="add-card" title="Add an item">
                    <Form ak_id="add-form" on_submit=add>
                        <Input
                            ak_id="item-input"
                            value=text
                            label="New item"
                            placeholder="What needs doing?"
                        />
                        <Button ak_id="add-button" submit=true>
                            "Add"
                        </Button>
                    </Form>
                </Card>
                <List
                    ak_id="item-list"
                    items=items
                    key=|i: &Item| i.id
                    row=move |i: Item| {
                        let id = i.id;
                        view! {
                            <span class="flex-1">{i.text}</span>
                            <Button
                                ak_id=format!("delete-{id}")
                                variant=ButtonVariant::Ghost
                                label="Delete"
                                on_click=move |_| remove(id)
                            >
                                "✕"
                            </Button>
                        }
                    }
                    empty="Nothing to do"
                />
                <Card ak_id="settings-card" title="Settings">
                    <div class="flex flex-wrap items-center justify-between gap-4">
                        <Toggle ak_id="notify-toggle" checked=notify label="Show a toast when adding" />
                        <div class="flex gap-2">
                            <Button
                                ak_id="info-button"
                                variant=ButtonVariant::Secondary
                                on_click=move |_| toaster.info("Just so you know")
                            >
                                "Show info"
                            </Button>
                            <Button
                                ak_id="clear-button"
                                variant=ButtonVariant::Danger
                                disabled=Signal::derive(move || items.with(Vec::is_empty))
                                on_click=move |_| confirm_open.set(true)
                            >
                                "Clear all"
                            </Button>
                        </div>
                    </div>
                </Card>
            </Show>
            <Dialog
                ak_id="clear-dialog"
                open=confirm_open
                title="Clear all items?"
                close_label="Close"
            >
                <p class="mb-6 text-sm text-gray-600 dark:text-gray-300">
                    "This removes every item in the list."
                </p>
                <div class="flex justify-end gap-2">
                    <Button
                        ak_id="clear-cancel"
                        variant=ButtonVariant::Ghost
                        on_click=move |_| confirm_open.set(false)
                    >
                        "Cancel"
                    </Button>
                    <Button
                        ak_id="clear-confirm"
                        variant=ButtonVariant::Danger
                        on_click=move |_| {
                            items.set(Vec::new());
                            confirm_open.set(false);
                            toaster.info("List cleared");
                        }
                    >
                        "Clear"
                    </Button>
                </div>
            </Dialog>
        </main>
    }
}
