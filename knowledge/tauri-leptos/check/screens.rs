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

// ---- browser ----
use wasm_bindgen::JsCast;

/// Copies `text` to the clipboard.
async fn copy_text(text: &str) -> Result<(), String> {
    let clipboard = web_sys::window().ok_or("no window")?.navigator().clipboard();
    wasm_bindgen_futures::JsFuture::from(clipboard.write_text(text))
        .await
        .map(|_| ())
        .map_err(|e| format!("Could not copy: {e:?}"))
}

/// The data-ak-id of the element that has the keyboard focus, if any.
fn focused_id() -> Option<String> {
    let el = web_sys::window()?.document()?.active_element()?;
    el.get_attribute("data-ak-id")
}

/// The `<input>` an event came from (its value, checked state, files).
fn input_of(ev: &web_sys::Event) -> Option<web_sys::HtmlInputElement> {
    ev.target()?.dyn_into::<web_sys::HtmlInputElement>().ok()
}

// ---- leptos-components: one handler, several places; forms: prop:checked ----
#[component]
fn Picker() -> impl IntoView {
    let selected = RwSignal::new(None::<(String, usize)>);
    let chosen = RwSignal::new(std::collections::HashSet::<String>::new());
    let toggle = Callback::new(move |p: String| {
        chosen.update(|c| {
            if !c.remove(&p) {
                c.insert(p);
            }
        })
    });
    let pick = Callback::new(move |(path, index): (String, usize)| {
        selected.set(Some((path, index)));
    });
    let path = "a.jpg".to_string();
    let path2 = path.clone();
    let path3 = path.clone();
    let items = RwSignal::new(vec!["a".to_string()]);
    let queue = RwSignal::new(vec![1u32]);
    let next = queue.try_update(|q| q.pop()).flatten();
    view! {
        <input type="checkbox" on:change=move |_| pick.run((path.clone(), 0)) />
        <span on:click=move |_| pick.run((path2.clone(), 0))>"a.jpg"</span>
        <input type="checkbox" prop:checked=move || chosen.with(|c| c.contains("a.jpg")) on:change=move |_| toggle.run(path3.clone()) />
        <For
            each={move || items.get().into_iter().enumerate().collect::<Vec<_>>()}
            key=|(i, _)| *i
            children=move |(_, s)| view! { <p>{s}</p> }
        />
        <p>{next}</p>
    }
}

// ---- leptos-components: For rows with Show ----
#[component]
fn PhotoRows() -> impl IntoView {
    let photos = RwSignal::new(vec![("a.jpg".to_string(), true)]);
    let opened = RwSignal::new(None::<String>);
    view! {
        <For
            each=move || photos.get()
            key=|(path, _)| path.clone()
            children=move |(path, starred)| {
                // Copy: usable in any number of closures, also inside Show.
                let path = StoredValue::new(path);
                view! {
                    <Show when=move || starred>
                        <button on:click=move |_| opened.set(Some(path.get_value()))>
                            {path.get_value()}
                        </button>
                    </Show>
                }
            }
        />
    }
}

// ---- network ----
use wasm_bindgen_futures::JsFuture;

/// GETs `url` and returns the answer as text; gives up after `timeout_ms`.
async fn fetch_text(url: &str, timeout_ms: u64) -> Result<String, String> {
    let window = web_sys::window().ok_or("no window")?;
    let abort = web_sys::AbortController::new().map_err(|e| format!("{e:?}"))?;
    let init = web_sys::RequestInit::new();
    init.set_signal(Some(&abort.signal()));
    // Cancels the request when the time is up (harmless if it already finished).
    set_timeout(
        move || abort.abort(),
        std::time::Duration::from_millis(timeout_ms),
    );
    let answer = JsFuture::from(window.fetch_with_str_and_init(url, &init))
        .await
        .map_err(|_| "Could not reach the server (no connection, or it took too long).".to_string())?;
    let response: web_sys::Response = answer.dyn_into().map_err(|_| "No answer.".to_string())?;
    if !response.ok() {
        return Err(format!("The server answered with error {}.", response.status()));
    }
    let text = response.text().map_err(|e| format!("{e:?}"))?;
    JsFuture::from(text)
        .await
        .map_err(|e| format!("{e:?}"))?
        .as_string()
        .ok_or_else(|| "The answer was not text.".to_string())
}

#[derive(serde::Deserialize, Clone)]
struct Current {
    temperature_2m: f64,
    time: String, // the place's own clock, "2026-10-23T14:00"
}

#[derive(serde::Deserialize, Clone)]
struct Forecast {
    current: Current,
}

#[component]
fn Weather() -> impl IntoView {
    let toaster = use_toaster();
    let now = RwSignal::new(None::<Current>);
    let load = move || {
        leptos::task::spawn_local(async move {
            let url = "https://api.open-meteo.com/v1/forecast?latitude=25.03&longitude=121.56&current=temperature_2m&timezone=auto";
            match fetch_text(url, 10_000).await.and_then(|t| {
                serde_json::from_str::<Forecast>(&t).map_err(|e| format!("Unexpected answer: {e}"))
            }) {
                Ok(f) => now.set(Some(f.current)),
                Err(e) => toaster.error(e),
            }
        });
    };
    load();
    view! {
        <p data-ak-id="weather-now">
            {move || now.get().map(|c| format!("{}  {:.0}°C", &c.time[11..16], c.temperature_2m))}
        </p>
    }
}

