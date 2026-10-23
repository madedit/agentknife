# ak-ui components

`use ak_ui::*;` Every component takes `ak_id` (rendered as `data-ak-id`); parts inside
get derived ids. Components have no built-in text: pass labels in the user's language.

Event props (`on_click`, `on_submit`, `on_change`) take a closure that returns
nothing. One that ends in a value (`move |_| items.try_update(…)`, which returns
an `Option`) fails with `IntoReactiveValue<Callback<MouseEvent>, _> is not
satisfied`, pointing at the prop: end it with `;` inside braces,
`move |_| { items.try_update(…); }`.

What the props take ("follows" = a closure or signal keeps it up to date):

| Component | Prop | Takes |
|---|---|---|
| all | `ak_id` | text, fixed |
| Button | `variant`, `submit`, `label` | fixed values |
| Button, Input, Toggle | `disabled` | follows (`Signal::derive(move \|\| …)`) |
| Input | `value` | `RwSignal<String>` only: keep numbers as text, `parse()` when used |
| Input | `label`, `placeholder` | text, or follows: `label=move \|\| …` |
| Input | `kind` | fixed text |
| Card | `title` | text, or follows |
| Toggle | `checked` | `RwSignal<bool>` |
| Toggle | `label` | text, or follows |
| Dialog | `open` | `RwSignal<bool>` (it closes itself) |
| Dialog | `title` | text, or follows |
| Dialog | `close_label` | fixed text |
| Nav | `items` | fixed list; `active`: `RwSignal<String>` |
| List | `items` | follows (`RwSignal<Vec<T>>` or `Signal`) |

## Button
```rust
<Button ak_id="save-button" on_click=move |_| save()>"Save"</Button>
<Button ak_id="delete-button" variant=ButtonVariant::Danger disabled=Signal::derive(move || busy.get())>"Delete"</Button>
<Button ak_id="add-button" submit=true>"Add"</Button>            // submits its Form
<Button ak_id="close-button" variant=ButtonVariant::Ghost label="Close">"✕"</Button>  // icon: give a label
```
One button that does different things by state (Start / Pause / Resume): decide
inside the handler and in the label, reading signals:
```rust
<Button ak_id="play-button" on_click=move |_| match phase.get_untracked() {
    Phase::Ready | Phase::Paused => start(),
    Phase::Playing => pause(),
}>
    {move || match phase.get() { Phase::Playing => "Pause", Phase::Paused => "Resume", Phase::Ready => "Start" }}
</Button>
```
`on_click` takes a closure `move |_| …` (the event is an `ev::MouseEvent`); not
`move |()|`, which is only for `Form`'s `on_submit`.

`Button` has no `class` prop: its looks come from `variant`. For a control that
must look different (big see-through arrow keys over a game), use a plain element
with Tailwind classes and give it its own `data-ak-id`:
```rust
<button type="button" data-ak-id="left-key" aria-label="Left"
    class="h-16 w-16 rounded-full bg-white/30 text-2xl text-white active:bg-white/50"
    on:click=move |_| turn(Dir::Left)>"◀"</button>
```
Variants: `Primary` (default, the main action), `Secondary`, `Danger` (cannot be
undone), `Ghost` (low emphasis).

## Input
```rust
let name = RwSignal::new(String::new());
<Input ak_id="name-input" value=name label="Name" placeholder="Ada" />
<Input ak_id="email-input" value=email kind="email" label="Email" />
```
Two-way bound to the `RwSignal<String>`. `kind`: text, email, number, password,
search, date. `disabled` takes a `Signal<bool>`.

## Form
```rust
<Form ak_id="add-form" on_submit=move |()| add()>
    <Input ak_id="item-input" value=text label="New item" />
    <Button ak_id="add-button" submit=true>"Add"</Button>
</Form>
```
Enter or the submit button calls `on_submit`; the page never reloads.
`Form` lays its children out itself: a column, side by side from the `sm` width
(`sm:flex-row sm:items-end`), made for one field and a button. For several fields,
put them all in one `<div class="grid w-full gap-3 sm:grid-cols-2">` inside it.

## List
```rust
<List
    ak_id="todo-list"
    items=todos                      // RwSignal<Vec<T>> or Signal<Vec<T>>
    key=|t: &Todo| t.id              // stable id → row ids "todo-list-<id>"
    row=move |t: Todo| view! { <span class="flex-1">{t.text}</span> }
    empty="Nothing to do yet"
/>
```
Rows are flex rows: put buttons at the end of `row`.

The `key` must be unique and stable, and printable (`Display`: numbers, `String`).
It becomes part of each row's `data-ak-id`: spaces and signs turn into `-`
(`"New York"` → `city-list-New-York`, `-6` → `city-list-minus-6`). The ids of
parts inside a row are yours to set: build them the same way,
`ak_ui::part_id("choose", c.id)`.

`row` runs once per key, when the item first appears: if an item is edited later
(same id, new amount), the row still shows the old values. For rows whose content
can change, read the latest values from the signal inside the row:
```rust
row=move |e: Entry| {
    let id = e.id;
    // The row is built once; this follows edits to the entry.
    let latest = Memo::new(move |_| entries.with(|v| v.iter().find(|x| x.id == id).cloned()));
    view! {
        <span class="flex-1">{move || latest.get().map(|x| x.title).unwrap_or_default()}</span>
        <span>{move || latest.get().map(|x| format!("{:.2}", x.amount)).unwrap_or_default()}</span>
    }
}
```

## Dialog
```rust
let open = RwSignal::new(false);
<Button ak_id="clear-button" variant=ButtonVariant::Danger on_click=move |_| open.set(true)>"Clear"</Button>
<Dialog ak_id="clear-dialog" open=open title="Clear everything?" close_label="Close">
    <p>"This cannot be undone."</p>
    <div class="mt-4 flex justify-end gap-2">
        <Button ak_id="clear-cancel" variant=ButtonVariant::Ghost on_click=move |_| open.set(false)>"Cancel"</Button>
        <Button ak_id="clear-confirm" variant=ButtonVariant::Danger on_click=move |_| { clear(); open.set(false); }>"Clear"</Button>
    </div>
</Dialog>
```
Escape, the × and a click outside close it. Ids: `clear-dialog-close`,
`clear-dialog-backdrop`.

## Nav (tabs)
```rust
let tab = RwSignal::new("all".to_string());
<Nav ak_id="filter-tabs" items=vec![NavItem::new("all", "All"), NavItem::new("done", "Done")] active=tab />
```
Item ids: `filter-tabs-all`, `filter-tabs-done`. `items` is fixed when the screen
is built: use `Nav` for fixed tabs only. Filters that change (this month, a date)
are plain buttons with fixed ids (`month-today`, `month-prev`); never put a date
or other changing value into an id.

## Card
```rust
<Card ak_id="settings-card" title="Settings"> …children… </Card>
```

## Toggle
```rust
let dark = RwSignal::new(false);
<Toggle ak_id="dark-toggle" checked=dark label="Dark mode" />
```

## Toast
Wrap the app once (the template already does), then show messages anywhere inside:
```rust
view! { <ToastHost ak_id="toasts"> <App /> </ToastHost> }

let toaster = use_toaster();   // call in the component body, not in a handler
toaster.success("Saved");
toaster.error("Could not save");
toaster.info("3 items");
```
Toasts disappear after 4 seconds (`duration_ms` on `ToastHost` to change).
`Toaster` is `Copy`: keep it in a struct or use it in many closures.
