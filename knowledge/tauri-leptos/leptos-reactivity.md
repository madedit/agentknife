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
Two effects that copy between the same values (the selected item's name → the
field, the field → the item) set each other off without end. Let one side be
the truth (`bind:value` on the field, a handler writes it back), or write only
when the value really differs: `if item_name != text { … }`.

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
`spawn_local` is not in the prelude: write `leptos::task::spawn_local` (plain
`spawn_local` gives "not found in this scope", `leptos::spawn_local` "not found in
`leptos`"). It takes an `async move { … }` block, not a closure (`async move ||`
gives "`{async closure}` is not a future").

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

## Timers (things that move by themselves)
`set_interval_with_handle` (in `leptos::prelude`) runs a closure every N
milliseconds and returns `Result<IntervalHandle, JsValue>` (not an `Option`).
Keep its handle to stop it, and stop it before starting a new one when the
speed changes:
```rust
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
```
Run one step of the game in the timer (`step()`), not the whole game: a `loop`
that never returns freezes the page. `set_interval` (without `_with_handle`)
returns nothing and cannot be stopped. Once, later: `set_timeout(move || …,
Duration::from_millis(800))`.

## Game state in one signal
Keep the whole state in one struct in an `RwSignal`. `update` changes it and
returns nothing; to get a result out of the change, use `try_update`:
```rust
let cleared = game.try_update(|g| g.clear_full_rows()).unwrap_or(0);
// The closure returns an Option itself: flatten the two.
let next = queue.try_update(|q| q.pop()).flatten();
```
A grid has both sizes in its type: `[[Cell; 10]; 20]` (20 rows of 10);
`[[Cell; 10]]` is a slice and cannot be stored.

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
- A derived value written as a closure (`let has_song = move || !song.get().is_empty();`)
  is called: `has_song()`. It has no `.get()` or `.get_untracked()` ("the method
  `get_untracked` exists for closure … but its trait bounds were not
  satisfied"): those are for signals, `Memo` and `Signal::derive`. In a handler,
  read the signal itself untracked: `!song.get_untracked().is_empty()`.
- "use of moved value" in closures: signals are Copy, but `String`s/`Vec`s are not —
  clone them before the closure (`let name2 = name.clone();`) or keep them in a signal.
- "cannot be sent between threads": values captured by reactive closures must be
  `Send + Sync`; use signals instead of `Rc`/`RefCell`.
- Reading a signal inside its own `update` panics: read first, then update.
- `StoredValue` is for things the screen does not show (a timer handle, a JS
  object): its methods are `get_value`, `set_value`, `with_value`,
  `update_value` (not `get`/`with`/`update`: those give "the method exists but its
  trait bounds were not satisfied"). Lists and sets the screen shows go in an
  `RwSignal`. `with_value` lends it read-only (`*n += 1` there: `cannot assign to
  *n, which is behind a & reference`); `update_value` changes it and returns
  nothing (`let n = seq.update_value(…)`: `expected (), found u64`). A running
  number for new ids:
  ```rust
  let seq = StoredValue::new(0u64);
  let next_id = move || {
      seq.update_value(|n| *n += 1);
      seq.get_value()
  };
  ```
  `get_value()` returns a copy of the whole value (`StoredValue<Option<T>>` gives
  `Option<T>`); `try_get_value()` adds one more `Option` (`None` once the
  component is gone). There is no `take_value`. To get something out of an
  update, use `try_update_value`, which returns what the closure returns; the
  closure of `update_value` returns `()`, so `?` cannot be used in it:
  ```rust
  let drag = StoredValue::new(None::<(f64, f64)>);
  // Takes the value out and leaves None (e.g. at pointerup).
  let end_drag = move || drag.try_update_value(Option::take).flatten();
  ```
  A value that is not `Send` (a canvas's 2D context, a JS `Closure`) needs
  `StoredValue::new_local(…)`; `StoredValue::new` asks for `Send + Sync`.
- `#[derive(Clone, Copy)]` on a struct of state works only when every field is
  `Copy` (signals, `StoredValue`, numbers); a `Cell`, `RefCell`, `String` or `Vec`
  gives E0204 "this field does not implement `Copy`". Use a signal for the field,
  or drop `Copy`.
- Closures in a component can only use closures written above them (`cannot find
  function step` when `stop_edit` calls `step` defined later): write the ones
  that use nothing else first.
- One action for a button and a shortcut key: a closure that captures only `Copy`
  values (signals, `StoredValue`, numbers) is `Copy` itself and can be used in
  both. Otherwise make it a `Callback::new(…)` (or `UnsyncCallback::new(…)` when
  it captures something not `Send`), both in the prelude, and call `.run(x)`.