// ---- date-time: another place's time ----
/// The time now at a place `offset_seconds` from UTC (an API's
/// `utc_offset_seconds`), as "14:05".
fn time_at(offset_seconds: i32) -> String {
    let ms = js_sys::Date::now() + f64::from(offset_seconds) * 1000.0;
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms));
    // On the shifted time, the UTC getters read that place's clock.
    format!("{:02}:{:02}", d.get_utc_hours(), d.get_utc_minutes())
}

// ---- ak-ui: List keys ----
fn choose_id(id: i64) -> String {
    ak_ui::part_id("choose", id)
}

// ---- leptos-components: NodeRef ----
#[component]
fn NameField() -> impl IntoView {
    // The element type is required: html::Input, html::Div, html::Canvas…
    let field = NodeRef::<leptos::html::Input>::new();
    view! {
        <input data-ak-id="name-input" node_ref=field />
        <button
            data-ak-id="focus-button"
            on:click=move |_| {
                // None until the element is on the page.
                if let Some(input) = field.get() {
                    let _ = input.focus();
                }
            }
        >
            "Focus"
        </button>
    }
}

// ---- rich-text ----
/// Runs a formatting command on the editor's selected text: "bold", "italic",
/// "underline", "insertUnorderedList", or with a value "foreColor" / "hiliteColor"
/// ("#fde047"), "fontSize" ("5").
fn format_text(editor: NodeRef<leptos::html::Div>, command: &str, value: Option<&str>) {
    let Some(div) = editor.get_untracked() else {
        return;
    };
    // The command works on the focused editor only.
    let _ = div.focus();
    let Some(doc) = html_document() else {
        return;
    };
    let _ = match value {
        Some(v) => doc.exec_command_with_show_ui_and_value(command, false, v),
        None => doc.exec_command(command),
    };
}

/// Whether the selected text has `command` on ("bold"…), for toolbar buttons.
fn is_on(command: &str) -> bool {
    html_document().is_some_and(|d| d.query_command_state(command).unwrap_or(false))
}

fn html_document() -> Option<web_sys::HtmlDocument> {
    web_sys::window()?.document()?.dyn_into().ok()
}

#[component]
fn Editor() -> impl IntoView {
    let editor = NodeRef::<leptos::html::Div>::new();
    let bold = RwSignal::new(false);
    let refresh = move || bold.set(is_on("bold"));
    view! {
        <button
            data-ak-id="bold-button"
            aria-pressed=move || bold.get().to_string()
            // Pressing a button takes the focus and the selection from the editor:
            // keep them where they are.
            on:mousedown=|e: leptos::ev::MouseEvent| e.prevent_default()
            on:click=move |_| {
                format_text(editor, "bold", None);
                refresh();
            }
        >
            "B"
        </button>
        <div
            data-ak-id="editor"
            node_ref=editor
            contenteditable="true"
            class="min-h-40 rounded border p-3"
            on:keyup=move |_| refresh()
            on:mouseup=move |_| refresh()
        ></div>
    }
}

/// Where `needle` appears in `text`, ignoring case, as byte ranges that are safe
/// to slice `text` with (also for Chinese and other non-ASCII text).
fn find_ignore_case(text: &str, needle: &str) -> Vec<std::ops::Range<usize>> {
    let needle: Vec<char> = needle.chars().flat_map(char::to_lowercase).collect();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while !needle.is_empty() && i < chars.len() {
        let mut lowered = Vec::new();
        let mut j = i;
        while j < chars.len() && lowered.len() < needle.len() {
            lowered.extend(chars[j].1.to_lowercase());
            j += 1;
        }
        if lowered == needle {
            let end = chars.get(j).map_or(text.len(), |c| c.0);
            found.push(chars[i].0..end);
            i = j;
        } else {
            i += 1;
        }
    }
    found
}

/// Replaces match number `which` (counted from 0) of `needle` in `text`.
fn replace_nth(text: &str, needle: &str, which: usize, with: &str) -> Option<String> {
    let range = find_ignore_case(text, needle).into_iter().nth(which)?;
    Some(format!("{}{with}{}", &text[..range.start], &text[range.end..]))
}

// ---- browser: exact types ----
/// Text inside a node: methods live on the exact type (`Text`, `HtmlElement`…).
fn add_text(node: &web_sys::Node, more: &str) {
    match node.dyn_ref::<web_sys::Text>() {
        Some(text) => {
            let _ = text.append_data(more);
        }
        None => {
            let old = node.text_content().unwrap_or_default();
            node.set_text_content(Some(&format!("{old}{more}")));
        }
    }
}

// ---- ak-ui: an event prop whose work returns a value ----
#[component]
fn CountButton() -> impl IntoView {
    let n = RwSignal::new(0);
    let bump = move |_| {
        n.try_update(|x| *x += 1);
    };
    view! { <Button ak_id="count-button" on_click=bump>"+1"</Button> }
}

