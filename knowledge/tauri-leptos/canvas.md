# Canvas editors (flow charts, diagrams, maps): pan, zoom, drag, lines

Things that are moved around freely on a stage (boxes and the lines between
them) are made of plain elements and SVG, moved with pointer events. (HTML5 drag
and drop, topic `drag-drop`, is for moving items between lists.)

## The parts
- **One stage** (`overflow-hidden`, `touch-none`, `select-none`) takes all the
  pointer events: `pointerdown` finds what was pressed, `pointermove` moves it,
  `pointerup` ends.
- **Pointer capture** on the stage at `pointerdown`: without it, a drag that
  leaves the window loses `pointermove`/`pointerup` and the box stays stuck to
  the mouse or halfway.
- **One transform** on an inner layer, `translate(x, y) scale(zoom)`, pans and
  zooms everything; positions inside are in drawing units.
- **Screen → drawing**: `(client - stage's corner - pan) / zoom`, the corner
  from `get_bounding_client_rect()`.
- **Wheel zoom** needs `prevent_default()`, which only works in a listener added
  with `passive: false`: add it yourself, once, through a `NodeRef` (below).
- **Lines in SVG**: the whole `<svg>` layer is `pointer-events-none` (it would
  block pressing the stage), and each line is drawn twice: a wide transparent
  path that takes clicks (`pointer-events-auto`), and the thin visible one.

In the screens' `Cargo.toml`:
```toml
wasm-bindgen = "0.2.129"
web-sys = { version = "0.3.106", features = ["Element", "HtmlElement", "DomRect", "EventTarget", "AddEventListenerOptions", "WheelEvent", "PointerEvent"] }
```

