# ak-ui components

`use ak_ui::*;` Every component takes `ak_id` (rendered as `data-ak-id`); parts inside
get derived ids. Components have no built-in text: pass labels in the user's language.

## Button
```rust
<Button ak_id="save-button" on_click=move |_| save()>"Save"</Button>
<Button ak_id="delete-button" variant=ButtonVariant::Danger disabled=Signal::derive(move || busy.get())>"Delete"</Button>
<Button ak_id="add-button" submit=true>"Add"</Button>            // submits its Form
<Button ak_id="close-button" variant=ButtonVariant::Ghost label="Close">"✕"</Button>  // icon: give a label
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
Item ids: `filter-tabs-all`, `filter-tabs-done`.

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
