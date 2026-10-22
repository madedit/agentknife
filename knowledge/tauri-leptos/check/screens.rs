//! Snippets from the knowledge pack (screens side), compiled by `cargo xtask check-knowledge`.
use ak_ui::*;
use leptos::prelude::*;
use serde::{Deserialize, Serialize};

// ---- leptos-components ----
#[component]
fn Greeting(
    name: String,
    #[prop(optional)] excited: bool,
    #[prop(into, optional)] note: Option<String>,
) -> impl IntoView {
    let mark = if excited { "!" } else { "." };
    view! {
        <p data-ak-id="greeting" class="text-lg">
            "Hello, " {name} {mark}
        </p>
        {note.map(|n| view! { <p class="text-sm text-gray-500">{n}</p> })}
    }
}

#[component]
fn Page() -> impl IntoView {
    view! { <Greeting name="Ada".to_string() excited=true note="Welcome back" /> }
}

#[component]
fn Panel(#[prop(into)] title: String, children: Children) -> impl IntoView {
    view! {
        <section data-ak-id="panel" class="rounded-xl border p-4">
            <h2 class="font-semibold">{title}</h2>
            {children()}
        </section>
    }
}

#[component]
fn UsesPanel() -> impl IntoView {
    view! { <Panel title="Settings"> <p>"Inside"</p> </Panel> }
}

#[component]
fn ItemList() -> impl IntoView {
    view! { <p>"items"</p> }
}
#[component]
fn Dashboard() -> impl IntoView {
    view! { <p>"dash"</p> }
}
#[component]
fn Login() -> impl IntoView {
    view! { <p>"login"</p> }
}
#[component]
fn Stats() -> impl IntoView {
    view! { <p>"stats"</p> }
}
#[component]
fn ListScreen() -> impl IntoView {
    view! { <p>"list"</p> }
}

#[derive(Clone, Debug, PartialEq)]
struct Todo {
    id: u32,
    text: String,
    done: bool,
}

#[component]
fn Conditional() -> impl IntoView {
    let items = RwSignal::new(Vec::<Todo>::new());
    let logged_in = RwSignal::new(false);
    let todos = items;
    let screen = RwSignal::new("list".to_string());
    view! {
        <Show when=move || items.with(|v| v.is_empty()) fallback=|| view! { <ItemList /> }>
            <p data-ak-id="empty-hint">"Nothing here yet"</p>
        </Show>
        {move || if logged_in.get() {
            view! { <Dashboard /> }.into_any()
        } else {
            view! { <Login /> }.into_any()
        }}
        <For
            each=move || todos.get()
            key=|t| t.id
            children=move |t: Todo| view! { <li data-ak-id=format!("todo-{}", t.id)>{t.text}</li> }
        />
        <Nav ak_id="main-nav" items=vec![NavItem::new("list", "List"), NavItem::new("stats", "Stats")] active=screen />
        {move || match screen.get().as_str() {
            "stats" => view! { <Stats /> }.into_any(),
            _ => view! { <ListScreen /> }.into_any(),
        }}
    }
}