// ---- leptos-components: menus, class strings, event types, into props ----
#[component]
fn ViewMenu() -> impl IntoView {
    let open = RwSignal::new(false);
    let outside = window_event_listener(leptos::ev::pointerdown, move |_| open.set(false));
    let esc = window_event_listener(leptos::ev::keydown, move |e| {
        if e.key() == "Escape" {
            open.set(false);
        }
    });
    on_cleanup(move || {
        outside.remove();
        esc.remove();
    });
    view! {
        <div class="relative" on:pointerdown=|e| e.stop_propagation()>
            <button data-ak-id="view-menu-button" on:click=move |_| open.update(|o| *o = !*o)>
                "View"
            </button>
            <div
                data-ak-id="view-menu"
                class="absolute z-10 mt-1 w-40 rounded-lg border bg-white shadow"
                class:hidden=move || !open.get()
            >
                <button data-ak-id="zoom-in-item" class="block w-full px-3 py-2 text-left"
                    on:click=move |_| open.set(false)>
                    "Zoom in"
                </button>
            </div>
        </div>
    }
}

#[component]
fn Swatch(#[prop(into)] ak_id: String, #[prop(into)] name: String) -> impl IntoView {
    let on = RwSignal::new(false);
    let px = "24".to_string();
    view! {
        <canvas
            data-ak-id=ak_id
            width=px.clone()
            height=px
            class=move || format!("rounded {}", if on.get() { "bg-[#535353]" } else { "bg-blue-500/35" })
            on:pointerdown=move |_e: leptos::ev::PointerEvent| on.set(true)
            on:wheel=move |_e: leptos::ev::WheelEvent| {}
        ></canvas>
        <span>{name}</span>
    }
}

#[component]
fn Swatches() -> impl IntoView {
    let rows = vec![("red".to_string(), 1u32)];
    view! {
        <Swatch ak_id="main-swatch" name="marquee" />
        <For
            each=move || rows.clone()
            key=|(name, _)| name.clone()
            children=move |(name, _)| view! { <span>{name}</span> }
        />
    }
}

// ---- drag-drop ----
#[derive(Clone, PartialEq)]
struct Card {
    id: u32,
    column: usize,
    text: String,
}

const COLUMNS: [&str; 3] = ["To do", "Doing", "Done"];

#[component]
fn Board() -> impl IntoView {
    let cards = RwSignal::new(vec![Card { id: 1, column: 0, text: "Buy milk".into() }]);
    // The card being dragged: set on dragstart, cleared on dragend.
    let dragging = RwSignal::new(None::<u32>);
    // Only signals inside: `Copy`, usable in every handler below.
    let move_card = move |id: u32, column: usize| {
        cards.update(|v| {
            if let Some(c) = v.iter_mut().find(|c| c.id == id) {
                c.column = column;
            }
        });
        // Save here (topic `storage`).
    };
    let columns = COLUMNS.iter().enumerate().map(|(col, name)| {
        view! {
            <section
                data-ak-id=ak_ui::part_id("column", col)
                class="min-h-40 rounded-xl bg-gray-100 p-3"
                // A place that takes drops calls prevent_default on dragover.
                on:dragover=move |e| e.prevent_default()
                on:drop=move |e| {
                    e.prevent_default();
                    // Move only here, on a real drop (not on dragend).
                    if let Some(id) = dragging.get_untracked() {
                        move_card(id, col);
                    }
                    dragging.set(None);
                }
            >
                <h2 class="mb-2 font-semibold">{*name}</h2>
                <For
                    each={move || cards.get().into_iter().filter(|c| c.column == col).collect::<Vec<_>>()}
                    key=|c| c.id
                    children=move |c: Card| {
                        let id = c.id;
                        view! {
                            <div
                                data-ak-id=ak_ui::part_id("card", id)
                                draggable="true"
                                class="mb-2 flex cursor-grab items-center justify-between rounded-lg bg-white p-2 shadow"
                                class:opacity-50=move || dragging.get() == Some(id)
                                on:dragstart=move |e| {
                                    dragging.set(Some(id));
                                    // Some browsers start a drag only with data in it.
                                    if let Some(data) = e.data_transfer() {
                                        let _ = data.set_data("text/plain", &id.to_string());
                                    }
                                }
                                on:dragend=move |_| dragging.set(None)
                            >
                                {c.text}
                                // The way without dragging: keyboard users, and AgentKnife's checks.
                                <button
                                    data-ak-id=ak_ui::part_id("card-right", id)
                                    aria-label="Move right"
                                    class="rounded px-1 hover:bg-gray-100"
                                    on:click=move |_| move_card(id, (col + 1).min(COLUMNS.len() - 1))
                                >
                                    "→"
                                </button>
                            </div>
                        }
                    }
                />
            </section>
        }
    });
    view! { <div class="grid grid-cols-3 gap-4">{columns.collect_view()}</div> }
}

// ---- leptos-reactivity: StoredValue counter; ak-ui: Dialog on_close, small ----
#[component]
fn EditDialog() -> impl IntoView {
    let seq = StoredValue::new(0u64);
    let next_id = move || {
        seq.update_value(|n| *n += 1);
        seq.get_value()
    };
    let _ = next_id();
    let editing = RwSignal::new(None::<u32>);
    let open = RwSignal::new(false);
    let edit = move |id: u32| {
        editing.set(Some(id));
        open.set(true);
    };
    view! {
        <Button ak_id="card-edit" variant=ButtonVariant::Ghost small=true on_click=move |_| edit(1)>"Edit"</Button>
        <Dialog ak_id="edit-dialog" open=open title="Edit card" close_label="Close"
            on_close=move |_| editing.set(None)>
            <p>{move || editing.get().map(|id| format!("Card {id}"))}</p>
        </Dialog>
    }
}

