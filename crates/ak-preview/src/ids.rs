//! Adds `data-ak-id` to elements in Leptos `view!` markup that lack one, so the
//! user can point at them in the preview.
//!
//! The markup is parsed the way Leptos parses it (syn for the file, rstml for each
//! `view!` body), so tag boundaries and existing attributes are exact. Rules:
//! - only meaningful lowercase HTML elements ([`kind`]); components are left
//!   alone (ak-ui components take `ak_id` themselves);
//! - never inside closures (`For` children, `.map(|x| view! {…})`, `move ||`): a
//!   fixed id there could repeat on the page;
//! - existing ids are never changed, so running again changes nothing.

use std::collections::BTreeSet;

use proc_macro2::LineColumn;
use rstml::node::{KeyedAttribute, Node, NodeAttribute, NodeBlock, NodeElement};
use syn::spanned::Spanned;
use syn::visit::Visit;

type Element = NodeElement<rstml::Infallible>;

/// One id the pass added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedId {
    pub id: String,
    pub tag: String,
    /// 1-based line of the element.
    pub line: usize,
}

/// The suffix for elements that get an id, by tag; `None` for the rest.
fn kind(tag: &str, input_type: Option<&str>) -> Option<&'static str> {
    Some(match tag {
        "button" => "button",
        "a" => "link",
        "input" => match input_type {
            Some("checkbox") => "checkbox",
            Some("radio") => "radio",
            _ => "input",
        },
        "select" => "select",
        "textarea" => "textarea",
        "form" => "form",
        "label" => "label",
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => "title",
        "p" => "text",
        "img" => "image",
        "canvas" => "canvas",
        "video" => "video",
        "ul" | "ol" => "list",
        "table" => "table",
        "section" => "section",
        "nav" => "nav",
        "header" => "header",
        "footer" => "footer",
        "main" => "main",
        _ => return None,
    })
}

/// `SignupForm` -> `Signup Form` (component names as words).
fn split_camel(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if i > 0 && c.is_ascii_uppercase() {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// Up to three lowercase ASCII words of `text`, joined by `-`.
fn slug(text: &str) -> String {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(3)
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join("-")
}

fn attr<'a>(el: &'a Element, key: &str) -> Option<&'a KeyedAttribute> {
    el.attributes().iter().find_map(|a| match a {
        NodeAttribute::Attribute(k) if k.key.to_string() == key => Some(k),
        _ => None,
    })
}