## A stage with boxes, a line, pan and zoom
```rust
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

#[derive(Clone, Copy, PartialEq)]
struct Camera {
    x: f64,
    y: f64,
    zoom: f64,
}

#[derive(Clone, PartialEq)]
struct Node {
    id: u32,
    x: f64,
    y: f64,
    label: String,
}

#[component]
fn Stage() -> impl IntoView {
    let camera = RwSignal::new(Camera { x: 0.0, y: 0.0, zoom: 1.0 });
    let nodes = RwSignal::new(vec![
        Node { id: 1, x: 40.0, y: 40.0, label: "Start".into() },
        Node { id: 2, x: 300.0, y: 140.0, label: "End".into() },
    ]);
    let links = RwSignal::new(vec![(1u32, 2u32)]);
    let selected = RwSignal::new(None::<(u32, u32)>);
    // While dragging: (node id, pointer's offset from the node's corner).
    let dragging = StoredValue::new(None::<(u32, f64, f64)>);
    let stage = NodeRef::<leptos::html::Div>::new();

    // A point on the screen → the same point on the drawing.
    let to_drawing = move |cx: i32, cy: i32| {
        let (left, top) = stage
            .get_untracked()
            .map(|s| s.get_bounding_client_rect())
            .map_or((0.0, 0.0), |r| (r.left(), r.top()));
        let c = camera.get_untracked();
        ((f64::from(cx) - left - c.x) / c.zoom, (f64::from(cy) - top - c.y) / c.zoom)
    };

    // Wheel zoom: a listener that may call prevent_default (passive: false).
    // This effect runs once, when the stage is on the page: one listener.
    Effect::new(move |_| {
        let Some(el) = stage.get() else { return };
        let on_wheel = Closure::<dyn Fn(web_sys::WheelEvent)>::new(move |e: web_sys::WheelEvent| {
            // Otherwise the page scrolls (or the whole view zooms) as well.
            e.prevent_default();
            let step = if e.delta_y() < 0.0 { 1.1 } else { 1.0 / 1.1 };
            camera.update(|c| c.zoom = (c.zoom * step).clamp(0.25, 4.0));
        });
        let options = web_sys::AddEventListenerOptions::new();
        options.set_passive(false);
        let _ = el.add_event_listener_with_callback_and_add_event_listener_options(
            "wheel",
            on_wheel.as_ref().unchecked_ref(),
            &options,
        );
        // Kept as long as the page: the stage is made once.
        on_wheel.forget();
    });

    let on_down = move |e: leptos::ev::PointerEvent| {
        // The node under the pointer (the press may be on its label).
        let id = e
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .and_then(|t| t.closest("[data-node]").ok().flatten())
            .and_then(|n| n.get_attribute("data-node"))
            .and_then(|s| s.parse::<u32>().ok());
        let Some(id) = id else { return };
        let (px, py) = to_drawing(e.client_x(), e.client_y());
        let corner = nodes.with_untracked(|v| v.iter().find(|n| n.id == id).map(|n| (n.x, n.y)));
        if let Some((x, y)) = corner {
            dragging.set_value(Some((id, px - x, py - y)));
            // Moves and the release keep coming to the stage, even outside the window.
            if let Some(s) = stage.get_untracked() {
                let _ = s.set_pointer_capture(e.pointer_id());
            }
        }
    };
    let on_move = move |e: leptos::ev::PointerEvent| {
        let Some((id, dx, dy)) = dragging.get_value() else { return };
        let (px, py) = to_drawing(e.client_x(), e.client_y());
        nodes.update(|v| {
            if let Some(n) = v.iter_mut().find(|n| n.id == id) {
                n.x = px - dx;
                n.y = py - dy;
            }
        });
    };
    let on_up = move |e: leptos::ev::PointerEvent| {
        if dragging.get_value().is_some() {
            dragging.set_value(None);
            if let Some(s) = stage.get_untracked() {
                let _ = s.release_pointer_capture(e.pointer_id());
            }
            // Save here (topic `storage`).
        }
    };

    view! {
        <div
            node_ref=stage
            data-ak-id="stage"
            class="relative h-96 w-full touch-none select-none overflow-hidden bg-gray-50"
            on:pointerdown=on_down
            on:pointermove=on_move
            on:pointerup=on_up
        >
            // One transform moves and scales everything drawn.
            <div
                class="absolute left-0 top-0 origin-top-left"
                style:transform=move || {
                    let c = camera.get();
                    format!("translate({}px, {}px) scale({})", c.x, c.y, c.zoom)
                }
            >
                // The lines: the layer lets the pointer through, only the hit paths take it.
                <svg class="pointer-events-none absolute left-0 top-0 overflow-visible" width="1" height="1">
                    <For
                        each=move || links.get()
                        key=|l| *l
                        children=move |(a, b): (u32, u32)| {
                            let d = move || nodes.with(|v| {
                                let end = |id: u32| v.iter().find(|n| n.id == id).map_or((0.0, 0.0), |n| (n.x + 64.0, n.y + 20.0));
                                let ((x1, y1), (x2, y2)) = (end(a), end(b));
                                format!("M {x1} {y1} C {} {y1}, {} {y2}, {x2} {y2}", x1 + 80.0, x2 - 80.0)
                            });
                            view! {
                                // Wide and invisible: easy to hit.
                                <path
                                    d=d
                                    data-ak-id=ak_ui::part_id("link", format!("{a}-{b}"))
                                    stroke="transparent"
                                    stroke-width="16"
                                    fill="none"
                                    class="pointer-events-auto cursor-pointer"
                                    on:click=move |_| selected.set(Some((a, b)))
                                />
                                // What is seen.
                                <path
                                    d=d
                                    fill="none"
                                    stroke-width="2"
                                    stroke=move || if selected.get() == Some((a, b)) { "#2563eb" } else { "#64748b" }
                                />
                            }
                        }
                    />
                </svg>
                <For
                    each=move || nodes.get()
                    key=|n| n.id
                    children=move |n: Node| {
                        let id = n.id;
                        let at = move || nodes.with(|v| v.iter().find(|m| m.id == id).map_or((0.0, 0.0), |m| (m.x, m.y)));
                        view! {
                            <div
                                data-node=id.to_string()
                                data-ak-id=ak_ui::part_id("node", id)
                                class="absolute w-32 cursor-move rounded-lg border bg-white px-3 py-2 shadow"
                                style:left=move || format!("{}px", at().0)
                                style:top=move || format!("{}px", at().1)
                            >
                                {n.label}
                            </div>
                        }
                    }
                />
            </div>
        </div>
    }
}
```
- "Fit everything" zoom: keep a lower limit (`zoom.max(0.5)`): the preview is
  much narrower than the real window, and a wide drawing would shrink to an
  unreadable size there.
