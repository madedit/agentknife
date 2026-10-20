//! Point-and-select inside the preview (wasm, `dev` only).
//!
//! AgentKnife enables it with `postMessage`; then hovering highlights the element
//! (preferring the closest one with a `data-ak-id`), and a click reports it
//! instead of reaching the app. Messages are accepted only from the parent
//! window, and answers go only to the origin that enabled the inspector.

use std::cell::RefCell;

use serde_json::Value;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Element, HtmlCanvasElement, HtmlElement, MessageEvent, MouseEvent, Window};

use crate::client::HitTestInput;
use crate::protocol::{
    CanvasPoint, INSPECTOR_KEY, InspectorCommand, InspectorEvent, PathStep, Picked, Rect, Size,
    css_path, short_text,
};

const OVERLAY_ID: &str = "__ak_inspector_overlay";
const TEXT_MAX: usize = 80;

type HitTest = Box<dyn Fn(&HitTestInput) -> Option<Value>>;
type Listener = Closure<dyn FnMut(web_sys::Event)>;

#[derive(Default)]
struct State {
    installed: bool,
    enabled: bool,
    /// The origin that enabled the inspector; replies go only there.
    parent_origin: Option<String>,
    last_hover: Option<Element>,
    hit_test: Option<HitTest>,
    /// Kept alive for the page's lifetime.
    listeners: Vec<Listener>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub fn set_hit_test(f: impl Fn(&HitTestInput) -> Option<Value> + 'static) {
    STATE.with(|s| s.borrow_mut().hit_test = Some(Box::new(f)));
}

/// Starts listening for AgentKnife, when the page runs inside a frame.
pub fn install() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let in_frame = window
        .parent()
        .ok()
        .flatten()
        .is_some_and(|p| !js_sys::Object::is(&p, &window));
    if !in_frame || STATE.with(|s| s.borrow().installed) {
        return;
    }
    let mut listeners = Vec::new();
    listeners.push(listen(&window, "message", false, on_message));
    // Capture phase: the inspector sees pointer input before the app does.
    listeners.push(listen(&window, "mousemove", true, on_move));
    for kind in [
        "click",
        "mousedown",
        "mouseup",
        "pointerdown",
        "pointerup",
        "dblclick",
    ] {
        listeners.push(listen(&window, kind, true, on_press));
    }
    listeners.push(listen(&window, "keydown", true, on_key));
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.installed = true;
        s.listeners = listeners;
    });
    // Tell AgentKnife the preview can be inspected (it may not know our origin yet).
    post_to(&window, &InspectorEvent::Ready, "*");
}

fn listen(window: &Window, kind: &str, capture: bool, f: fn(web_sys::Event)) -> Listener {
    let closure = Listener::new(f);
    let options = web_sys::AddEventListenerOptions::new();
    options.set_capture(capture);
    let _ = window.add_event_listener_with_callback_and_add_event_listener_options(
        kind,
        closure.as_ref().unchecked_ref(),
        &options,
    );
    closure
}

fn post_to(window: &Window, event: &InspectorEvent, origin: &str) {
    let Ok(Some(parent)) = window.parent() else {
        return;
    };
    let Ok(body) = serde_json::to_value(event) else {
        return;
    };
    let text = serde_json::json!({ INSPECTOR_KEY: body }).to_string();
    let _ = parent.post_message(&JsValue::from_str(&text), origin);
}

fn post(event: &InspectorEvent) {
    let origin = STATE.with(|s| s.borrow().parent_origin.clone());
    if let (Some(window), Some(origin)) = (web_sys::window(), origin) {
        post_to(&window, event, &origin);
    }
}

