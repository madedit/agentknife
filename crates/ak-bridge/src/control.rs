//! Control actions inside the preview page (wasm, `dev` only): the agent clicks,
//! types and reads the page to check its own work.

use std::cell::RefCell;

use serde_json::{Value, json};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    DataTransfer, DragEvent, DragEventInit, Element, Event, EventInit, HtmlElement,
    HtmlInputElement, HtmlSelectElement, HtmlTextAreaElement, KeyboardEvent, KeyboardEventInit,
    MouseEvent, MouseEventInit, PointerEvent, PointerEventInit,
};

use crate::protocol::{
    AppState, ControlAction, DomItem, DomSummary, MAX_HOLD_MS, Rect, ScreenshotArea, Size,
    error_file, key_code, parse_key, redact_bridge, short_text, strip_bridge_param,
};

/// More than this many items is a sign of a list; the agent gets the start.
const MAX_ITEMS: usize = 400;
const TEXT_MAX: usize = 120;
const OVERLAY_ID: &str = "__ak_inspector_overlay";

type StateReader = Box<dyn Fn() -> Value>;

/// Page errors kept for `dom_summary` (the newest).
const MAX_ERRORS: usize = 10;
const ERROR_MAX: usize = 300;

thread_local! {
    static STATE_READER: RefCell<Option<StateReader>> = const { RefCell::new(None) };
    static ERRORS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

pub fn set_state_reader(f: impl Fn() -> Value + 'static) {
    STATE_READER.with(|r| *r.borrow_mut() = Some(Box::new(f)));
}

fn field(v: &wasm_bindgen::JsValue, name: &str) -> wasm_bindgen::JsValue {
    js_sys::Reflect::get(v, &name.into()).unwrap_or(wasm_bindgen::JsValue::UNDEFINED)
}

pub(crate) fn remember_error(text: String) {
    // try_*: this also runs from the panic hook, which may fire anywhere.
    let _ = ERRORS.try_with(|e| {
        let Ok(mut e) = e.try_borrow_mut() else {
            return;
        };
        if let Some(text) = short_text(&redact_bridge(&text), ERROR_MAX) {
            if e.len() == MAX_ERRORS {
                e.remove(0);
            }
            e.push(text);
        }
    });
}

/// Records Rust panics in the screens with their message and place (the
/// browser itself only reports `RuntimeError: unreachable`), and prints them to
/// the console.
pub fn watch_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let text = format!("Rust panic: {info}");
        web_sys::console::error_1(&text.as_str().into());
        remember_error(text);
        previous(info);
    }));
}

/// Records uncaught errors and rejected promises (e.g. from the app's own
/// JavaScript), so the agent sees why a canvas stays blank.
pub fn watch_errors() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let on_error = wasm_bindgen::closure::Closure::<dyn Fn(Event)>::new(|ev: Event| {
        let message = field(&ev, "message").as_string().unwrap_or_default();
        let url = field(&ev, "filename").as_string().unwrap_or_default();
        let line = field(&ev, "lineno").as_f64().unwrap_or(0.0);
        remember_error(match error_file(&url) {
            Some(file) => format!("{message} ({file}:{line})"),
            None => message,
        });
    });
    let on_rejection = wasm_bindgen::closure::Closure::<dyn Fn(Event)>::new(|ev: Event| {
        let reason = field(&ev, "reason");
        let text = js_sys::Reflect::get(&reason, &"message".into())
            .ok()
            .and_then(|m| m.as_string())
            .or_else(|| reason.as_string())
            .unwrap_or_else(|| "a promise was rejected".into());
        remember_error(format!("Unhandled rejection: {text}"));
    });
    let _ = window.add_event_listener_with_callback("error", on_error.as_ref().unchecked_ref());
    let _ = window.add_event_listener_with_callback(
        "unhandledrejection",
        on_rejection.as_ref().unchecked_ref(),
    );
    // For the page's lifetime.
    on_error.forget();
    on_rejection.forget();
}