/// Static text directly inside the element (`"Add item"`).
fn own_text(el: &Element) -> String {
    el.children
        .iter()
        .filter_map(|n| match n {
            Node::Text(t) => Some(t.value_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

struct Pass<'s> {
    /// Ids already in the file (and those we add).
    taken: BTreeSet<String>,
    /// (byte offset, text to insert).
    inserts: Vec<(usize, String)>,
    added: Vec<AddedId>,
    /// Name of the function being visited (the component).
    component: String,
    fallback: &'s str,
    closures: usize,
    line_starts: Vec<usize>,
    source: &'s str,
}

impl Pass<'_> {
    fn offset(&self, at: LineColumn) -> Option<usize> {
        let start = *self.line_starts.get(at.line.checked_sub(1)?)?;
        let line = &self.source[start..];
        let byte = line
            .char_indices()
            .nth(at.column)
            .map_or(line.len(), |(i, _)| i);
        Some(start + byte)
    }

    fn unique(&mut self, base: &str) -> String {
        let mut id = base.to_owned();
        let mut n = 2;
        while self.taken.contains(&id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        self.taken.insert(id.clone());
        id
    }

    fn element(&mut self, el: &Element) {
        let tag = el.name().to_string();
        let html = tag.chars().next().is_some_and(|c| c.is_ascii_lowercase());
        if html && attr(el, "data-ak-id").is_none() {
            let input_type = attr(el, "type").and_then(KeyedAttribute::value_literal_string);
            if let Some(suffix) = kind(&tag, input_type.as_deref()) {
                let lit = |key: &str| attr(el, key).and_then(KeyedAttribute::value_literal_string);
                // A field's kind says more than example text ("email", not
                // "you-example-com").
                let meaningful_type = input_type.clone().filter(|t| {
                    matches!(
                        t.as_str(),
                        "email" | "password" | "search" | "tel" | "url" | "number" | "date"
                    )
                });
                let named_by = [
                    Some(own_text(el)),
                    lit("name"),
                    lit("aria-label"),
                    lit("alt"),
                    lit("title"),
                    lit("id"),
                    meaningful_type,
                    lit("placeholder"),
                ];
                let mut words = named_by
                    .into_iter()
                    .flatten()
                    .map(|t| slug(&t))
                    .find(|s| !s.is_empty())
                    .unwrap_or_default();
                if words.is_empty() {
                    words = slug(&split_camel(&self.component));
                }
                if words.is_empty() {
                    words = slug(self.fallback);
                }
                let base = if words.is_empty() {
                    suffix.to_owned()
                } else if words.ends_with(suffix) {
                    words
                } else {
                    format!("{words}-{suffix}")
                };
                let end = el.open_tag.name.span().end();
                if let Some(at) = self.offset(end) {
                    let id = self.unique(&base);
                    self.inserts.push((at, format!(" data-ak-id=\"{id}\"")));
                    self.added.push(AddedId {
                        id,
                        tag: tag.clone(),
                        line: end.line,
                    });
                }
            }
        }
        self.nodes(&el.children);
    }

    fn nodes(&mut self, nodes: &[Node<rstml::Infallible>]) {
        for node in nodes {
            match node {
                Node::Element(el) => self.element(el),
                Node::Fragment(f) => self.nodes(&f.children),
                // `{ if … { view! {…} } }`: nested markup (closures are skipped
                // by the visitor).
                Node::Block(NodeBlock::ValidBlock(block)) => self.visit_block(block),
                _ => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for Pass<'_> {
    fn visit_item_fn(&mut self, f: &'ast syn::ItemFn) {
        let outer = std::mem::replace(&mut self.component, f.sig.ident.to_string());
        syn::visit::visit_item_fn(self, f);
        self.component = outer;
    }

    fn visit_expr_closure(&mut self, c: &'ast syn::ExprClosure) {
        self.closures += 1;
        syn::visit::visit_expr_closure(self, c);
        self.closures -= 1;
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let is_view = mac.path.segments.last().is_some_and(|s| s.ident == "view");
        if !is_view || self.closures > 0 {
            return;
        }
        let config = rstml::ParserConfig::default().recover_block(true);
        let (nodes, _errors) = rstml::Parser::new(config)
            .parse_recoverable(mac.tokens.clone())
            .split_vec();
        self.nodes(&nodes);
    }
}

/// Ids written in the file already (`data-ak-id="…"`, `ak_id="…"`).
fn existing_ids(source: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for key in ["data-ak-id=\"", "ak_id=\""] {
        for part in source.split(key).skip(1) {
            if let Some(id) = part.split('"').next() {
                out.insert(id.to_owned());
            }
        }
    }
    out
}

/// Adds missing ids to `source` (a Rust file). `fallback` names elements when
/// nothing else does (e.g. the file stem). Returns the new text and what was
/// added; unchanged text when nothing was missing.
pub fn add_missing_ids(source: &str, fallback: &str) -> Result<(String, Vec<AddedId>), String> {
    let file = syn::parse_file(source).map_err(|e| e.to_string())?;
    let mut line_starts = vec![0];
    line_starts.extend(source.match_indices('\n').map(|(i, _)| i + 1));
    let mut pass = Pass {
        taken: existing_ids(source),
        inserts: Vec::new(),
        added: Vec::new(),
        component: String::new(),
        fallback,
        closures: 0,
        line_starts,
        source,
    };
    pass.visit_file(&file);
    let mut out = source.to_owned();
    let mut inserts = pass.inserts;
    inserts.sort_by_key(|i| std::cmp::Reverse(i.0));
    for (at, text) in inserts {
        out.insert_str(at, &text);
    }
    Ok((out, pass.added))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(src: &str) -> (String, Vec<String>) {
        let (out, added) = add_missing_ids(src, "page").unwrap();
        (out, added.into_iter().map(|a| a.id).collect())
    }

    #[test]
    fn adds_ids_from_text_and_attributes() {
        let src = r#"use leptos::prelude::*;

#[component]
fn Home() -> impl IntoView {
    view! {
        <main class="p-4">
            <h1 class="text-2xl">"My Todos"</h1>
            <input type="text" placeholder="What to buy?" prop:value=move || text.get() />
            <input type="checkbox" name="done" />
            <button on:click=move |_| add() class=move || if busy.get() { "a" } else { "b" }>"Add item"</button>
            <div class="row"><span>"not these"</span></div>
            <img src="logo.png" alt="Company logo" />
        </main>
    }
}
"#;
        let (out, added) = ids(src);
        assert_eq!(
            added,
            [
                "home-main",
                "my-todos-title",
                "what-to-buy-input",
                "done-checkbox",
                "add-item-button",
                "company-logo-image"
            ]
        );
        assert!(
            out.contains(r#"<h1 data-ak-id="my-todos-title" class="text-2xl">"#),
            "{out}"
        );
        assert!(out.contains(r#"<button data-ak-id="add-item-button" on:click=move |_| add() class=move || if busy.get() { "a" } else { "b" }>"#), "{out}");
        assert!(
            out.contains(r#"<div class="row">"#),
            "divs and spans are left alone"
        );
        // The result still parses, and a second run changes nothing.
        let (again, more) = ids(&out);
        assert_eq!(again, out);
        assert!(more.is_empty());
    }

    #[test]
    fn existing_ids_components_and_closures_are_left_alone() {
        let src = r#"
#[component]
fn List() -> impl IntoView {
    view! {
        <section data-ak-id="todo-section">
            <Button ak_id="save-button" on_click=move |_| save()>"Save"</Button>
            <For each=move || todos.get() key=|t| t.id children=move |t| view! { <li><button>"Delete"</button></li> } />
            {move || view! { <p>"Reactive branch"</p> }}
            {items.iter().map(|i| view! { <button>"Row"</button> }).collect_view()}
            <ul><li>"first"</li></ul>
        </section>
    }
}
"#;
        let (out, added) = ids(src);
        assert_eq!(added, ["list"], "{out}");
        assert!(out.contains(r#"<For each=move || todos.get() key=|t| t.id children=move |t| view! { <li><button>"Delete"</button></li> } />"#));
        assert!(out.contains(r#"<section data-ak-id="todo-section">"#));
    }

    #[test]
    fn names_are_unique_and_fall_back_for_other_scripts() {
        let src = r#"
fn Settings() -> impl IntoView {
    view! {
        <h2>"設定"</h2>
        <button>"Save"</button>
        <button>"Save"</button>
        <p data-ak-id="save-button">"taken"</p>
    }
}
"#;
        let (_, added) = ids(src);
        assert_eq!(added, ["settings-title", "save-button-2", "save-button-3"]);
    }

    #[test]
    fn nested_blocks_and_non_ascii_lines_land_in_the_right_place() {
        let src = "fn Page() -> impl IntoView {\n    // 註解：中文\n    let 名稱 = \"x\";\n    view! {\n        <div>{ if ok { view! { <p>\"Done\"</p> } } else { view! { <p>\"待辦\"</p> } } }</div>\n    }\n}\n";
        let (out, added) = ids(src);
        assert_eq!(added, ["done-text", "page-text"]);
        assert!(
            out.contains("<p data-ak-id=\"done-text\">\"Done\"</p>"),
            "{out}"
        );
        assert!(
            out.contains("<p data-ak-id=\"page-text\">\"待辦\"</p>"),
            "{out}"
        );
    }

    #[test]
    fn broken_files_are_reported_not_changed() {
        assert!(add_missing_ids("fn x( {", "f").is_err());
        let (out, added) = add_missing_ids("fn main() {}\n", "f").unwrap();
        assert_eq!(out, "fn main() {}\n");
        assert!(added.is_empty());
    }
}
