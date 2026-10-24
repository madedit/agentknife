# Drag and drop (cards between columns, sorting a list)

## Turn off Tauri's file-drop handler first (Windows)
On Windows, Tauri takes over drag and drop for files dropped from the computer,
and **HTML5 drag and drop inside the page then does not work at all** (in the
installed app). New projects have it off; in an older project check
`src-tauri/tauri.conf.json` and add `"dragDropEnabled": false` to the window:
```json
"windows": [{ "label": "main", "title": "My App", "dragDropEnabled": false }]
```
With it off, files dropped onto the page come as `e.data_transfer()` → `files()`
(their contents, not their paths; web-sys feature `FileList`). For the paths,
leave it on and listen to Tauri's drag-drop event in the backend instead; then
in-page dragging does not work on Windows, so pick one.

## web-sys features
Leptos gives the `DragEvent`, but its `data_transfer()` needs the features. In the
screens' `Cargo.toml`:
```toml
web-sys = { version = "0.3.106", features = ["DragEvent", "DataTransfer"] }
```
Without them: `no method named data_transfer found for DragEvent`.

## A board: cards dragged between columns
```rust
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
```
- `draggable="true"` on what is dragged; `dragstart` remembers which one.
- Every place that takes drops: `on:dragover` with `prevent_default()` (without it
  `drop` never comes), and `on:drop` does the move.
- Move only in `drop`. `dragend` also comes after a drag that was let go
  elsewhere (or a click that started a tiny drag): there it only clears.
- Order inside a column: put the drop handler on each card too, and insert before
  that card (its position from the list, then save).
- Keep a way without dragging (buttons, or keys): for keyboard users, and it is
  easy to check.

## Checking it in the preview
`interact` with `action: "drag"`, `ak_id` of the card and `to` of the column. It
sends the drag events and answers `"dropped": true` when the column took it
(`false`: no `prevent_default` on `dragover`). Then check with `dom_summary` that
the card is in the new column. Also try the buttons. Files dropped from the
computer can only be tried by the user.