pub async fn run(action: ControlAction) -> Result<Value, String> {
    match action {
        ControlAction::Click { ak_id } => {
            let (el, html) = pressable(&ak_id)?;
            press(&el, true);
            press(&el, false);
            click(&el, html.as_ref());
            settle().await;
            Ok(json!({ "clicked": ak_id, "tag": tag(&el) }))
        }
        ControlAction::Scroll { ak_id, by } => {
            let el = match &ak_id {
                Some(id) => find(id)?,
                None => web_sys::window()
                    .and_then(|w| w.document())
                    .and_then(|d| d.scrolling_element())
                    .ok_or("no page to scroll")?,
            };
            match by {
                Some(dy) => el.scroll_by_with_x_and_y(0.0, f64::from(dy)),
                None => el.scroll_into_view(),
            }
            settle().await;
            Ok(json!({
                "scrolled": ak_id.as_deref().unwrap_or("page"),
                "top": el.scroll_top(),
                "content_height": el.scroll_height(),
                "visible_height": el.client_height(),
            }))
        }
        ControlAction::Drag { ak_id, to } => {
            let (from, html) = pressable(&ak_id)?;
            let target = find(&to)?;
            let draggable = html.is_some_and(|h| h.draggable());
            let dropped = drag(&from, draggable, &target).await?;
            settle().await;
            Ok(json!({
                "dragged": ak_id,
                "to": to,
                "kind": if draggable { "html5 drag and drop" } else { "pointer down, move, up" },
                "dropped": dropped,
            }))
        }
        ControlAction::Hold { ak_id, ms } => {
            let (el, html) = pressable(&ak_id)?;
            let ms = ms.min(MAX_HOLD_MS);
            press(&el, true);
            sleep(ms).await;
            press(&el, false);
            click(&el, html.as_ref());
            settle().await;
            Ok(json!({ "held": ak_id, "ms": ms, "tag": tag(&el) }))
        }
        ControlAction::Key { key, ms } => {
            let document = web_sys::window()
                .and_then(|w| w.document())
                .ok_or("no document")?;
            // Where a real key goes: the focused element, else the page.
            let target: Element = document
                .active_element()
                .or_else(|| document.body().map(Into::into))
                .ok_or("no page body")?;
            let ms = ms.min(MAX_HOLD_MS);
            let press = parse_key(&key);
            let init = KeyboardEventInit::new();
            init.set_key(&press.key);
            init.set_code(&key_code(&press.key));
            init.set_ctrl_key(press.ctrl);
            init.set_shift_key(press.shift);
            init.set_alt_key(press.alt);
            init.set_meta_key(press.meta);
            init.set_bubbles(true);
            init.set_cancelable(true);
            let mut clicked = None;
            for (kind, wait) in [("keydown", ms), ("keyup", 0)] {
                if let Ok(ev) = KeyboardEvent::new_with_keyboard_event_init_dict(kind, &init) {
                    let not_prevented = target.dispatch_event(&ev).unwrap_or(false);
                    // A made-up Tab does not move the focus: do what a real one does.
                    if kind == "keydown" && press.key == "Tab" && not_prevented {
                        move_focus(&document, press.shift);
                    }
                    // Nor does Space or Enter press the focused button, as a real
                    // one does (a game that does not prevent it restarts).
                    if kind == "keydown"
                        && not_prevented
                        && presses_button(&target, &press.key)
                        && let Some(button) = target.dyn_ref::<HtmlElement>()
                    {
                        button.click();
                        clicked = Some(
                            target
                                .get_attribute("data-ak-id")
                                .unwrap_or_else(|| tag(&target)),
                        );
                    }
                }
                sleep(wait).await;
            }
            settle().await;
            // Where the focus is now (Tab moves it): the nearest data-ak-id.
            let focus = document
                .active_element()
                .and_then(|el| el.closest("[data-ak-id]").ok().flatten())
                .and_then(|el| el.get_attribute("data-ak-id"));
            Ok(
                json!({ "pressed": key, "ms": ms, "on": tag(&target), "focus": focus, "clicked": clicked }),
            )
        }
        ControlAction::Type { ak_id, text, enter } => {
            let el = find(&ak_id)?;
            set_text(&el, &text).ok_or_else(|| format!("`{ak_id}` is not a text field"))?;
            if enter {
                press_enter(&el);
            }
            settle().await;
            Ok(json!({ "typed": ak_id, "tag": tag(&el) }))
        }
        ControlAction::DomSummary => {
            settle().await;
            serde_json::to_value(dom_summary()?).map_err(|e| e.to_string())
        }
        ControlAction::AppState => serde_json::to_value(app_state()?).map_err(|e| e.to_string()),
        ControlAction::PrepareScreenshot { ak_id } => {
            let window = web_sys::window().ok_or("no window")?;
            let target = match &ak_id {
                Some(id) => Some(find(id)?),
                None => None,
            };
            if let Some(el) = &target {
                el.scroll_into_view();
            }
            settle().await;
            let viewport = Size {
                width: window
                    .inner_width()
                    .ok()
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
                height: window
                    .inner_height()
                    .ok()
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0),
            };
            let rect = match &target {
                Some(el) => {
                    let r = el.get_bounding_client_rect();
                    Rect {
                        x: r.x(),
                        y: r.y(),
                        width: r.width(),
                        height: r.height(),
                    }
                }
                None => Rect {
                    x: 0.0,
                    y: 0.0,
                    width: viewport.width,
                    height: viewport.height,
                },
            };
            serde_json::to_value(ScreenshotArea {
                rect,
                viewport,
                device_pixel_ratio: window.device_pixel_ratio(),
            })
            .map_err(|e| e.to_string())
        }
    }
}