// ---- leptos-reactivity ----
#[component]
fn Reactivity() -> impl IntoView {
    let count = RwSignal::new(0);
    count.get();
    count.set(5);
    count.update(|n| *n += 1);
    count.with(|n| *n > 3);
    count.get_untracked();
    let (name, set_name) = signal(String::new());
    set_name.set("x".into());
    let _ = name.get();

    let items = RwSignal::new(Vec::<String>::new());
    let double = move || count.get() * 2;
    let total = Memo::new(move |_| items.with(|v| v.len()));

    let todos = RwSignal::new(Vec::<Todo>::new());
    todos.update(|v| v.push(Todo { id: 1, text: "Milk".into(), done: false }));
    todos.update(|v| v.retain(|t| t.id != 1));
    todos.update(|v| if let Some(t) = v.iter_mut().find(|t| t.id == 1) { t.done = !t.done });

    Effect::new(move |_| {
        let count = todos.with(|v| v.len());
        leptos::logging::log!("{count} todos");
    });

    let busy = RwSignal::new(false);
    let toaster = use_toaster();
    let load = move |_| {
        busy.set(true);
        leptos::task::spawn_local(async move {
            let result: Result<Vec<String>, _> = ak_bridge::invoke("list_items", &()).await;
            match result {
                Ok(list) => items.set(list),
                Err(e) => toaster.error(e.to_string()),
            }
            busy.set(false);
        });
    };
    let loaded = LocalResource::new(|| async {
        ak_bridge::invoke::<Vec<String>>("list_items", &()).await.unwrap_or_default()
    });

    view! {
        <p>{double}</p> <p>{move || total.get()}</p>
        <button on:click=load>"Load"</button>
        <Suspense fallback=|| view! { <p>"Loading…"</p> }>
            {move || loaded.get().map(|items| view! { <p>{items.len()} " items"</p> })}
        </Suspense>
    }
}

#[derive(Clone, Copy)]
struct AppState {
    todos: RwSignal<Vec<Todo>>,
}

fn context_example() {
    provide_context(AppState { todos: RwSignal::new(vec![]) });
    let state = expect_context::<AppState>();
    let _ = state.todos;
}

// ---- forms ----
#[component]
fn Forms() -> impl IntoView {
    let title = RwSignal::new(String::new());
    let notes = RwSignal::new(String::new());
    let amount = RwSignal::new(String::new());
    let parsed = move || amount.with(|a| a.trim().parse::<f64>().ok());
    let done = RwSignal::new(false);
    let color = RwSignal::new("blue".to_string());
    let text = RwSignal::new(String::new());
    let add = move || text.set(String::new());
    view! {
        <input data-ak-id="title-input" type="text" bind:value=title
            class="rounded-lg border px-3 py-2" />
        <p>"You typed: " {move || title.get()}</p>
        <textarea data-ak-id="notes" bind:value=notes rows="4"></textarea>
        <input data-ak-id="amount-input" type="number" step="0.01" bind:value=amount />
        <Show when=move || parsed().is_none() && !amount.with(String::is_empty)>
            <p data-ak-id="amount-error" class="text-sm text-red-600">"Please type a number"</p>
        </Show>
        <label class="flex items-center gap-2">
            <input data-ak-id="done-checkbox" type="checkbox" bind:checked=done />
            "Done"
        </label>
        <select data-ak-id="color-select" bind:value=color class="rounded-lg border px-3 py-2">
            <option value="blue">"Blue"</option>
            <option value="green">"Green"</option>
        </select>
        <form data-ak-id="add-form" on:submit=move |ev| {
            ev.prevent_default();
            add();
        }>
            <input data-ak-id="add-input" bind:value=text />
            <button data-ak-id="add-button" type="submit">"Add"</button>
        </form>
    }
}

// ---- ak-bridge ----
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RenameArgs {
    item_id: u64,
    new_name: String,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Item {
    id: u64,
    name: String,
}

async fn rename(id: u64, name: String) -> Result<Item, ak_bridge::InvokeError> {
    ak_bridge::invoke::<Item>("rename_item", &RenameArgs { item_id: id, new_name: name }).await
}

fn bridge_helpers(todos: RwSignal<Vec<Todo>>) {
    ak_bridge::set_state_reader(move || serde_json::json!({ "todos": todos.get_untracked().len() }));
    ak_bridge::set_hit_test(|p| (p.x < 10.0).then(|| serde_json::json!({ "shape": "a" })));
}

// ---- storage (screens) ----
#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct Note {
    id: u64,
    text: String,
}