#[component]
fn MiniButton(#[prop(into)] ak_id: String, #[prop(into)] on_click: Callback<()>) -> impl IntoView {
    view! { <button data-ak-id=ak_id on:click=move |_| on_click.run(())>"+"</button> }
}

#[component]
fn UsesMiniButton() -> impl IntoView {
    let n = RwSignal::new(0);
    let id = StoredValue::new("todo".to_string());
    view! {
        <MiniButton ak_id="mini" on_click=move |_| n.update(|x| *x += 1) />
        <p class:font-bold=move || id.get_value() == "todo" class:italic=move || id.get_value().is_empty()>
            {move || n.get()}
        </p>
    }
}

// ---- canvas ----
use wasm_bindgen::closure::Closure;

#[derive(Clone, Copy, PartialEq)]
struct Camera {
    x: f64,
    y: f64,
    zoom: f64,
}

#[derive(Clone, PartialEq)]
struct Node {
    id: u32,
    x: f64,
    y: f64,
    label: String,
}

#[component]
fn Stage() -> impl IntoView {
    let camera = RwSignal::new(Camera { x: 0.0, y: 0.0, zoom: 1.0 });
    let nodes = RwSignal::new(vec![
        Node { id: 1, x: 40.0, y: 40.0, label: "Start".into() },
        Node { id: 2, x: 300.0, y: 140.0, label: "End".into() },
    ]);
    let links = RwSignal::new(vec![(1u32, 2u32)]);
    let selected = RwSignal::new(None::<(u32, u32)>);
    // While dragging: (node id, pointer's offset from the node's corner).
    let dragging = StoredValue::new(None::<(u32, f64, f64)>);
    let stage = NodeRef::<leptos::html::Div>::new();

    // A point on the screen → the same point on the drawing.
    let to_drawing = move |cx: i32, cy: i32| {
        let (left, top) = stage
            .get_untracked()
            .map(|s| s.get_bounding_client_rect())
            .map_or((0.0, 0.0), |r| (r.left(), r.top()));
        let c = camera.get_untracked();
        ((f64::from(cx) - left - c.x) / c.zoom, (f64::from(cy) - top - c.y) / c.zoom)
    };

    // Wheel zoom: a listener that may call prevent_default (passive: false).
    // This effect runs once, when the stage is on the page: one listener.
    Effect::new(move |_| {
        let Some(el) = stage.get() else { return };
        let on_wheel = Closure::<dyn Fn(web_sys::WheelEvent)>::new(move |e: web_sys::WheelEvent| {
            // Otherwise the page scrolls (or the whole view zooms) as well.
            e.prevent_default();
            let step = if e.delta_y() < 0.0 { 1.1 } else { 1.0 / 1.1 };
            camera.update(|c| c.zoom = (c.zoom * step).clamp(0.25, 4.0));
        });
        let options = web_sys::AddEventListenerOptions::new();
        options.set_passive(false);
        let _ = el.add_event_listener_with_callback_and_add_event_listener_options(
            "wheel",
            on_wheel.as_ref().unchecked_ref(),
            &options,
        );
        // Kept as long as the page: the stage is made once.
        on_wheel.forget();
    });

    let on_down = move |e: leptos::ev::PointerEvent| {
        // The node under the pointer (the press may be on its label).
        let id = e
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .and_then(|t| t.closest("[data-node]").ok().flatten())
            .and_then(|n| n.get_attribute("data-node"))
            .and_then(|s| s.parse::<u32>().ok());
        let Some(id) = id else { return };
        let (px, py) = to_drawing(e.client_x(), e.client_y());
        let corner = nodes.with_untracked(|v| v.iter().find(|n| n.id == id).map(|n| (n.x, n.y)));
        if let Some((x, y)) = corner {
            dragging.set_value(Some((id, px - x, py - y)));
            // Moves and the release keep coming to the stage, even outside the window.
            if let Some(s) = stage.get_untracked() {
                let _ = s.set_pointer_capture(e.pointer_id());
            }
        }
    };
    let on_move = move |e: leptos::ev::PointerEvent| {
        let Some((id, dx, dy)) = dragging.get_value() else { return };
        let (px, py) = to_drawing(e.client_x(), e.client_y());
        nodes.update(|v| {
            if let Some(n) = v.iter_mut().find(|n| n.id == id) {
                n.x = px - dx;
                n.y = py - dy;
            }
        });
    };
    let on_up = move |e: leptos::ev::PointerEvent| {
        if dragging.get_value().is_some() {
            dragging.set_value(None);
            if let Some(s) = stage.get_untracked() {
                let _ = s.release_pointer_capture(e.pointer_id());
            }
            // Save here (topic `storage`).
        }
    };

    view! {
        <div
            node_ref=stage
            data-ak-id="stage"
            class="relative h-96 w-full touch-none select-none overflow-hidden bg-gray-50"
            on:pointerdown=on_down
            on:pointermove=on_move
            on:pointerup=on_up
        >
            // One transform moves and scales everything drawn.
            <div
                class="absolute left-0 top-0 origin-top-left"
                style:transform=move || {
                    let c = camera.get();
                    format!("translate({}px, {}px) scale({})", c.x, c.y, c.zoom)
                }
            >
                // The lines: the layer lets the pointer through, only the hit paths take it.
                <svg class="pointer-events-none absolute left-0 top-0 overflow-visible" width="1" height="1">
                    <For
                        each=move || links.get()
                        key=|l| *l
                        children=move |(a, b): (u32, u32)| {
                            let d = move || nodes.with(|v| {
                                let end = |id: u32| v.iter().find(|n| n.id == id).map_or((0.0, 0.0), |n| (n.x + 64.0, n.y + 20.0));
                                let ((x1, y1), (x2, y2)) = (end(a), end(b));
                                format!("M {x1} {y1} C {} {y1}, {} {y2}, {x2} {y2}", x1 + 80.0, x2 - 80.0)
                            });
                            view! {
                                // Wide and invisible: easy to hit.
                                <path
                                    d=d
                                    data-ak-id=ak_ui::part_id("link", format!("{a}-{b}"))
                                    stroke="transparent"
                                    stroke-width="16"
                                    fill="none"
                                    class="pointer-events-auto cursor-pointer"
                                    on:click=move |_| selected.set(Some((a, b)))
                                />
                                // What is seen.
                                <path
                                    d=d
                                    fill="none"
                                    stroke-width="2"
                                    stroke=move || if selected.get() == Some((a, b)) { "#2563eb" } else { "#64748b" }
                                />
                            }
                        }
                    />
                </svg>
                <For
                    each=move || nodes.get()
                    key=|n| n.id
                    children=move |n: Node| {
                        let id = n.id;
                        let at = move || nodes.with(|v| v.iter().find(|m| m.id == id).map_or((0.0, 0.0), |m| (m.x, m.y)));
                        view! {
                            <div
                                data-node=id.to_string()
                                data-ak-id=ak_ui::part_id("node", id)
                                class="absolute w-32 cursor-move rounded-lg border bg-white px-3 py-2 shadow"
                                style:left=move || format!("{}px", at().0)
                                style:top=move || format!("{}px", at().1)
                            >
                                {n.label}
                            </div>
                        }
                    }
                />
            </div>
        </div>
    }
}