/// Whether `key` on the focused `el` clicks it in a real browser: Space or
/// Enter on a button, Enter on a link.
fn presses_button(el: &Element, key: &str) -> bool {
    let kind = el
        .get_attribute("type")
        .unwrap_or_default()
        .to_ascii_lowercase();
    match el.tag_name().to_ascii_lowercase().as_str() {
        "button" => matches!(key, " " | "Enter"),
        "input" if matches!(kind.as_str(), "button" | "submit" | "reset") => {
            matches!(key, " " | "Enter")
        }
        "a" => key == "Enter" && el.has_attribute("href"),
        _ => false,
    }
}

/// Elements Tab stops at (unless hidden, disabled or `tabindex="-1"`).
const FOCUSABLE: &str = "a[href], button, input, select, textarea, [tabindex], [contenteditable]";

/// Moves the focus to the next (`back`: previous) element Tab stops at, in page
/// order, wrapping around like the browser.
fn move_focus(document: &web_sys::Document, back: bool) {
    let Ok(all) = document.query_selector_all(FOCUSABLE) else {
        return;
    };
    let stops: Vec<HtmlElement> = (0..all.length())
        .filter_map(|i| all.item(i))
        .filter_map(|n| n.dyn_into::<HtmlElement>().ok())
        .filter(|el| {
            let rect = el.get_bounding_client_rect();
            el.tab_index() >= 0
                && !el.has_attribute("disabled")
                && (rect.width() > 0.0 || rect.height() > 0.0)
        })
        .collect();
    let n = stops.len();
    if n == 0 {
        return;
    }
    let current = document.active_element();
    let at = stops
        .iter()
        .position(|el| current.as_ref() == Some(el.unchecked_ref::<Element>()));
    let next = match (at, back) {
        (Some(i), false) => (i + 1) % n,
        (Some(i), true) => (i + n - 1) % n,
        (None, false) => 0,
        (None, true) => n - 1,
    };
    let _ = stops[next].focus();
}

fn tag(el: &Element) -> String {
    el.tag_name().to_ascii_lowercase()
}

fn find(ak_id: &str) -> Result<Element, String> {
    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or("no document")?;
    let all = document
        .query_selector_all("[data-ak-id]")
        .map_err(|_| "query failed")?;
    (0..all.length())
        .filter_map(|i| all.item(i))
        .filter_map(|n| n.dyn_into::<Element>().ok())
        .find(|e| e.get_attribute("data-ak-id").as_deref() == Some(ak_id))
        .ok_or_else(|| format!("no element with data-ak-id `{ak_id}` on the page"))
}

fn fire(el: &Element, kind: &str) {
    let init = EventInit::new();
    init.set_bubbles(true);
    if let Ok(ev) = Event::new_with_event_init_dict(kind, &init) {
        let _ = el.dispatch_event(&ev);
    }
}

/// Sets the field like a user would: focus, new value, `input`, `change`.
fn set_text(el: &Element, text: &str) -> Option<()> {
    if let Some(html) = el.dyn_ref::<HtmlElement>() {
        let _ = html.focus();
    }
    if let Some(input) = el.dyn_ref::<HtmlInputElement>() {
        input.set_value(text);
    } else if let Some(area) = el.dyn_ref::<HtmlTextAreaElement>() {
        area.set_value(text);
    } else if el
        .get_attribute("contenteditable")
        .is_some_and(|v| v != "false")
    {
        el.set_text_content(Some(text));
    } else {
        return None;
    }
    fire(el, "input");
    fire(el, "change");
    Some(())
}

