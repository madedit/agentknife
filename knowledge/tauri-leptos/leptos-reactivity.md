# Signals and async in Leptos 0.8

## Signals hold state
```rust
let count = RwSignal::new(0);          // read and write in one, and it is Copy
count.get();                           // read (tracked: re-runs closures that read it)
count.set(5);                          // replace
count.update(|n| *n += 1);             // change in place
count.with(|n| *n > 3);                // read by reference (no clone; good for Vec)
count.get_untracked();                 // read without tracking (in event handlers)

let (name, set_name) = signal(String::new());   // separate read and write halves
```
Signals are `Copy`: use them inside many `move` closures without cloning.

## Derived values
```rust
let double = move || count.get() * 2;                   // cheap: a closure
let total = Memo::new(move |_| items.with(|v| v.len())); // cached; for heavier work
view! { <p>{double}</p> <p>{move || total.get()}</p> }
```
Pass a derived value to a component prop with `Signal::derive(move || …)`.

## Collections
Keep lists in `RwSignal<Vec<T>>` and change them with `update`:
```rust
#[derive(Clone, Debug, PartialEq)]
struct Todo { id: u32, text: String, done: bool }

let todos = RwSignal::new(Vec::<Todo>::new());
todos.update(|v| v.push(Todo { id: 1, text: "Milk".into(), done: false }));
todos.update(|v| v.retain(|t| t.id != 1));
todos.update(|v| if let Some(t) = v.iter_mut().find(|t| t.id == 1) { t.done = !t.done });
```
Give items a stable `id` (a counter or a timestamp), never use the position.

## Side effects
```rust
Effect::new(move |_| {
    // Runs after the screen renders, and again whenever `todos` changes.
    let count = todos.with(|v| v.len());
    leptos::logging::log!("{count} todos");
});
```
Use effects for syncing (e.g. saving), not for computing values to show.

## Async work (calling the backend)
Event handlers cannot be `async`; start a task:
```rust
let busy = RwSignal::new(false);
let toaster = use_toaster();   // in the component body
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
```
To load once when a screen opens:
```rust
let loaded = LocalResource::new(|| async {
    ak_bridge::invoke::<Vec<String>>("list_items", &()).await.unwrap_or_default()
});
view! {
    <Suspense fallback=|| view! { <p>"Loading…"</p> }>
        {move || loaded.get().map(|items| view! { <p>{items.len()} " items"</p> })}
    </Suspense>
}
```

## Sharing state between components
```rust
#[derive(Clone, Copy)]
struct AppState { todos: RwSignal<Vec<Todo>> }

// in the top component:
provide_context(AppState { todos: RwSignal::new(vec![]) });
// in any component below it:
let state = expect_context::<AppState>();
```
Or pass signals as props (`todos: RwSignal<Vec<Todo>>`) — they are Copy.

## Common mistakes
- `{count.get()}` in `view!` does not update: write `{move || count.get()}`.
- "use of moved value" in closures: signals are Copy, but `String`s/`Vec`s are not —
  clone them before the closure (`let name2 = name.clone();`) or keep them in a signal.
- "cannot be sent between threads": values captured by reactive closures must be
  `Send + Sync`; use signals instead of `Rc`/`RefCell`.
- Reading a signal inside its own `update` panics: read first, then update.