fn on_message(e: web_sys::Event) {
    let Some(e) = e.dyn_ref::<MessageEvent>() else {
        return;
    };
    let Some(window) = web_sys::window() else {
        return;
    };
    // Only the embedding page (AgentKnife) may drive the inspector.
    let from_parent = match (e.source(), window.parent().ok().flatten()) {
        (Some(source), Some(parent)) => js_sys::Object::is(&source, &parent),
        _ => false,
    };
    if !from_parent {
        return;
    }
    let Some(text) = e.data().as_string() else {
        return;
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return;
    };
    let Ok(cmd) = serde_json::from_value::<InspectorCommand>(v[INSPECTOR_KEY].clone()) else {
        return;
    };
    match cmd {
        InspectorCommand::Enable => {
            STATE.with(|s| {
                let mut s = s.borrow_mut();
                s.enabled = true;
                s.parent_origin = Some(e.origin());
            });
            // Clicks no longer move focus into the frame (their default is
            // prevented), so take it now: Escape must reach us.
            let _ = window.focus();
        }
        InspectorCommand::Disable => {
            STATE.with(|s| {
                let mut s = s.borrow_mut();
                s.enabled = false;
                s.last_hover = None;
            });
            remove_overlay();
        }
    }
}

fn enabled() -> bool {
    STATE.with(|s| s.borrow().enabled)
}

fn on_move(e: web_sys::Event) {
    if !enabled() {
        return;
    }
    let Some(me) = e.dyn_ref::<MouseEvent>() else {
        return;
    };
    let Some(raw) = element_at(me) else {
        return;
    };
    let shown = highlight_target(&raw);
    let changed = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let same = s
            .last_hover
            .as_ref()
            .is_some_and(|last| js_sys::Object::is(last, &raw));
        s.last_hover = Some(raw.clone());
        !same
    });
    show_overlay(&shown);
    if changed && let Some(target) = pick(&raw, me) {
        post(&InspectorEvent::Hover { target });
    }
}

fn on_press(e: web_sys::Event) {
    if !enabled() {
        return;
    }
    // The app must not react to clicks while the user is pointing.
    e.prevent_default();
    e.stop_immediate_propagation();
    if e.type_() != "click" {
        return;
    }
    let Some(me) = e.dyn_ref::<MouseEvent>() else {
        return;
    };
    if let Some(target) = element_at(me).and_then(|raw| pick(&raw, me)) {
        post(&InspectorEvent::Select { target });
    }
}

fn on_key(e: web_sys::Event) {
    let Some(k) = e.dyn_ref::<web_sys::KeyboardEvent>() else {
        return;
    };
    if enabled() && k.key() == "Escape" {
        e.prevent_default();
        e.stop_immediate_propagation();
        post(&InspectorEvent::Cancel);
    }
}

fn element_at(me: &MouseEvent) -> Option<Element> {
    let document = web_sys::window()?.document()?;
    let el = document.element_from_point(me.client_x() as f32, me.client_y() as f32)?;
    // Never pick our own overlay (it has pointer-events: none, but be safe).
    (el.id() != OVERLAY_ID).then_some(el)
}

/// The element to highlight: the closest one with a `data-ak-id`, else the raw one.
fn highlight_target(raw: &Element) -> Element {
    raw.closest("[data-ak-id]")
        .ok()
        .flatten()
        .unwrap_or_else(|| raw.clone())
}

fn pick(raw: &Element, me: &MouseEvent) -> Option<Picked> {
    let window = web_sys::window()?;
    let shown = highlight_target(raw);
    let ak_id = shown.get_attribute("data-ak-id");
    let exact = raw.has_attribute("data-ak-id");
    let r = shown.get_bounding_client_rect();
    let text = shown
        .dyn_ref::<HtmlElement>()
        .map(HtmlElement::inner_text)
        .and_then(|t| short_text(&t, TEXT_MAX));
    let canvas = raw
        .dyn_ref::<HtmlCanvasElement>()
        .map(|c| canvas_point(c, me));
    Some(Picked {
        ak_id,
        exact,
        path: css_path(&path_steps(&shown)),
        tag: shown.tag_name().to_ascii_lowercase(),
        text,
        rect: Rect {
            x: r.x(),
            y: r.y(),
            width: r.width(),
            height: r.height(),
        },
        viewport: Size {
            width: window.inner_width().ok()?.as_f64()?,
            height: window.inner_height().ok()?.as_f64()?,
        },
        canvas,
    })
}

