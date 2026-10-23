# Text editors (bold, colours, find and replace)

A plain `<textarea>` (topic `forms`) is enough for plain text. For formatted
text, an element with `contenteditable="true"` lets the user type, and the
browser's editing commands format the selection. web-sys features (topic
`browser`): `Window`, `Document`, `HtmlDocument`, `HtmlDivElement`.

```rust
use wasm_bindgen::JsCast;

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
```
- `exec_command(cmd)`, or `exec_command_with_show_ui_and_value(cmd, false,
  value)` for commands with a value (a colour, a size). They need the editor
  focused with the text selected.
- Toolbar buttons take the focus away when pressed: then the command and
  `query_command_state` do nothing. `prevent_default()` on `mousedown` (above)
  keeps the focus and the selection; otherwise `focus()` the editor first.
- Save and load the content as HTML: `div.inner_html()` and
  `div.set_inner_html(&saved)` (in `NodeRef::on_load`, or after loading).
- The selection disappears when the focus moves elsewhere (a find-and-replace
  dialog's text field): do not rely on `get_selection()` then. Keep which match
  is current (its number) in a signal and work on the text.

## Finding text
Byte positions from one string do not fit another: `to_lowercase()` may change
the length, and slicing Chinese text off a character's edge panics (`byte index
27 is not a char boundary` / `is out of bounds`). Search the original text:
```rust
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
```
