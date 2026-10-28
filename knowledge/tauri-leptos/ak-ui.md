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
| Button | `variant`, `submit`, `label`, `small` | fixed values |
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
| Dialog | `on_close` | optional closure `move \|_\| …`: runs when it closes itself |
| Nav | `items` | fixed list; `active`: `RwSignal<String>` |
| List | `items` | follows (`RwSignal<Vec<T>>` or `Signal`) |
| all | `class` | fixed text: more classes |

**`class` adds classes after the component's own**: layout such as `w-full`,
`mt-4`, `flex-1`, `max-w-sm`. Two classes for the same thing (the component's
`bg-white` and your `bg-slate-800`) do not reliably pick yours: end yours with
`!` to make it win (`class="bg-slate-800! text-white!"`). On `Input` it goes on
the field itself; on `Dialog` on its panel (`class="max-w-2xl!"` for a wider
one). For a whole other look (a big coloured button, a game's keys) use a plain
element with Tailwind classes and its own `data-ak-id` (see Button below).

Where `class` lands, and the look you may want to change (light / dark):

| Component | `class` goes on | Its look | Not reached by `class` |
|---|---|---|---|
| Button | the `<button>` | Primary `bg-blue-600 text-white`; Secondary `bg-white ring-gray-300` / `dark:bg-gray-800`; Danger `bg-red-600`; Ghost `text-gray-700 hover:bg-gray-100` | — |
| Input | the `<input>` | `bg-white border-gray-300` / `dark:bg-gray-800 dark:border-gray-600` | its label (`text-gray-700` / `dark:text-gray-300`) |
| Form | the `<form>` | `flex flex-col gap-3 sm:flex-row` | — |
| Card | the `<section>` | `rounded-xl border-gray-200 bg-white p-4` / `dark:bg-gray-900 dark:border-gray-700` | the title (`text-gray-900` / `dark:text-gray-100`) |
| List | the `<ul>` | `rounded-xl border-gray-200 bg-white divide-y` / `dark:bg-gray-900` | the rows (`px-4 py-3 text-gray-900`) |
| Toggle | the outer `<label>` | on `bg-blue-600`, off `bg-gray-300` / `dark:bg-gray-600` | the switch itself |
| Nav | the `<nav>` | chosen tab `bg-blue-600 text-white`, others `text-gray-700 hover:bg-gray-100` | the tabs |
| Dialog | the panel | `max-w-md rounded-2xl bg-white p-6` / `dark:bg-gray-900` | the backdrop (`bg-black/40`) |
| ToastHost | (no `class`) | dark toasts, green for success, red for errors | — |

What `class` does not reach, and changes to every component of a kind, go in
the stylesheet ("Restyling every one at once" below). A dark app is the `dark`
class on `<html>` (topic `tailwind`, "Dark mode"), not `class="bg-…!"` on every
component.

## Restyling every one at once
"Make the buttons rounder", "no shadows", "teal tabs": every part of every
component carries a fixed marker class. A rule for it in `style/input.css`
(after `@import "tailwindcss";`, outside any `@layer`) changes all of them, and
wins over the component's own classes:
```css
/* Every button pill-shaped, without a shadow. */
.ak-button { @apply rounded-full shadow-none; }
/* The main buttons and the chosen tab in teal. */
.ak-button-primary { @apply bg-teal-600 hover:bg-teal-700; }
.ak-nav-tab[aria-current="page"] { @apply bg-teal-600; }
/* Cards and lists on a dark page. */
.ak-card, .ak-list { @apply border-slate-700 bg-slate-900 text-slate-100; }
```
| Component | Marker classes |
|---|---|
| Button | `ak-button`, and `ak-button-primary` / `-secondary` / `-danger` / `-ghost` |
| Input | `ak-input` (the field), `ak-input-label` |
| Form | `ak-form` |
| Card | `ak-card`, `ak-card-title` |
| List | `ak-list`, `ak-list-row`, `ak-list-empty` |
| Toggle | `ak-toggle` (the label), `ak-toggle-switch` (`[aria-checked="true"]` when on), `ak-toggle-knob` |
| Nav | `ak-nav`, `ak-nav-tab` (`[aria-current="page"]` for the chosen one) |
| Dialog | `ak-dialog` (the panel), `ak-dialog-backdrop`, `ak-dialog-title`, `ak-dialog-close` |
| ToastHost | `ak-toasts`, `ak-toast` and `ak-toast-info` / `-success` / `-error` |

This is the way to change how every ak-ui component looks: never `ak/`
(AgentKnife updates it, and its tools refuse changes there, commands
included). One component only: its `class` prop. The markers are in ak-ui
since pack 0.2.27; an older project's ak-ui has no `ak-…` classes (search
`ak/ak-ui/src` for `ak-button`): there, change colours with `@theme` or use
your own elements.