- Shortcut keys for the stage (Delete, Escape, +, -) with `window_event_listener`
  (topic `leptos-components`) must skip keys meant for a field:
  ```rust
  /// Whether the user is typing in a field: then keys are theirs, not shortcuts.
  fn is_typing() -> bool {
      web_sys::window()
          .and_then(|w| w.document())
          .and_then(|d| d.active_element())
          .is_some_and(|el| {
              matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT")
                  || el.has_attribute("contenteditable")
          })
  }
  ```
  and at the start of the key handler: `if is_typing() { return; }`.

## Drawing on a `<canvas>` (big grids, charts with thousands of points)
Thousands of cells as elements make scrolling slow; a `<canvas>` that draws only
the visible part stays fast. Features: add `HtmlCanvasElement` and
`CanvasRenderingContext2d` to web-sys.
- Get the context from the element of a `NodeRef::<leptos::html::Canvas>`, and
  keep it in `StoredValue::new_local(…)`: it is not `Send`, so `StoredValue::new`
  does not take it.
- Scrolling: an `overflow-auto` box holds a `<div>` as big as the whole grid
  (it only makes the scroll bars) and the canvas `sticky top-0 left-0` on top.
  On `scroll`, read `scroll_top()` / `scroll_left()` (whole pixels, `i32`; the
  setters take `i32` too: `x as i32`) and draw again.
- Sharp on high-DPI screens: canvas `width`/`height` = CSS size ×
  `window.device_pixel_ratio()`, then `ctx.scale(ratio, ratio)` once.
- Colours take a string: `set_fill_style_str("#111827")`,
  `set_stroke_style_str(…)`; `fill_text` returns a `Result` (`let _ =`).
- Mouse: `on:pointerdown` gives a `PointerEvent`, `on:dblclick` a `MouseEvent`;
  a helper used by both takes `&web_sys::MouseEvent` (`&*e` from a pointer event).
- Typing into a cell: a real `<input>` placed over the cell while editing (IME
  and the clipboard then just work); copy and paste for the selection: topic
  `leptos-components`.
```rust
use wasm_bindgen::JsCast;

const ROW: f64 = 24.0;

/// The 2D drawing context of a `<canvas>`.
fn context_2d(canvas: &web_sys::HtmlCanvasElement) -> Option<web_sys::CanvasRenderingContext2d> {
    canvas.get_context("2d").ok()??.dyn_into().ok()
}

/// Draws only the rows on screen: `first..first + count`.
fn draw_rows(
    ctx: &web_sys::CanvasRenderingContext2d,
    width: f64,
    first: usize,
    count: usize,
    cell: impl Fn(usize) -> String,
) {
    ctx.clear_rect(0.0, 0.0, width, count as f64 * ROW);
    ctx.set_font("14px sans-serif");
    for i in 0..count {
        let y = i as f64 * ROW;
        ctx.set_stroke_style_str("#e5e7eb");
        ctx.stroke_rect(0.0, y, width, ROW);
        ctx.set_fill_style_str("#111827");
        let _ = ctx.fill_text(&cell(first + i), 6.0, y + 17.0);
    }
}
```
With `scroll_top` from the box: `first = scroll_top as usize / 24`, `count` =
the box's height / 24 + 2.

## Checking it in the preview
- `interact` `click` works on SVG shapes that have a `data-ak-id` (the hit
  path), and `dom_summary` lists them (SVG parts without one, such as icons, are
  left out). Projects made before pack 0.2.13 have an older `ak/ak-bridge`:
  there report the selection with `set_state_reader` and check it with
  `read_state`.
- `interact` `drag` with `ak_id` and `to` presses, moves and lets go (pointer
  events) from one element to another: a box onto a spot marked by another
  element.