/// The pointer in the canvas's own pixels, plus the app's hit test, if any.
fn canvas_point(canvas: &HtmlCanvasElement, me: &MouseEvent) -> CanvasPoint {
    // The drawing surface is the content box: skip the border, scale by its size.
    let r = canvas.get_bounding_client_rect();
    let scale = |pixels: u32, css: i32| {
        if css > 0 {
            f64::from(pixels) / f64::from(css)
        } else {
            1.0
        }
    };
    let left = r.x() + f64::from(canvas.client_left());
    let top = r.y() + f64::from(canvas.client_top());
    let x = (f64::from(me.client_x()) - left) * scale(canvas.width(), canvas.client_width());
    let y = (f64::from(me.client_y()) - top) * scale(canvas.height(), canvas.client_height());
    let input = HitTestInput {
        ak_id: canvas.get_attribute("data-ak-id"),
        x,
        y,
    };
    let hit = STATE.with(|s| s.borrow().hit_test.as_ref().and_then(|f| f(&input)));
    CanvasPoint { x, y, hit }
}

fn path_steps(el: &Element) -> Vec<PathStep> {
    let mut steps = Vec::new();
    let mut cur = Some(el.clone());
    while let Some(e) = cur {
        let tag = e.tag_name().to_ascii_lowercase();
        if tag == "html" {
            break;
        }
        let parent = e.parent_element();
        let nth_of_type = parent.as_ref().and_then(|p| {
            let children = p.children();
            let same: Vec<Element> = (0..children.length())
                .filter_map(|i| children.item(i))
                .filter(|c| c.tag_name() == e.tag_name())
                .collect();
            if same.len() < 2 {
                return None;
            }
            same.iter()
                .position(|c| js_sys::Object::is(c, &e))
                .map(|i| i + 1)
        });
        let id = Some(e.id()).filter(|id| !id.is_empty() && id != OVERLAY_ID);
        steps.push(PathStep {
            tag,
            id,
            nth_of_type,
        });
        cur = parent;
    }
    steps.reverse();
    steps
}

fn overlay() -> Option<HtmlElement> {
    let document = web_sys::window()?.document()?;
    if let Some(el) = document.get_element_by_id(OVERLAY_ID) {
        return el.dyn_into().ok();
    }
    let el: HtmlElement = document.create_element("div").ok()?.dyn_into().ok()?;
    el.set_id(OVERLAY_ID);
    let style = el.style();
    for (k, v) in [
        ("position", "fixed"),
        ("pointer-events", "none"),
        ("z-index", "2147483647"),
        ("border", "2px solid #3b82f6"),
        ("background", "rgba(59, 130, 246, 0.12)"),
        ("border-radius", "3px"),
        ("box-sizing", "border-box"),
        ("font", "12px/1.4 system-ui, sans-serif"),
        ("color", "#fff"),
    ] {
        let _ = style.set_property(k, v);
    }
    document.body()?.append_child(&el).ok()?;
    Some(el)
}

fn show_overlay(target: &Element) {
    let Some(el) = overlay() else {
        return;
    };
    let r = target.get_bounding_client_rect();
    let style = el.style();
    for (k, v) in [
        ("left", r.x()),
        ("top", r.y()),
        ("width", r.width()),
        ("height", r.height()),
    ] {
        let _ = style.set_property(k, &format!("{v}px"));
    }
    let _ = style.set_property("display", "block");
    // The id label, on a small tab above the box.
    let label = target.get_attribute("data-ak-id").unwrap_or_default();
    el.set_inner_html("");
    if !label.is_empty()
        && let Some(document) = web_sys::window().and_then(|w| w.document())
        && let Ok(tab) = document.create_element("span")
        && let Ok(tab) = tab.dyn_into::<HtmlElement>()
    {
        tab.set_text_content(Some(&label));
        let s = tab.style();
        for (k, v) in [
            ("position", "absolute"),
            ("left", "-2px"),
            ("top", "-20px"),
            ("padding", "0 6px"),
            ("background", "#3b82f6"),
            ("border-radius", "3px"),
            ("white-space", "nowrap"),
        ] {
            let _ = s.set_property(k, v);
        }
        let _ = el.append_child(&tab);
    }
}

fn remove_overlay() {
    if let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(OVERLAY_ID))
    {
        el.remove();
    }
}