/// The element to press, scrolled into view and focused (HTML elements; SVG
/// shapes have no focus).
fn pressable(ak_id: &str) -> Result<(Element, Option<HtmlElement>), String> {
    let el = find(ak_id)?;
    if el.has_attribute("disabled") {
        return Err(format!("`{ak_id}` is disabled"));
    }
    el.scroll_into_view();
    let html = el.dyn_ref::<HtmlElement>().cloned();
    if let Some(html) = &html {
        let _ = html.focus();
    }
    Ok((el, html))
}

/// The `click` after down and up: the element's own (HTML), else a dispatched
/// one (an SVG shape).
fn click(el: &Element, html: Option<&HtmlElement>) {
    if let Some(html) = html {
        html.click();
        return;
    }
    let (x, y) = centre(el);
    let init = MouseEventInit::new();
    init.set_bubbles(true);
    init.set_cancelable(true);
    init.set_client_x(x);
    init.set_client_y(y);
    if let Ok(ev) = MouseEvent::new_with_mouse_event_init_dict("click", &init) {
        let _ = el.dispatch_event(&ev);
    }
}

/// The element's centre on the screen.
fn centre(el: &Element) -> (i32, i32) {
    let r = el.get_bounding_client_rect();
    #[allow(clippy::cast_possible_truncation)]
    (
        (r.x() + r.width() / 2.0) as i32,
        (r.y() + r.height() / 2.0) as i32,
    )
}

/// What a mouse button sends going down or up at the element's centre:
/// `pointerdown`/`mousedown` or `pointerup`/`mouseup`.
fn press(el: &Element, down: bool) {
    let (pointer, mouse) = if down {
        ("pointerdown", "mousedown")
    } else {
        ("pointerup", "mouseup")
    };
    pointer_event(el, pointer, mouse, centre(el), u16::from(down));
}

/// Drags `from` onto `to`: whether the drop was accepted (HTML5 drag and drop,
/// `from` draggable), or `None` (pointer moves; the app decides).
async fn drag(from: &Element, draggable: bool, to: &Element) -> Result<Option<bool>, String> {
    let (start, end) = (centre(from), centre(to));
    if !draggable {
        pointer_event(from, "pointerdown", "mousedown", start, 1);
        let document = web_sys::window()
            .and_then(|w| w.document())
            .ok_or("no document")?;
        const STEPS: i32 = 5;
        for i in 1..=STEPS {
            let at = (
                start.0 + (end.0 - start.0) * i / STEPS,
                start.1 + (end.1 - start.1) * i / STEPS,
            );
            // What is under the pointer gets the move, as with a real mouse.
            #[allow(clippy::cast_precision_loss)]
            let under = document
                .element_from_point(at.0 as f32, at.1 as f32)
                .unwrap_or_else(|| to.clone());
            pointer_event(&under, "pointermove", "mousemove", at, 1);
            settle().await;
        }
        pointer_event(to, "pointerup", "mouseup", end, 0);
        return Ok(None);
    }
    let data = DataTransfer::new().map_err(|_| "drag and drop is not available")?;
    // Whether the page let it happen (no preventDefault).
    let send = |el: &Element, kind: &str, (x, y): (i32, i32)| {
        let init = DragEventInit::new();
        init.set_bubbles(true);
        init.set_cancelable(true);
        init.set_client_x(x);
        init.set_client_y(y);
        init.set_data_transfer(Some(&data));
        DragEvent::new_with_event_init_dict(kind, &init)
            .map(|ev| el.dispatch_event(&ev).unwrap_or(true))
            .unwrap_or(true)
    };
    if !send(from, "dragstart", start) {
        return Ok(Some(false));
    }
    settle().await;
    send(to, "dragenter", end);
    // A target that takes drops calls preventDefault on dragover.
    let accepted = !send(to, "dragover", end);
    if accepted {
        data.set_drop_effect("move");
        send(to, "drop", end);
    } else {
        data.set_drop_effect("none");
        send(to, "dragleave", end);
    }
    send(from, "dragend", start);
    Ok(Some(accepted))
}