**Do not edit `ak/`** (ak-ui, ak-bridge), not with a command or a script either:
AgentKnife updates it, and its tools refuse changes there. Restyle with
`class` or the marker classes below; when a component does not fit, write your
own in `src/`.
`class` is in ak-ui since pack 0.2.26: in an older project (`class` missing in
`ak/ak-ui/src/button.rs`), `class=` on a component does not build; use a plain
element there.

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
`on_click` takes a closure `move |_| …`: leave the event's type out (it is a
`leptos::ev::MouseEvent`; `ev::` alone needs `use leptos::ev;`). Not `move |()|`,
which is only for `Form`'s `on_submit`.

`small=true` gives a smaller button (toolbars, a card's own buttons):
`<Button ak_id="card-edit" variant=ButtonVariant::Ghost small=true>"Edit"</Button>`.
`class` adds to its look: `<Button ak_id="start" class="w-full">"Start"</Button>`.

A control that must look quite different (big see-through arrow keys over a
game) is a plain element with Tailwind classes and its own `data-ak-id`:
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
Rows are flex rows: put buttons at the end of `row`. `List` looks like a light
card (white, rounded, a border). On a dark panel, `class="bg-slate-900!
border-slate-700! divide-slate-700!"` changes the card; rows that must look
quite different are drawn yourself (`<ul>` with `<For>` and a row component,
topic `leptos-components`).

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

`open` is the one truth about whether it shows. A dialog that edits something
keeps what it edits in a second signal; clear that in `on_close`, which runs when
the dialog closes itself (Escape, ×, backdrop), so nothing is left behind:
```rust
let editing = RwSignal::new(None::<u32>);
let open = RwSignal::new(false);
let edit = move |id: u32| {
    editing.set(Some(id));
    open.set(true);
};
<Dialog ak_id="edit-dialog" open=open title="Edit card" close_label="Close"
    on_close=move |_| editing.set(None)>
    <p>{move || editing.get().map(|id| format!("Card {id}"))}</p>
</Dialog>
```
Your own buttons that close it (Save, Cancel) set `open` to false and clear
`editing` themselves. (`small` and `on_close` are in ak-ui since pack 0.2.12; an
older project's `ak/ak-ui/src` may not have them: use an `Effect` that clears
`editing` when `open` turns false.)

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
<Card ak_id="score-card" class="bg-slate-800! border-slate-700! text-white"> …children… </Card>
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

let toaster = use_toaster();   // in a component's body, inside ToastHost
toaster.success("Saved");
toaster.error("Could not save");
toaster.info("3 items");
```
**Call `use_toaster()` only in the body of a component inside `ToastHost`.** In a
handler, or in a plain function of another module (`store.rs`), it finds no
toaster and the page panics at run time (it still builds). To show messages from
there, get the `Toaster` in the component and pass it on as an argument
(`Store::new(data, use_toaster())`).

Toasts disappear after 4 seconds (`duration_ms` on `ToastHost` to change).
`Toaster` is `Copy`: keep it in a struct or use it in many closures.
