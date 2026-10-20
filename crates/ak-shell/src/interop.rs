//! CodeMirror 6 glue. The modules come from the vendored bundle; their bare
//! specifiers (`@codemirror/view`) resolve through the import map in
//! index.html. Only plain values are imported (classes, facets, functions as
//! `JsValue`); everything else goes through `Reflect`, so the binding surface
//! stays small. Browser only.

use js_sys::{Array, Function, Object, Reflect};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "codemirror")]
extern "C" {
    #[wasm_bindgen(thread_local_v2, js_name = basicSetup)]
    static BASIC_SETUP: JsValue;
}

#[wasm_bindgen(module = "@codemirror/view")]
extern "C" {
    #[wasm_bindgen(thread_local_v2, js_name = EditorView)]
    static EDITOR_VIEW: JsValue;
    #[wasm_bindgen(thread_local_v2, js_name = keymap)]
    static KEYMAP: JsValue;
}

#[wasm_bindgen(module = "@codemirror/state")]
extern "C" {
    #[wasm_bindgen(thread_local_v2, js_name = Prec)]
    static PREC: JsValue;
}

#[wasm_bindgen(module = "@codemirror/language")]
extern "C" {
    #[wasm_bindgen(thread_local_v2, js_name = StreamLanguage)]
    static STREAM_LANGUAGE: JsValue;
}

#[wasm_bindgen(module = "@codemirror/legacy-modes/mode/toml")]
extern "C" {
    #[wasm_bindgen(thread_local_v2, js_name = toml)]
    static TOML_MODE: JsValue;
}

#[wasm_bindgen(module = "@codemirror/lang-rust")]
extern "C" {
    #[wasm_bindgen(js_name = rust)]
    fn lang_rust() -> JsValue;
}

#[wasm_bindgen(module = "@codemirror/lang-css")]
extern "C" {
    #[wasm_bindgen(js_name = css)]
    fn lang_css() -> JsValue;
}

#[wasm_bindgen(module = "@codemirror/lang-html")]
extern "C" {
    #[wasm_bindgen(js_name = html)]
    fn lang_html() -> JsValue;
}

#[wasm_bindgen(module = "@codemirror/lang-markdown")]
extern "C" {
    #[wasm_bindgen(js_name = markdown)]
    fn lang_markdown() -> JsValue;
}

#[wasm_bindgen(module = "@codemirror/theme-one-dark")]
extern "C" {
    #[wasm_bindgen(thread_local_v2, js_name = oneDark)]
    static ONE_DARK: JsValue;
}

use crate::files::Lang;

fn get(obj: &JsValue, key: &str) -> Result<JsValue, JsValue> {
    Reflect::get(obj, &JsValue::from_str(key))
}

/// `obj.method(...args)`.
fn call(obj: &JsValue, method: &str, args: &[&JsValue]) -> Result<JsValue, JsValue> {
    let f: Function = get(obj, method)?.dyn_into()?;
    let list = Array::new();
    for a in args {
        list.push(a);
    }
    f.apply(obj, &list)
}

fn language(lang: Lang) -> Result<Option<JsValue>, JsValue> {
    Ok(match lang {
        Lang::Rust => Some(lang_rust()),
        Lang::Css => Some(lang_css()),
        Lang::Html => Some(lang_html()),
        Lang::Markdown => Some(lang_markdown()),
        Lang::Toml => {
            let mode = TOML_MODE.with(JsValue::clone);
            Some(STREAM_LANGUAGE.with(|s| call(s, "define", &[&mode]))?)
        }
        Lang::Plain => None,
    })
}

/// One CodeMirror view in `parent`. Dropping it removes the view.
pub struct Editor {
    view: JsValue,
    _on_update: Closure<dyn FnMut(JsValue)>,
    _on_save: Closure<dyn FnMut() -> bool>,
}

impl Editor {
    pub fn new(
        parent: &web_sys::Element,
        text: &str,
        lang: Lang,
        dark: bool,
        on_change: impl Fn(String) + 'static,
        on_save: impl Fn() + 'static,
    ) -> Result<Editor, JsValue> {
        let on_update = Closure::<dyn FnMut(JsValue)>::new(move |update: JsValue| {
            if get(&update, "docChanged").is_ok_and(|v| v.is_truthy())
                && let Ok(text) = doc_text(&get(&update, "state").unwrap_or(JsValue::NULL))
            {
                on_change(text);
            }
        });
        let on_save = Closure::<dyn FnMut() -> bool>::new(move || {
            on_save();
            true
        });

        let extensions = Array::new();
        extensions.push(&BASIC_SETUP.with(JsValue::clone));
        if let Some(l) = language(lang)? {
            extensions.push(&l);
        }
        if dark {
            extensions.push(&ONE_DARK.with(JsValue::clone));
        }
        let view_class = EDITOR_VIEW.with(JsValue::clone);
        let listener = get(&view_class, "updateListener")?;
        extensions.push(&call(&listener, "of", &[on_update.as_ref()])?);
        extensions.push(&get(&view_class, "lineWrapping")?);
        // Ctrl/Cmd+S saves, ahead of the browser's "save page".
        let binding = Object::new();
        Reflect::set(&binding, &"key".into(), &"Mod-s".into())?;
        Reflect::set(&binding, &"run".into(), on_save.as_ref())?;
        let keys = KEYMAP.with(|k| call(k, "of", &[&Array::of1(&binding)]))?;
        extensions.push(&PREC.with(|p| call(p, "high", &[&keys]))?);

        let config = Object::new();
        Reflect::set(&config, &"doc".into(), &text.into())?;
        Reflect::set(&config, &"extensions".into(), &extensions)?;
        Reflect::set(&config, &"parent".into(), parent)?;
        let class: Function = view_class.dyn_into()?;
        let view = Reflect::construct(&class, &Array::of1(&config))?;
        Ok(Editor {
            view,
            _on_update: on_update,
            _on_save: on_save,
        })
    }

    pub fn focus(&self) {
        let _ = call(&self.view, "focus", &[]);
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        let _ = call(&self.view, "destroy", &[]);
    }
}

fn doc_text(state: &JsValue) -> Result<String, JsValue> {
    let doc = get(state, "doc")?;
    Ok(call(&doc, "toString", &[])?.as_string().unwrap_or_default())
}
