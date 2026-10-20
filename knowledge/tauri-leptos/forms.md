# Inputs and forms

Prefer ak-ui's `Input`, `Form`, `Toggle` and `Button` (topic `ak-ui`). For anything
else, bind elements to signals yourself.

## Text
```rust
let title = RwSignal::new(String::new());
view! {
    <input data-ak-id="title-input" type="text" bind:value=title
        class="rounded-lg border px-3 py-2" />
    <p>"You typed: " {move || title.get()}</p>
}
```
Multi-line: `<textarea data-ak-id="notes" bind:value=notes rows="4"></textarea>`.

## Numbers
Inputs give text; parse it:
```rust
let amount = RwSignal::new(String::new());
let parsed = move || amount.with(|a| a.trim().parse::<f64>().ok());
view! {
    <input data-ak-id="amount-input" type="number" step="0.01" bind:value=amount />
    <Show when=move || parsed().is_none() && !amount.with(String::is_empty)>
        <p data-ak-id="amount-error" class="text-sm text-red-600">"Please type a number"</p>
    </Show>
}
```

## Checkboxes and selects
```rust
let done = RwSignal::new(false);
let color = RwSignal::new("blue".to_string());
view! {
    <label class="flex items-center gap-2">
        <input data-ak-id="done-checkbox" type="checkbox" bind:checked=done />
        "Done"
    </label>
    <select data-ak-id="color-select" bind:value=color class="rounded-lg border px-3 py-2">
        <option value="blue">"Blue"</option>
        <option value="green">"Green"</option>
    </select>
}
```
For on/off settings, ak-ui's `Toggle` looks better than a checkbox.

## Submitting
A `<form>` submits on Enter and on a `type="submit"` button. Always prevent the page
reload (ak-ui's `Form` does it):
```rust
view! {
    <form data-ak-id="add-form" on:submit=move |ev| {
        ev.prevent_default();
        add();
    }>
        <input data-ak-id="add-input" bind:value=text />
        <button data-ak-id="add-button" type="submit">"Add"</button>
    </form>
}
```

## Validation
- Check on submit: trim text, refuse empty values, show a short message near the field
  (or a toast: `toaster.error("…")`, with `let toaster = use_toaster();` in the
  component body).
- Clear the field after a successful add: `text.set(String::new())`.
- Disable the button while saving: `disabled=move || busy.get()`.