/// Whether the user is typing in a field: then keys are theirs, not shortcuts.
fn is_typing() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element())
        .is_some_and(|el| {
            matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT")
                || el.has_attribute("contenteditable")
        })
}

#[component]
fn NamePanel() -> impl IntoView {
    let name_field = NodeRef::<leptos::html::Input>::new();
    Effect::new(move |_| {
        if let Some(input) = name_field.get() {
            let _ = input.focus();
        }
    });
    view! { <input data-ak-id="node-name" node_ref=name_field /> }
}

// ==== generated from new topics (compose_checks.py) ====

// ---- reports ----
/// Colours for the series, in order. Written as colours, not Tailwind classes:
/// a chart saved as a picture has no Tailwind.
const COLOURS: [&str; 6] = ["#0ea5e9", "#f59e0b", "#10b981", "#ef4444", "#8b5cf6", "#64748b"];

/// Bars for (label, value) pairs; values from 0 up.
#[component]
fn BarChart(#[prop(into)] ak_id: String, #[prop(into)] data: Signal<Vec<(String, f64)>>) -> impl IntoView {
    const W: f64 = 400.0; // drawing units, not pixels: the chart scales to its box
    const H: f64 = 200.0;
    const TOP: f64 = 16.0; // room for the value above the tallest bar
    view! {
        <svg data-ak-id=ak_id xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 240" class="w-full h-auto">
            {move || {
                let rows = data.get();
                let max = rows.iter().map(|r| r.1).fold(0.0, f64::max);
                let slot = W / rows.len().max(1) as f64;
                rows.into_iter()
                    .enumerate()
                    .map(|(i, (label, value))| {
                        let h = if max > 0.0 { value / max * H } else { 0.0 };
                        let mid = (i as f64 + 0.5) * slot;
                        view! {
                            <rect x={mid - slot * 0.35} y={TOP + H - h} width={slot * 0.7} height=h fill={COLOURS[0]} />
                            <text x=mid y={TOP + H - h - 4.0} text-anchor="middle" font-size="11" fill="#111827">
                                {format!("{value:.0}")}
                            </text>
                            <text x=mid y={TOP + H + 16.0} text-anchor="middle" font-size="12" fill="#4b5563">
                                {label}
                            </text>
                        }
                    })
                    .collect_view()
            }}
        </svg>
    }
}

/// Shares of a whole as a ring: each slice is a circle whose dashed border shows
/// only its part (no angles to compute; one slice of 100% works too).
#[component]
fn PieChart(#[prop(into)] ak_id: String, #[prop(into)] data: Signal<Vec<(String, f64)>>) -> impl IntoView {
    view! {
        <div class="flex items-center gap-4">
            // r = 15.915 makes the circle 100 units around: a slice of 30% is 30 long.
            <svg data-ak-id=ak_id xmlns="http://www.w3.org/2000/svg" viewBox="0 0 42 42" class="w-40 h-40 -rotate-90">
                {move || {
                    let rows = data.get();
                    let total: f64 = rows.iter().map(|r| r.1).sum();
                    let mut start = 0.0;
                    rows.iter()
                        .enumerate()
                        .map(|(i, (_, value))| {
                            let part = if total > 0.0 { value / total * 100.0 } else { 0.0 };
                            let offset = -start;
                            start += part;
                            view! {
                                <circle cx="21" cy="21" r="15.915" fill="none" stroke-width="6"
                                    stroke={COLOURS[i % COLOURS.len()]}
                                    stroke-dasharray={format!("{part} {}", 100.0 - part)}
                                    stroke-dashoffset=offset />
                            }
                        })
                        .collect_view()
                }}
            </svg>
            <ul class="text-sm">
                {move || {
                    let rows = data.get();
                    let total: f64 = rows.iter().map(|r| r.1).sum();
                    rows.into_iter()
                        .enumerate()
                        .map(|(i, (label, value))| {
                            let pct = if total > 0.0 { value / total * 100.0 } else { 0.0 };
                            view! {
                                <li class="flex items-center gap-2">
                                    <span class="inline-block w-3 h-3 rounded-sm" style:background={COLOURS[i % COLOURS.len()]}></span>
                                    {format!("{label} {pct:.0}%")}
                                </li>
                            }
                        })
                        .collect_view()
                }}
            </ul>
        </div>
    }
}

