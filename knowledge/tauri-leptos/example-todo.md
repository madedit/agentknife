# Complete example: a to-do app

Add items, tick them off, delete them, filter by state; everything is saved to a file
by the backend and loaded when the app starts. Adapt names and texts to the user's
request and language.

## Backend: `src-tauri/src/main.rs`
Add `serde_json = "1.0.154"` to `src-tauri/Cargo.toml` `[dependencies]`.
```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use tauri::Manager;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Todo {
    id: u64,
    text: String,
    done: bool,
}

fn todos_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("todos.json"))
}

#[tauri::command]
fn load_todos(app: tauri::AppHandle) -> Result<Vec<Todo>, String> {
    match std::fs::read_to_string(todos_path(&app)?) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("The saved list is damaged: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
fn save_todos(app: tauri::AppHandle, todos: Vec<Todo>) -> Result<(), String> {
    let path = todos_path(&app)?;
    let text = serde_json::to_string_pretty(&todos).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(ak_bridge::init())
        .invoke_handler(tauri::generate_handler![load_todos, save_todos])
        .setup(|app| {
            if !ak_bridge::is_preview()
                && let Some(window) = app.get_webview_window("main")
            {
                window.show()?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("the app failed to start");
}
```

## Screens: `src/main.rs`
```rust
use ak_ui::*;
use leptos::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Todo {
    id: u64,
    text: String,
    done: bool,
}

#[derive(Serialize)]
struct SaveArgs {
    todos: Vec<Todo>,
}

fn main() {
    ak_bridge::start();
    leptos::mount::mount_to_body(|| view! { <ToastHost ak_id="toasts"><TodoApp /></ToastHost> });
}

#[component]
fn TodoApp() -> impl IntoView {
    let toaster = use_toaster();
    let todos = RwSignal::new(Vec::<Todo>::new());
    let text = RwSignal::new(String::new());
    let filter = RwSignal::new("all".to_string());
    let confirm_clear = RwSignal::new(false);

    // Load once at start.
    leptos::task::spawn_local(async move {
        match ak_bridge::invoke::<Vec<Todo>>("load_todos", &()).await {
            Ok(list) => todos.set(list),
            Err(e) => toaster.error(e.to_string()),
        }
    });

    // Save the whole list after every change.
    let save = move || {
        let args = SaveArgs { todos: todos.get_untracked() };
        leptos::task::spawn_local(async move {
            if let Err(e) = ak_bridge::invoke::<()>("save_todos", &args).await {
                toaster.error(e.to_string());
            }
        });
    };

    let add = move |()| {
        let t = text.get_untracked().trim().to_string();
        if t.is_empty() {
            toaster.error("Type something first");
            return;
        }
        let id = todos.with_untracked(|v| v.iter().map(|t| t.id).max().unwrap_or(0) + 1);
        todos.update(|v| v.push(Todo { id, text: t, done: false }));
        text.set(String::new());
        save();
    };
    let toggle = move |id: u64| {
        todos.update(|v| {
            if let Some(t) = v.iter_mut().find(|t| t.id == id) {
                t.done = !t.done;
            }
        });
        save();
    };
    let remove = move |id: u64| {
        todos.update(|v| v.retain(|t| t.id != id));
        save();
    };

    let shown = Signal::derive(move || {
        let f = filter.get();
        todos
            .get()
            .into_iter()
            .filter(|t| match f.as_str() {
                "open" => !t.done,
                "done" => t.done,
                _ => true,
            })
            .collect::<Vec<_>>()
    });
    let left = move || todos.with(|v| v.iter().filter(|t| !t.done).count());

    view! {
        <main class="mx-auto flex min-h-screen max-w-xl flex-col gap-4 p-6">
            <h1 data-ak-id="app-title" class="text-3xl font-bold">"To-do"</h1>
            <Form ak_id="add-form" on_submit=add>
                <Input ak_id="todo-input" value=text label="New item" placeholder="Buy milk" />
                <Button ak_id="add-button" submit=true>"Add"</Button>
            </Form>
            <div class="flex items-center justify-between">
                <Nav
                    ak_id="filter-tabs"
                    items=vec![
                        NavItem::new("all", "All"),
                        NavItem::new("open", "Open"),
                        NavItem::new("done", "Done"),
                    ]
                    active=filter
                />
                <span data-ak-id="items-left" class="text-sm text-gray-500">
                    {left} " left"
                </span>
            </div>
            <List
                ak_id="todo-list"
                items=shown
                key=|t: &Todo| t.id
                row=move |t: Todo| {
                    let id = t.id;
                    view! {
                        <input
                            type="checkbox"
                            data-ak-id=format!("todo-{id}-done")
                            prop:checked=t.done
                            on:change=move |_| toggle(id)
                            class="size-4"
                        />
                        <span class=if t.done { "flex-1 text-gray-400 line-through" } else { "flex-1" }>
                            {t.text}
                        </span>
                        <Button
                            ak_id=format!("todo-{id}-delete")
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
            <div class="flex justify-end">
                <Button
                    ak_id="clear-done-button"
                    variant=ButtonVariant::Secondary
                    disabled=Signal::derive(move || todos.with(|v| !v.iter().any(|t| t.done)))
                    on_click=move |_| confirm_clear.set(true)
                >
                    "Clear done"
                </Button>
            </div>
            <Dialog ak_id="clear-dialog" open=confirm_clear title="Remove finished items?" close_label="Close">
                <div class="flex justify-end gap-2">
                    <Button ak_id="clear-cancel" variant=ButtonVariant::Ghost on_click=move |_| confirm_clear.set(false)>
                        "Cancel"
                    </Button>
                    <Button
                        ak_id="clear-confirm"
                        variant=ButtonVariant::Danger
                        on_click=move |_| {
                            todos.update(|v| v.retain(|t| !t.done));
                            confirm_clear.set(false);
                            save();
                        }
                    >
                        "Remove"
                    </Button>
                </div>
            </Dialog>
        </main>
    }
}
```

## Notes
- Each `List` row gets the id `todo-list-<id>`; checkboxes and delete buttons get
  `todo-<id>-done` / `todo-<id>-delete`.
- `use_toaster()` is called once in the component body; the closures use the copy.
- The list is saved whole after each change: simple and safe for hundreds of items.