#[component]
fn Storage() -> impl IntoView {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct SaveArgs { notes: Vec<Note> }

    let toaster = use_toaster();
    let notes = RwSignal::new(Vec::<Note>::new());
    leptos::task::spawn_local(async move {
        match ak_bridge::invoke::<Vec<Note>>("load_notes", &()).await {
            Ok(list) => notes.set(list),
            Err(e) => toaster.error(e.to_string()),
        }
    });
    let save = move || {
        let args = SaveArgs { notes: notes.get_untracked() };
        leptos::task::spawn_local(async move {
            if let Err(e) = ak_bridge::invoke::<()>("save_notes", &args).await {
                toaster.error(e.to_string());
            }
        });
    };
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ExportArgs {
        file_name: String,
        rows: Vec<Vec<String>>,
    }

    let export = move |_| {
        let mut rows = vec![vec!["Date".to_owned(), "Amount".to_owned()]];
        rows.extend(notes.get_untracked().iter().map(|n| vec![n.id.to_string(), n.text.clone()]));
        let args = ExportArgs { file_name: "notes 2026-10".into(), rows };
        leptos::task::spawn_local(async move {
            match ak_bridge::invoke::<String>("export_csv", &args).await {
                Ok(path) => toaster.success(format!("Saved to {path}")),
                Err(e) => toaster.error(e.to_string()),
            }
        });
    };
    view! {
        <button on:click=move |_| save()>"Save"</button>
        <button on:click=export>"Export"</button>
    }
}

fn save_setting(key: &str, value: &str) {
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = storage.set_item(key, value);
    }
}

// ---- ak-ui ----
#[component]
fn AkUi() -> impl IntoView {
    let busy = RwSignal::new(false);
    let save = move || ();
    let name = RwSignal::new(String::new());
    let email = RwSignal::new(String::new());
    let text = RwSignal::new(String::new());
    let add = move || ();
    let todos = RwSignal::new(Vec::<Todo>::new());
    let open = RwSignal::new(false);
    let clear = move || ();
    let tab = RwSignal::new("all".to_string());
    let dark = RwSignal::new(false);
    let toaster = use_toaster();
    toaster.success("Saved");
    toaster.error("Could not save");
    toaster.info("3 items");
    view! {
        <Button ak_id="save-button" on_click=move |_| save()>"Save"</Button>
        <Button ak_id="delete-button" variant=ButtonVariant::Danger disabled=Signal::derive(move || busy.get())>"Delete"</Button>
        <Button ak_id="add-button" submit=true>"Add"</Button>
        <Button ak_id="close-button" variant=ButtonVariant::Ghost label="Close">"✕"</Button>
        <Input ak_id="name-input" value=name label="Name" placeholder="Ada" />
        <Input ak_id="email-input" value=email kind="email" label="Email" />
        <Form ak_id="add-form" on_submit=move |()| add()>
            <Input ak_id="item-input" value=text label="New item" />
            <Button ak_id="add-button" submit=true>"Add"</Button>
        </Form>
        <List
            ak_id="todo-list"
            items=todos
            key=|t: &Todo| t.id
            row=move |t: Todo| view! { <span class="flex-1">{t.text}</span> }
            empty="Nothing to do yet"
        />
        <List
            ak_id="edit-list"
            items=todos
            key=|t: &Todo| t.id
            row=move |t: Todo| {
                let id = t.id;
                let latest = Memo::new(move |_| todos.with(|v| v.iter().find(|x| x.id == id).cloned()));
                view! {
                    <span class="flex-1">{move || latest.get().map(|x| x.text).unwrap_or_default()}</span>
                    <span>{move || latest.get().map(|x| format!("{}", x.done)).unwrap_or_default()}</span>
                }
            }
        />
        <Button ak_id="clear-button" variant=ButtonVariant::Danger on_click=move |_| open.set(true)>"Clear"</Button>
        <Dialog ak_id="clear-dialog" open=open title="Clear everything?" close_label="Close">
            <p>"This cannot be undone."</p>
            <div class="mt-4 flex justify-end gap-2">
                <Button ak_id="clear-cancel" variant=ButtonVariant::Ghost on_click=move |_| open.set(false)>"Cancel"</Button>
                <Button ak_id="clear-confirm" variant=ButtonVariant::Danger on_click=move |_| { clear(); open.set(false); }>"Clear"</Button>
            </div>
        </Dialog>
        <Nav ak_id="filter-tabs" items=vec![NavItem::new("all", "All"), NavItem::new("done", "Done")] active=tab />
        <Card ak_id="settings-card" title="Settings"> <p>"x"</p> </Card>
        <Toggle ak_id="dark-toggle" checked=dark label="Dark mode" />
    }
}