/// A pointer event and the matching mouse event at `(x, y)` on `el`.
fn pointer_event(el: &Element, pointer: &str, mouse: &str, (x, y): (i32, i32), buttons: u16) {
    let init = PointerEventInit::new();
    init.set_bubbles(true);
    init.set_cancelable(true);
    init.set_client_x(x);
    init.set_client_y(y);
    init.set_buttons(buttons);
    init.set_pointer_id(1);
    init.set_pointer_type("mouse");
    init.set_is_primary(true);
    if let Ok(ev) = PointerEvent::new_with_event_init_dict(pointer, &init) {
        let _ = el.dispatch_event(&ev);
    }
    let init = MouseEventInit::new();
    init.set_bubbles(true);
    init.set_cancelable(true);
    init.set_client_x(x);
    init.set_client_y(y);
    init.set_buttons(buttons);
    if let Ok(ev) = MouseEvent::new_with_mouse_event_init_dict(mouse, &init) {
        let _ = el.dispatch_event(&ev);
    }
}

/// Waits `ms` milliseconds (none for 0).
async fn sleep(ms: u32) {
    let Some(window) = web_sys::window() else {
        return;
    };
    if ms == 0 {
        return;
    }
    let ms = i32::try_from(ms).unwrap_or(i32::MAX);
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
    });
    let _ = JsFuture::from(promise).await;
}

/// Enter key events; if nothing handled them, submits the field's form.
fn press_enter(el: &Element) {
    let init = KeyboardEventInit::new();
    init.set_key("Enter");
    init.set_code("Enter");
    init.set_bubbles(true);
    init.set_cancelable(true);
    let handled = KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init)
        .map(|ev| !el.dispatch_event(&ev).unwrap_or(true))
        .unwrap_or(false);
    if let Ok(ev) = KeyboardEvent::new_with_keyboard_event_init_dict("keyup", &init) {
        let _ = el.dispatch_event(&ev);
    }
    if !handled
        && let Some(form) = el
            .dyn_ref::<HtmlInputElement>()
            .and_then(HtmlInputElement::form)
    {
        let _ = form.request_submit();
    }
}

/// Lets the app react: two animation frames (paint), or a short timeout when
/// the page is not being painted (hidden frame).
async fn settle() {
    for _ in 0..2 {
        let Some(window) = web_sys::window() else {
            return;
        };
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            let _ = window.request_animation_frame(&resolve);
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 100);
        });
        let _ = JsFuture::from(promise).await;
    }
}

fn app_state() -> Result<AppState, String> {
    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;
    let mut local_storage = serde_json::Map::new();
    if let Ok(Some(storage)) = window.local_storage() {
        let len = storage.length().unwrap_or(0);
        for i in 0..len {
            if let Ok(Some(key)) = storage.key(i)
                && let Ok(Some(raw)) = storage.get_item(&key)
            {
                // Apps usually store JSON; show it as JSON when it is.
                let value = serde_json::from_str(&raw).unwrap_or(Value::String(raw));
                local_storage.insert(key, value);
            }
        }
    }
    Ok(AppState {
        url: strip_bridge_param(&window.location().href().unwrap_or_default()),
        title: document.title(),
        local_storage,
        app: STATE_READER.with(|r| r.borrow().as_ref().map(|f| f())),
    })
}

const INTERACTIVE: &[&str] = &["a", "button", "input", "select", "textarea", "label"];
const TEXT_BLOCKS: &[&str] = &[
    "h1", "h2", "h3", "h4", "h5", "h6", "p", "li", "td", "th", "img", "canvas",
];
const SKIP: &[&str] = &["script", "style", "noscript", "template", "head"];
const SVG_NS: &str = "http://www.w3.org/2000/svg";

fn dom_summary() -> Result<DomSummary, String> {
    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;
    let body = document.body().ok_or("no body")?;
    let mut items = Vec::new();
    let mut omitted = 0;
    walk(&body, 0, &mut items, &mut omitted);
    Ok(DomSummary {
        title: document.title(),
        url: strip_bridge_param(&window.location().href().unwrap_or_default()),
        items,
        omitted,
        errors: ERRORS.with(|e| e.borrow().clone()),
    })
}

