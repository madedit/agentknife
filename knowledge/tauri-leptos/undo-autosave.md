# Undo, redo and saving by itself

Editors (drawings, boards, documents, lists the user arranges) need Ctrl+Z. Plan it
from the start: every change must go through one place, or some changes cannot be
undone.

## Undo and redo
Keep a copy of the data before each change. The data is small (a list, a drawing),
so copies are cheap; keep the last 100.
```rust
/// Copies of the data before each change (past) and after each undo (future).
struct History<T> {
    past: Vec<T>,
    future: Vec<T>,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self { past: Vec::new(), future: Vec::new() }
    }
}

impl<T> History<T> {
    const LIMIT: usize = 100;

    /// Call with the data as it was *before* a change.
    fn record(&mut self, before: T) {
        self.past.push(before);
        if self.past.len() > Self::LIMIT {
            self.past.remove(0);
        }
        self.future.clear(); // a new change ends the redo steps
    }
    fn undo(&mut self, now: T) -> Option<T> {
        let before = self.past.pop()?;
        self.future.push(now);
        Some(before)
    }
    fn redo(&mut self, now: T) -> Option<T> {
        let after = self.future.pop()?;
        self.past.push(now);
        Some(after)
    }
}

#[derive(Clone, PartialEq)]
struct Shape {
    id: u32,
    x: f64,
    y: f64,
    label: String,
}

/// The document and its history. Only signals and stored values inside, so it is
/// `Copy`: create it once in the component and use it in every handler.
#[derive(Clone, Copy)]
struct Doc {
    shapes: RwSignal<Vec<Shape>>,
    history: StoredValue<History<Vec<Shape>>>,
    /// (undo steps, redo steps), for the buttons.
    steps: RwSignal<(usize, usize)>,
    /// True while the user types in one field: the whole edit is one undo step.
    typing: StoredValue<bool>,
}

impl Doc {
    fn new(shapes: Vec<Shape>) -> Self {
        Self {
            shapes: RwSignal::new(shapes),
            history: StoredValue::new(History::default()),
            steps: RwSignal::new((0, 0)),
            typing: StoredValue::new(false),
        }
    }

    /// Every change goes through here, so each one can be undone.
    fn change(self, f: impl FnOnce(&mut Vec<Shape>)) {
        let before = self.shapes.get_untracked();
        self.shapes.update(f);
        self.history.update_value(|h| h.record(before));
        self.typing.set_value(false);
        self.changed();
    }

    /// For `on:input`: the first key records, the next ones do not.
    fn type_text(self, f: impl FnOnce(&mut Vec<Shape>)) {
        if !self.typing.get_value() {
            let before = self.shapes.get_untracked();
            self.history.update_value(|h| h.record(before));
            self.typing.set_value(true);
        }
        self.shapes.update(f);
        self.changed();
    }

    /// For a drag: call once when the drag really starts (the first move, not
    /// `pointerdown`: a click that only selects changes nothing), then move with
    /// `shapes.update` until `pointerup`. The whole drag is one step.
    fn checkpoint(self) {
        let now = self.shapes.get_untracked();
        self.history.update_value(|h| h.record(now));
        self.changed();
    }

    /// For the field's `on:blur`: the next typing is a new step.
    fn stop_typing(self) {
        self.typing.set_value(false);
    }

    fn undo(self) {
        let now = self.shapes.get_untracked();
        if let Some(before) = self.history.try_update_value(|h| h.undo(now)).flatten() {
            self.shapes.set(before);
            self.changed();
        }
    }

    fn redo(self) {
        let now = self.shapes.get_untracked();
        if let Some(after) = self.history.try_update_value(|h| h.redo(now)).flatten() {
            self.shapes.set(after);
            self.changed();
        }
    }

    fn changed(self) {
        let steps = self.history.with_value(|h| (h.past.len(), h.future.len()));
        self.steps.set(steps);
        // and save (below)
    }
}
```
In the component:
```rust
let doc = Doc::new(Vec::new());
// Ctrl+Z, Ctrl+Y and Ctrl+Shift+Z anywhere in the window. In a text field they
// are the field's own undo (is_typing, topic `canvas`).
let keys = window_event_listener(leptos::ev::keydown, move |e| {
    if !(e.ctrl_key() || e.meta_key()) || is_typing() {
        return;
    }
    match e.key().to_lowercase().as_str() {
        "z" if e.shift_key() => doc.redo(),
        "z" => doc.undo(),
        "y" => doc.redo(),
        _ => return,
    }
    e.prevent_default();
});
on_cleanup(move || keys.remove());

let add = move |_| doc.change(|s| s.push(Shape { id: 1, x: 0.0, y: 0.0, label: "Box".into() }));
let rename = move |id: u32, text: String| {
    doc.type_text(move |s| {
        if let Some(shape) = s.iter_mut().find(|s| s.id == id) {
            shape.label = text;
        }
    })
};
// <button on:click=add>"Add box"</button>
// <input on:input=move |e| rename(id, event_target_value(&e)) on:blur=move |_| doc.stop_typing() />
// Buttons: disabled=move || doc.steps.get().0 == 0 (undo), .1 == 0 (redo)
```
- Selection, zoom and scroll position are not part of the document: keep them in
  their own signals, outside `Doc`.
- Check with `interact`: make a change, press `key` `Control+z` (then
  `Control+y`), and compare `read_state` before and after.

## Saving by itself
Save a short moment after the last change, not on every key: a timer that each
change restarts.
```rust
/// Saves `save()` 600 ms after the last call; `dirty` is true until then.
#[derive(Clone, Copy)]
struct AutoSave {
    timer: StoredValue<Option<TimeoutHandle>>,
    dirty: RwSignal<bool>,
}

impl AutoSave {
    fn new() -> Self {
        Self { timer: StoredValue::new(None), dirty: RwSignal::new(false) }
    }

    fn later(self, save: impl Fn() + 'static) {
        if let Some(t) = self.timer.get_value() {
            t.clear();
        }
        self.dirty.set(true);
        let dirty = self.dirty;
        let t = set_timeout_with_handle(
            move || {
                save();
                dirty.set(false);
            },
            std::time::Duration::from_millis(600),
        );
        self.timer.set_value(t.ok());
    }
}
```
Call `autosave.later(save)` from `Doc::changed` (give `Doc` the `AutoSave` as a
field) and show `dirty` as "Saving…" / "Saved". A click (add, delete, move) can
also save at once; typing waits for the pause. The save writes a temporary file
and renames it (topic `storage`), so closing the app mid-save never breaks the
file.

## Drafts
Text the user has not confirmed yet (a long note in a dialog, a form) is lost if
the app closes. Keep it in `localStorage` while they type, and delete it on save:
```rust
fn save_draft(text: &str) {
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.set_item("draft-note", text);
    }
}

fn take_draft() -> Option<String> {
    let s = web_sys::window()?.local_storage().ok()??;
    let text = s.get_item("draft-note").ok()??;
    let _ = s.remove_item("draft-note");
    (!text.is_empty()).then_some(text)
}
```
When the dialog opens, fill it from `take_draft()` and tell the user ("Your unsaved
text was restored"). Use one key per kind of draft.
