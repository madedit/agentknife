//! A to-do list: add and delete items. The list is kept in the web view's
//! local storage, so it is still there after a restart.
//!
//! (tests/e2e.rs: what the scripted AI writes as the project's src/main.rs.)

use ak_ui::*;
use leptos::prelude::*;
use serde::{Deserialize, Serialize};

/// The local-storage key of the list (JSON).
const STORAGE_KEY: &str = "todos";

fn main() {
    ak_bridge::start();
    leptos::mount::mount_to_body(App);
}

#[derive(Clone, Serialize, Deserialize)]
struct Todo {
    id: u32,
    text: String,
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

fn load() -> Vec<Todo> {
    storage()
        .and_then(|s| s.get_item(STORAGE_KEY).ok().flatten())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save(todos: &[Todo]) {
    if let (Some(s), Ok(text)) = (storage(), serde_json::to_string(todos)) {
        let _ = s.set_item(STORAGE_KEY, &text);
    }
}

#[component]
fn App() -> impl IntoView {
    view! {
        <ToastHost ak_id="toasts">
            <Home />
        </ToastHost>
    }
}

#[component]
fn Home() -> impl IntoView {
    let todos = RwSignal::new(load());
    let draft = RwSignal::new(String::new());

    let add = move |()| {
        let text = draft.get_untracked().trim().to_owned();
        if text.is_empty() {
            return;
        }
        todos.update(|list| {
            let id = list.iter().map(|t| t.id).max().unwrap_or(0) + 1;
            list.push(Todo { id, text });
            save(list);
        });
        draft.set(String::new());
    };
    let remove = move |id: u32| {
        todos.update(|list| {
            list.retain(|t| t.id != id);
            save(list);
        });
    };

    view! {
        <main data-ak-id="home-screen" class="mx-auto flex min-h-screen max-w-lg flex-col gap-6 p-6">
            <h1 data-ak-id="home-title" class="text-3xl font-bold">
                "待辦清單"
            </h1>
            <Card ak_id="todo-card" title="新增待辦">
                <Form ak_id="todo-form" on_submit=add>
                    <Input ak_id="todo-input" value=draft label="要做的事" placeholder="買牛奶" />
                    <Button ak_id="add-button" submit=true>
                        "加入"
                    </Button>
                </Form>
            </Card>
            <List
                ak_id="todo-list"
                items=todos
                key=|t: &Todo| t.id
                row=move |t: Todo| {
                    let id = t.id;
                    view! {
                        <span class="flex-1">{t.text}</span>
                        <Button
                            ak_id=format!("delete-{id}")
                            variant=ButtonVariant::Danger
                            on_click=Callback::new(move |_| remove(id))
                        >
                            "刪除"
                        </Button>
                    }
                }
                empty="還沒有待辦事項"
            />
        </main>
    }
}