/// A line through (label, value) points, e.g. one per month.
#[component]
fn LineChart(#[prop(into)] ak_id: String, #[prop(into)] data: Signal<Vec<(String, f64)>>) -> impl IntoView {
    const W: f64 = 400.0;
    const H: f64 = 200.0;
    const TOP: f64 = 10.0;
    view! {
        <svg data-ak-id=ak_id xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 236" class="w-full h-auto">
            {move || {
                let rows = data.get();
                let max = rows.iter().map(|r| r.1).fold(0.0, f64::max);
                let step = W / rows.len().max(1) as f64;
                let at = |i: usize, v: f64| {
                    let y = if max > 0.0 { TOP + H - v / max * H } else { TOP + H };
                    ((i as f64 + 0.5) * step, y)
                };
                let points: Vec<String> = rows
                    .iter()
                    .enumerate()
                    .map(|(i, r)| {
                        let (x, y) = at(i, r.1);
                        format!("{x:.1},{y:.1}")
                    })
                    .collect();
                let labels = rows
                    .iter()
                    .enumerate()
                    .map(|(i, (label, value))| {
                        let (x, y) = at(i, *value);
                        view! {
                            <circle cx=x cy=y r="3" fill={COLOURS[0]} />
                            <text x=x y={TOP + H + 18.0} text-anchor="middle" font-size="12" fill="#4b5563">
                                {label.clone()}
                            </text>
                        }
                    })
                    .collect_view();
                view! {
                    <polyline points={points.join(" ")} fill="none" stroke={COLOURS[0]} stroke-width="2" />
                    {labels}
                }
            }}
        </svg>
    }
}

#[derive(Clone, PartialEq)]
struct Expense {
    category: String,
    amount: f64,
}

#[component]
fn Report() -> impl IntoView {
    let expenses = RwSignal::new(vec![
        Expense { category: "Food".into(), amount: 320.0 },
        Expense { category: "Bus".into(), amount: 80.0 },
    ]);
    // One (label, total) per category, biggest first.
    let by_category = Memo::new(move |_| {
        let mut totals: Vec<(String, f64)> = Vec::new();
        for e in expenses.get() {
            match totals.iter_mut().find(|t| t.0 == e.category) {
                Some(t) => t.1 += e.amount,
                None => totals.push((e.category, e.amount)),
            }
        }
        totals.sort_by(|a, b| b.1.total_cmp(&a.1));
        totals
    });
    view! {
        <BarChart ak_id="chart-by-category" data=by_category />
        <PieChart ak_id="chart-shares" data=by_category />
    }
}

#[component]
fn PrintButton() -> impl IntoView {
    let print = move |_| {
        if let Some(w) = web_sys::window() {
            let _ = w.print();
        }
    };
    // <Button ak_id="print-report" on_click=print>"Print / PDF"</Button>
    view! { <Button ak_id="print-report" on_click=print>"Print / PDF"</Button> }
}

/// The SVG text of the element with this `data-ak-id`.
fn svg_text(ak_id: &str) -> Option<String> {
    let doc = web_sys::window()?.document()?;
    let el = doc.query_selector(&format!("[data-ak-id='{ak_id}']")).ok()??;
    Some(el.outer_html())
}

// ---- long-lists ----
#[derive(Clone, PartialEq)]
struct Contact {
    id: u32,
    name: String,
    city: String,
    spent: f64,
}

const PAGE: usize = 50;

