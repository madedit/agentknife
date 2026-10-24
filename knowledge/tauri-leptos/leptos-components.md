# Leptos components and view! (Leptos 0.8, client-side rendering)

Start every screen file with `use leptos::prelude::*;`.

## Components
```rust
use leptos::prelude::*;

/// A component is a function marked #[component] returning `impl IntoView`.
/// Its name is CamelCase; each argument is a prop.
#[component]
fn Greeting(
    /// Required prop.
    name: String,
    /// Optional prop: may be left out (defaults to Default::default()).
    #[prop(optional)]
    excited: bool,
    /// `into` accepts anything convertible: &str, String…
    #[prop(into, optional)]
    note: Option<String>,
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
```
- A prop with `#[prop(into)]` takes the value as it is: `note="Welcome back"`.
  **Not** `"text".into()`: that fails with `type annotations needed: multiple impls
  satisfying _: From<&str>` (the compiler's `<&str as Into<T>>` hint does not help).
- A `String` prop without `into` needs `"Ada".to_string()` (`expected String, found
  &str`). In your own components give text props `#[prop(into)]`, so
  `ak_id="save-button"` works as with ak-ui.

## The view! macro
- Text in quotes: `"Hello"`. Rust values in braces: `{name}`.
- **Reactive** values are closures: `{move || count.get()}`. A plain `{count.get()}`
  is read once and never updates.
- Attributes: `class="…"`, `data-ak-id="…"`, `disabled=move || busy.get()`,
  `placeholder=hint` (a variable).
- Class toggles: `class:hidden=move || !open.get()`. For several conditions use one
  closure returning the whole class string (every class written out in full):
  `class=move || format!("rounded p-2 {}", if hp.get() <= 3 { "bg-red-500" } else { "bg-gray-100" })`.
  After `class:` only a plain class name works. A Tailwind class with `[ ] # . / %`
  in it (`bg-[#535353]`, `text-[12px]`, `bg-blue-500/35`) fails with `invalid tag
  name or attribute key` and `tokens was ignored during parsing`: put it in the
  closure that returns the whole class string, as above.
- Attribute and prop values are moved in: one `String` for two attributes,
  `width=px height=px`, fails with `use of moved value`: `width=px.clone() height=px`.
- **Any attribute or prop value with `<` or `>` in it goes in braces**, on every
  element and component. That includes comparisons and types:
  `when={move || total.get() > 0.0}`,
  `each={move || items.get().into_iter().enumerate().collect::<Vec<_>>()}`.
  Without them `view!` cuts the value at the `<`/`>` and the error looks unrelated
  (`invalid tag name or attribute key`, `expected bool, found f64`, `expected
  identifier, found keyword move`), often on a later line: check the other
  attributes of the same element.
- Events: `on:click=move |_| set_count.update(|n| *n += 1)`,
  `on:input=move |ev| text.set(event_target_value(&ev))`.
- Comments: `// …` lines are fine inside `view!`.
- Keys for the whole window (games), with `use leptos::ev;`:
  ```rust
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
  ```
- One handler used in several places (two events, many rows): a closure that
  captures a `String` can be used only once (`expected a closure that implements
  the Fn trait, but this closure only implements FnOnce`). Make it a `Callback`
  (it is `Copy`) and call it with `.run(…)` (not `.call`; several arguments go in
  one tuple):
  ```rust
  let pick = Callback::new(move |(path, index): (String, usize)| {
      selected.set(Some((path, index)));
  });
  let path = "a.jpg".to_string();
  let path2 = path.clone();
  view! {
      <input type="checkbox" on:change=move |_| pick.run((path.clone(), 0)) />
      <span on:click=move |_| pick.run((path2.clone(), 0))>"a.jpg"</span>
  }
  ```
- Events that need a value (which arrow, which item): a small function that
  returns the handler, then call it per element:
  ```rust
  let press = move |dir: Dir| move |_| turn.set(dir);
  view! {
      <button data-ak-id="up-button" on:click=press(Dir::Up)>"▲"</button>
      <button data-ak-id="down-button" on:click=press(Dir::Down)>"▼"</button>
  }
  ```
  (`Dir` must be `Copy`: `#[derive(Clone, Copy)]`.)
- The event's type: leave it out, `move |e| …`, and Leptos picks the right one.
  When you write it (`use leptos::ev;` first, the prelude does not bring it), it
  must match the event, or you get `type mismatch in closure arguments: expected
  closure signature fn(PointerEvent), found fn(MouseEvent)` plus an `IntoView is
  not satisfied` wall:

  | Events | Type |
  |---|---|
  | `click`, `dblclick`, `mousedown`, `mouseup`, `mousemove`, `contextmenu` | `ev::MouseEvent` |
  | `pointerdown`, `pointerup`, `pointermove`, `pointerover`, `pointerleave` | `ev::PointerEvent` |
  | `keydown`, `keyup` | `ev::KeyboardEvent` |
  | `wheel` | `ev::WheelEvent` |
  | `input`, `change`, `submit` | `ev::Event`; `submit`: `ev::SubmitEvent` (or leave the type out) |
- Properties (current input value): `prop:value=move || text.get()`.
- Reaching the element itself (focus it, read its HTML, a canvas): a `NodeRef`
  with the element type, `node_ref=` on the element, `.get()` (or
  `.get_untracked()` in handlers) gives it once it is on the page:
  ```rust
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
  ```
  Write the type: with `NodeRef::new()` alone, calling a method on what `.get()`
  returns fails with "type annotations needed".
- A plain closure is called like a function, `save(x)`. Only a `Callback` has
  `.run(x)`; `.run` on a closure gives `expected Option<()>, found ()` pointing
  into Leptos's `effect_function.rs`.
- Two-way binding of inputs: `bind:value=text` (an `RwSignal<String>`),
  `bind:checked=done` (an `RwSignal<bool>`).
- Several root elements are fine: `view! { <h1>"A"</h1> <p>"B"</p> }`.

## Children
```rust
#[component]
fn Panel(#[prop(into)] title: String, children: Children) -> impl IntoView {
    view! {
        <section data-ak-id="panel" class="rounded-xl border p-4">
            <h2 class="font-semibold">{title}</h2>
            {children()}
        </section>
    }
}
// <Panel title="Settings"> <p>"Inside"</p> </Panel>
```
Use `ChildrenFn` (call `children()` many times) when the children sit inside `Show`
or are re-rendered.

## Showing things conditionally
```rust
view! {
    <Show when=move || items.with(|v| v.is_empty()) fallback=|| view! { <ItemList /> }>
        <p data-ak-id="empty-hint">"Nothing here yet"</p>
    </Show>
}
```
In a closure that returns different views, call `.into_any()` on each branch:
```rust
move || if logged_in.get() {
    view! { <Dashboard /> }.into_any()
} else {
    view! { <Login /> }.into_any()
}
```

## Menus and pop-ups (open, close, close on a click outside)
Small layers that open and close often (a drop-down menu, a tooltip, a colour
picker) stay on the page and are hidden with `class:hidden`; do not put them in
`Show`. Built again on every opening, a menu whose item closes it can fail at run
time with `closure invoked recursively or after being dropped`.
```rust
#[component]
fn ViewMenu() -> impl IntoView {
    let open = RwSignal::new(false);
    // A press anywhere else closes it, and so does Esc.
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
        // Presses inside the menu do not reach the window's listener above.
        <div class="relative" on:pointerdown=|e| e.stop_propagation()>
            // Open and close on click, not on pointerdown.
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
```

## Lists
```rust
<For
    each=move || todos.get()
    key=|t| t.id            // a stable id, never the position
    children=move |t: Todo| view! { <li data-ak-id=format!("todo-{}", t.id)>{t.text}</li> }
/>
```
ak-ui's `List` does this for you, with ids for each row (topic `ak-ui`).
- `each` is always a closure: `each=rows` fails with `expected an Fn() closure,
  found Vec<…>`; write `each=move || rows.get()` (or `move || rows.clone()`).
- `key` must be `Eq + Hash`: give it a number or a `String` (`key=|r| r.id`,
  `key=|(name, _)| name.clone()`). A struct of your own in the key fails with
  `the trait bound …: Eq / Hash is not satisfied`.

**`Show` (and `Suspense`, `ChildrenFn`) runs its children again every time it
shows them**, so they may not move anything out: a row's `String` used in a
handler inside `Show` fails with `FnOnce` errors that point at the whole `view!`.
`Rc`, `Arc` and `AnyView` do not help. Turn the row's values into a `StoredValue`
(it is `Copy`) at the top of `children`, and read it with `.get_value()`:
```rust
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
```
Or skip `Show`: `{move || starred.then(|| view! { … })}` builds the part afresh
each time (clone what it needs inside the closure).

## Mounting
`src/main.rs` already does it; keep `ak_bridge::start()` first:
```rust
fn main() {
    ak_bridge::start();
    leptos::mount::mount_to_body(App);
}
```

## Several screens
Keep the current screen in a signal and switch with `Nav` (topic `ak-ui`) — no router
needed for small apps:
```rust
let screen = RwSignal::new("list".to_string());
view! {
    <Nav ak_id="main-nav" items=vec![NavItem::new("list", "List"), NavItem::new("stats", "Stats")] active=screen />
    {move || match screen.get().as_str() {
        "stats" => view! { <Stats /> }.into_any(),
        _ => view! { <ListScreen /> }.into_any(),
    }}
}
```
