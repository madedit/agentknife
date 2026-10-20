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

## The view! macro
- Text in quotes: `"Hello"`. Rust values in braces: `{name}`.
- **Reactive** values are closures: `{move || count.get()}`. A plain `{count.get()}`
  is read once and never updates.
- Attributes: `class="…"`, `data-ak-id="…"`, `disabled=move || busy.get()`,
  `placeholder=hint` (a variable).
- Class toggles: `class:hidden=move || !open.get()`, or a closure returning the whole
  class string.
- Events: `on:click=move |_| set_count.update(|n| *n += 1)`,
  `on:input=move |ev| text.set(event_target_value(&ev))`.
- Properties (current input value): `prop:value=move || text.get()`.
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

## Lists
```rust
<For
    each=move || todos.get()
    key=|t| t.id            // a stable id, never the position
    children=move |t: Todo| view! { <li data-ak-id=format!("todo-{}", t.id)>{t.text}</li> }
/>
```
ak-ui's `List` does this for you, with ids for each row (topic `ak-ui`).

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