#[component]
fn Contacts() -> impl IntoView {
    let all = RwSignal::new(Vec::<Contact>::new()); // loaded from the backend
    let query = RwSignal::new(String::new());
    let sort = RwSignal::new("name".to_string());
    let page = RwSignal::new(0usize);

    // Runs again only when the rows, the search or the sort change.
    let found = Memo::new(move |_| {
        let q = query.get().trim().to_lowercase();
        let mut rows: Vec<Contact> = all.with(|rows| {
            rows.iter()
                .filter(|c| {
                    q.is_empty() || c.name.to_lowercase().contains(&q) || c.city.to_lowercase().contains(&q)
                })
                .cloned()
                .collect()
        });
        match sort.get().as_str() {
            // Numbers as numbers: as text, "100" comes before "9".
            "spent" => rows.sort_by(|a, b| b.spent.total_cmp(&a.spent)),
            _ => rows.sort_by(|a, b| a.name.cmp(&b.name)),
        }
        rows
    });
    let pages = move || found.with(|f| f.len().div_ceil(PAGE).max(1));
    let shown = move || {
        found.with(|f| f.iter().skip(page.get() * PAGE).take(PAGE).cloned().collect::<Vec<_>>())
    };

    view! {
        <input data-ak-id="contact-search" type="search" placeholder="Search"
            prop:value=move || query.get()
            // A new search starts on the first page.
            on:input=move |e| { query.set(event_target_value(&e)); page.set(0); } />
        <select data-ak-id="contact-sort" on:change=move |e| { sort.set(event_target_value(&e)); page.set(0); }>
            <option value="name">"Name"</option>
            <option value="spent">"Spent"</option>
        </select>
        <p data-ak-id="contact-count">{move || format!("{} found", found.with(|f| f.len()))}</p>
        <ul>
            <For
                each=shown
                key=|c| c.id
                children=move |c: Contact| view! {
                    <li data-ak-id=format!("contact-{}", c.id)>{c.name} " · " {c.city}</li>
                }
            />
        </ul>
        <Show when=move || found.with(|f| f.is_empty())>
            <p data-ak-id="contact-none">"Nothing matches the search."</p>
        </Show>
        <div class="flex gap-2 items-center">
            <button data-ak-id="page-prev" disabled=move || page.get() == 0
                on:click=move |_| page.update(|p| *p = p.saturating_sub(1))>"Previous"</button>
            <span data-ak-id="page-number">{move || format!("{} / {}", page.get() + 1, pages())}</span>
            <button data-ak-id="page-next" disabled=move || page.get() + 1 >= pages()
                on:click=move |_| page.update(|p| *p += 1)>"Next"</button>
        </div>
    }
}

// ---- undo-autosave ----
/// Copies of the data before each change (past) and after each undo (future).
struct History<T> {
    past: Vec<T>,
    future: Vec<T>,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self { past: Vec::new(), future: Vec::new() }
    }
}

impl<T> History<T> {
    const LIMIT: usize = 100;

    /// Call with the data as it was *before* a change.
    fn record(&mut self, before: T) {
        self.past.push(before);
        if self.past.len() > Self::LIMIT {
            self.past.remove(0);
        }
        self.future.clear(); // a new change ends the redo steps
    }
    fn undo(&mut self, now: T) -> Option<T> {
        let before = self.past.pop()?;
        self.future.push(now);
        Some(before)
    }
    fn redo(&mut self, now: T) -> Option<T> {
        let after = self.future.pop()?;
        self.past.push(now);
        Some(after)
    }
}

#[derive(Clone, PartialEq)]
struct Shape {
    id: u32,
    x: f64,
    y: f64,
    label: String,
}

/// The document and its history. Only signals and stored values inside, so it is
/// `Copy`: create it once in the component and use it in every handler.
#[derive(Clone, Copy)]
struct Doc {
    shapes: RwSignal<Vec<Shape>>,
    history: StoredValue<History<Vec<Shape>>>,
    /// (undo steps, redo steps), for the buttons.
    steps: RwSignal<(usize, usize)>,
    /// True while the user types in one field: the whole edit is one undo step.
    typing: StoredValue<bool>,
}

impl Doc {
    fn new(shapes: Vec<Shape>) -> Self {
        Self {
            shapes: RwSignal::new(shapes),
            history: StoredValue::new(History::default()),
            steps: RwSignal::new((0, 0)),
            typing: StoredValue::new(false),
        }
    }

    /// Every change goes through here, so each one can be undone.
    fn change(self, f: impl FnOnce(&mut Vec<Shape>)) {
        let before = self.shapes.get_untracked();
        self.shapes.update(f);
        self.history.update_value(|h| h.record(before));
        self.typing.set_value(false);
        self.changed();
    }

    /// For `on:input`: the first key records, the next ones do not.
    fn type_text(self, f: impl FnOnce(&mut Vec<Shape>)) {
        if !self.typing.get_value() {
            let before = self.shapes.get_untracked();
            self.history.update_value(|h| h.record(before));
            self.typing.set_value(true);
        }
        self.shapes.update(f);
        self.changed();
    }

    /// For a drag: call once when the drag really starts (the first move, not
    /// `pointerdown`: a click that only selects changes nothing), then move with
    /// `shapes.update` until `pointerup`. The whole drag is one step.
    fn checkpoint(self) {
        let now = self.shapes.get_untracked();
        self.history.update_value(|h| h.record(now));
        self.changed();
    }

    /// For the field's `on:blur`: the next typing is a new step.
    fn stop_typing(self) {
        self.typing.set_value(false);
    }

    fn undo(self) {
        let now = self.shapes.get_untracked();
        if let Some(before) = self.history.try_update_value(|h| h.undo(now)).flatten() {
            self.shapes.set(before);
            self.changed();
        }
    }

    fn redo(self) {
        let now = self.shapes.get_untracked();
        if let Some(after) = self.history.try_update_value(|h| h.redo(now)).flatten() {
            self.shapes.set(after);
            self.changed();
        }
    }

    fn changed(self) {
        let steps = self.history.with_value(|h| (h.past.len(), h.future.len()));
        self.steps.set(steps);
        // and save (below)
    }
}