/// Not rendered (display: none, detached) or explicitly hidden.
fn hidden(el: &Element) -> bool {
    el.get_client_rects().length() == 0 || el.has_attribute("hidden")
}

fn walk(el: &Element, depth: usize, out: &mut Vec<DomItem>, omitted: &mut usize) {
    let children = el.children();
    for i in 0..children.length() {
        let Some(child) = children.item(i) else {
            continue;
        };
        let tag = tag(&child);
        if SKIP.contains(&tag.as_str()) || child.id() == OVERLAY_ID || hidden(&child) {
            continue;
        }
        let ak_id = child.get_attribute("data-ak-id");
        if !reported(&child) {
            walk(&child, depth, out, omitted);
            continue;
        }
        if out.len() >= MAX_ITEMS {
            *omitted += 1;
            walk(&child, depth + 1, &mut Vec::new(), omitted);
            continue;
        }
        let at = out.len();
        out.push(item(&child, tag, ak_id, depth));
        walk(&child, depth + 1, out, omitted);
        // Leaves get their full text; containers the text that no reported child
        // shows (e.g. a row's label in a plain <span> next to its buttons).
        let leaf = out.len() == at + 1;
        let text = if leaf {
            child
                .dyn_ref::<HtmlElement>()
                .and_then(|h| short_text(&h.inner_text(), TEXT_MAX))
        } else {
            let mut text = String::new();
            loose_text(&child, &mut text);
            short_text(&text, TEXT_MAX)
        };
        if let Some(slot) = out.get_mut(at) {
            slot.text = text.or_else(|| child.get_attribute("alt"));
        }
    }
}

/// Elements that get their own line in the summary.
fn reported(el: &Element) -> bool {
    // Inside an SVG only the shapes the app named (not icons' paths).
    if el.namespace_uri().as_deref() == Some(SVG_NS) {
        return el.has_attribute("data-ak-id");
    }
    let tag = tag(el);
    el.has_attribute("data-ak-id")
        || INTERACTIVE.contains(&tag.as_str())
        || TEXT_BLOCKS.contains(&tag.as_str())
        || el.has_attribute("role")
}

/// Text inside `el`, skipping reported (or hidden) descendants: they have their
/// own lines.
fn loose_text(el: &Element, out: &mut String) {
    let nodes = el.child_nodes();
    for i in 0..nodes.length() {
        let Some(n) = nodes.item(i) else {
            continue;
        };
        if n.node_type() == web_sys::Node::TEXT_NODE {
            out.push_str(&n.text_content().unwrap_or_default());
            out.push(' ');
        } else if let Some(child) = n.dyn_ref::<Element>()
            && !reported(child)
            && !hidden(child)
            && !SKIP.contains(&tag(child).as_str())
        {
            loose_text(child, out);
        }
    }
}

fn item(el: &Element, tag: String, ak_id: Option<String>, depth: usize) -> DomItem {
    let mut attrs = Vec::new();
    // The aria states show which button of a group is chosen, which tab is open.
    for name in [
        "type",
        "role",
        "href",
        "placeholder",
        "aria-label",
        "name",
        "aria-pressed",
        "aria-checked",
        "aria-selected",
        "aria-expanded",
        "aria-current",
    ] {
        if let Some(v) = el.get_attribute(name).filter(|v| !v.is_empty()) {
            attrs.push((name.to_owned(), v));
        }
    }
    // A canvas with no height shows nothing and reports no error.
    if tag == "canvas" {
        let rect = el.get_bounding_client_rect();
        attrs.push((
            "size".to_owned(),
            format!("{:.0}x{:.0}", rect.width(), rect.height()),
        ));
    }
    let (value, checked) = if let Some(input) = el.dyn_ref::<HtmlInputElement>() {
        match input.type_().as_str() {
            "checkbox" | "radio" => (None, Some(input.checked())),
            "password" => (Some("********".to_owned()), None),
            _ => (Some(input.value()), None),
        }
    } else if let Some(area) = el.dyn_ref::<HtmlTextAreaElement>() {
        (Some(area.value()), None)
    } else if let Some(select) = el.dyn_ref::<HtmlSelectElement>() {
        (Some(select.value()), None)
    } else {
        (None, None)
    };
    DomItem {
        depth,
        tag,
        ak_id,
        attrs,
        text: None,
        value,
        checked,
        disabled: el.has_attribute("disabled"),
    }
}