#[component]
fn ToastWrap() -> impl IntoView {
    view! { <ToastHost ak_id="toasts"> <AkUi /> </ToastHost> }
}

// ---- tailwind / data-ak-id ----
fn classes(urgent: bool) -> &'static str {
    if urgent { "bg-red-500 text-white" } else { "bg-gray-100 text-gray-900" }
}

#[component]
fn Rows(t: Todo) -> impl IntoView {
    view! { <li data-ak-id=format!("todo-{}", t.id)> <button data-ak-id=format!("todo-{}-delete", t.id)>"✕"</button></li> }
}

// ---- date-time ----
fn today() -> String {
    let d = js_sys::Date::new_0();
    format!("{:04}-{:02}-{:02}", d.get_full_year(), d.get_month() + 1, d.get_date())
}

fn shift_month(month: &str, by: i32) -> String {
    let (y, m) = month
        .split_once('-')
        .and_then(|(y, m)| Some((y.parse::<i32>().ok()?, m.parse::<i32>().ok()?)))
        .unwrap_or((1970, 1));
    let n = y * 12 + (m - 1) + by;
    format!("{:04}-{:02}", n.div_euclid(12), n.rem_euclid(12) + 1)
}

fn date_bits() -> (String, String, f64, u32) {
    let d = js_sys::Date::new_0();
    (today()[..7].to_owned(), String::from(d.to_iso_string()), js_sys::Date::now(), d.get_hours())
}

// ---- leptos-reactivity: timers, game state; leptos-components: window keys ----
#[derive(Clone, Default)]
struct Grid {
    cells: [[u8; 10]; 20],
}

impl Grid {
    fn clear_full_rows(&mut self) -> u32 {
        0
    }
}

#[component]
fn Ticker() -> impl IntoView {
    use leptos::ev;
    use std::time::Duration;

    let tick = RwSignal::new(0u32);
    // The running timer, kept to stop it or change its speed.
    let timer = StoredValue::new(None::<IntervalHandle>);
    let stop = move || {
        if let Some(h) = timer.try_get_value().flatten() {
            h.clear();
        }
    };
    // Every `ms` milliseconds; calling it again replaces the old timer.
    let start = move |ms: u64| {
        stop();
        let h = set_interval_with_handle(move || tick.update(|t| *t += 1), Duration::from_millis(ms));
        timer.set_value(h.ok());
    };
    start(500);
    on_cleanup(stop);
    set_timeout(move || tick.set(0), Duration::from_millis(800));
    let game = RwSignal::new(Grid::default());
    let cleared = game.try_update(|g| g.clear_full_rows()).unwrap_or(0);
    let shift = RwSignal::new(0i32);
    let paused = RwSignal::new(false);
    // Keys anywhere in the window, not only on a focused element (games).
    let keys = window_event_listener(ev::keydown, move |e| {
        match e.key().as_str() {
            "ArrowLeft" => shift.set(-1),
            "ArrowRight" => shift.set(1),
            " " => paused.update(|p| *p = !*p),
            _ => return,
        }
        // Otherwise arrows scroll the page and space presses the focused button.
        e.prevent_default();
    });
    on_cleanup(move || keys.remove());
    let toaster = use_toaster();
    let copies = (toaster, toaster);
    view! {
        // A comment inside view!
        <p>{move || tick.get()} {cleared}</p>
    }
}