#[component]
fn UndoScreen() -> impl IntoView {
    let doc = Doc::new(Vec::new());
    // Ctrl+Z, Ctrl+Y and Ctrl+Shift+Z anywhere in the window. In a text field they
    // are the field's own undo (is_typing, topic `canvas`).
    let keys = window_event_listener(leptos::ev::keydown, move |e| {
        if !(e.ctrl_key() || e.meta_key()) || is_typing() {
            return;
        }
        match e.key().to_lowercase().as_str() {
            "z" if e.shift_key() => doc.redo(),
            "z" => doc.undo(),
            "y" => doc.redo(),
            _ => return,
        }
        e.prevent_default();
    });
    on_cleanup(move || keys.remove());

    let add = move |_| doc.change(|s| s.push(Shape { id: 1, x: 0.0, y: 0.0, label: "Box".into() }));
    let rename = move |id: u32, text: String| {
        doc.type_text(move |s| {
            if let Some(shape) = s.iter_mut().find(|s| s.id == id) {
                shape.label = text;
            }
        })
    };
    // <button on:click=add>"Add box"</button>
    // <input on:input=move |e| rename(id, event_target_value(&e)) on:blur=move |_| doc.stop_typing() />
    // Buttons: disabled=move || doc.steps.get().0 == 0 (undo), .1 == 0 (redo)
    let _ = rename;
    view! { <button on:click=add>"Add"</button> <p>{move || doc.steps.get().0}</p> }
}

/// Saves `save()` 600 ms after the last call; `dirty` is true until then.
#[derive(Clone, Copy)]
struct AutoSave {
    timer: StoredValue<Option<TimeoutHandle>>,
    dirty: RwSignal<bool>,
}

impl AutoSave {
    fn new() -> Self {
        Self { timer: StoredValue::new(None), dirty: RwSignal::new(false) }
    }

    fn later(self, save: impl Fn() + 'static) {
        if let Some(t) = self.timer.get_value() {
            t.clear();
        }
        self.dirty.set(true);
        let dirty = self.dirty;
        let t = set_timeout_with_handle(
            move || {
                save();
                dirty.set(false);
            },
            std::time::Duration::from_millis(600),
        );
        self.timer.set_value(t.ok());
    }
}

fn save_draft(text: &str) {
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.set_item("draft-note", text);
    }
}

fn take_draft() -> Option<String> {
    let s = web_sys::window()?.local_storage().ok()??;
    let text = s.get_item("draft-note").ok()??;
    let _ = s.remove_item("draft-note");
    (!text.is_empty()).then_some(text)
}

// ---- import-backup (screens) ----
/// "1,234.5", "$80", " 12 " → a number; None if it is not one.
fn parse_amount(cell: &str) -> Option<f64> {
    let s: String = cell.chars().filter(|c| !matches!(c, ',' | '$' | '¥' | '元' | ' ')).collect();
    s.parse().ok()
}

/// "2026/10/5", "2026-10-05" → "2026-10-05".
fn parse_date(cell: &str) -> Option<String> {
    let parts: Vec<u32> = cell.trim().split(['/', '-', '.']).map(|p| p.parse().ok()).collect::<Option<_>>()?;
    match parts[..] {
        [y, m, d] if y > 1900 && (1..=12).contains(&m) && (1..=31).contains(&d) => {
            Some(format!("{y:04}-{m:02}-{d:02}"))
        }
        _ => None,
    }
}

/// The column whose header is one of `names`.
fn column(header: &[String], names: &[&str]) -> Option<usize> {
    header.iter().position(|h| names.contains(&h.trim()))
}

// ---- leptos-components: copy and paste ----
fn clipboard_listeners(
    selected_text: impl Fn() -> String + 'static,
    paste_text: impl Fn(&str) + 'static,
) {
    use leptos::ev;
    let copy = window_event_listener(ev::copy, move |e| {
        if is_typing() {
            return; // a text field copies its own text
        }
        if let Some(data) = e.clipboard_data() {
            let _ = data.set_data("text/plain", &selected_text());
            e.prevent_default();
        }
    });
    let paste = window_event_listener(ev::paste, move |e| {
        if is_typing() {
            return;
        }
        if let Some(text) = e.clipboard_data().and_then(|d| d.get_data("text/plain").ok()) {
            paste_text(&text); // cells from Excel: rows split by '\n', columns by '\t'
            e.prevent_default();
        }
    });
    on_cleanup(move || {
        copy.remove();
        paste.remove();
    });
}

// ---- leptos-reactivity: taking a value out of a StoredValue ----
fn drag_end() -> Option<(f64, f64)> {
    let drag = StoredValue::new(None::<(f64, f64)>);
    // Takes the value out and leaves None (e.g. at pointerup).
    let end_drag = move || drag.try_update_value(Option::take).flatten();
    end_drag()
}

// ---- canvas: drawing on a <canvas> ----
const ROW: f64 = 24.0;

/// The 2D drawing context of a `<canvas>`.
fn context_2d(canvas: &web_sys::HtmlCanvasElement) -> Option<web_sys::CanvasRenderingContext2d> {
    use wasm_bindgen::JsCast;
    canvas.get_context("2d").ok()??.dyn_into().ok()
}

/// Draws only the rows on screen: `first..first + count`.
fn draw_rows(
    ctx: &web_sys::CanvasRenderingContext2d,
    width: f64,
    first: usize,
    count: usize,
    cell: impl Fn(usize) -> String,
) {
    ctx.clear_rect(0.0, 0.0, width, count as f64 * ROW);
    ctx.set_font("14px sans-serif");
    for i in 0..count {
        let y = i as f64 * ROW;
        ctx.set_stroke_style_str("#e5e7eb");
        ctx.stroke_rect(0.0, y, width, ROW);
        ctx.set_fill_style_str("#111827");
        let _ = ctx.fill_text(&cell(first + i), 6.0, y + 17.0);
    }
}
