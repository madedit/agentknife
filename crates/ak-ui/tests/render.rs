//! Every component renders the caller's `data-ak-id` (and derived ids for its
//! parts) plus the accessibility attributes the agent and screen readers rely on.
//! Rendered to HTML natively; behaviour in the browser is checked with the gallery.

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use ak_ui::*;
use leptos::prelude::*;

fn html<V: IntoView + 'static>(f: impl FnOnce() -> V) -> String {
    let owner = Owner::new();
    owner.with(|| f().into_view().to_html())
}

#[test]
fn button_variants_and_attributes() {
    let out = html(|| {
        view! {
            <Button ak_id="save-button" on_click=|_| {}>"Save"</Button>
            <Button ak_id="delete-button" variant=ButtonVariant::Danger disabled=true>
                "Delete"
            </Button>
            <Button ak_id="send" submit=true label="Send message">"→"</Button>
        }
    });
    assert!(out.contains(r#"data-ak-id="save-button""#), "{out}");
    assert!(out.contains(r#"type="button""#));
    assert!(out.contains("bg-blue-600"));
    assert!(out.contains("bg-red-600"));
    assert!(out.contains(r#"data-ak-id="delete-button""#));
    assert!(out.contains(" disabled"), "{out}");
    assert!(out.contains(r#"type="submit""#));
    assert!(out.contains(r#"aria-label="Send message""#));
}

#[test]
fn input_carries_its_id_and_value() {
    let out = html(|| {
        let value = RwSignal::new("milk".to_owned());
        view! {
            <Input ak_id="todo-input" value=value label="New item" placeholder="What to buy?" />
            <Input ak_id="email" value=RwSignal::new(String::new()) kind="email" />
        }
    });
    assert!(out.contains(r#"data-ak-id="todo-input""#), "{out}");
    assert!(out.contains("New item"));
    assert!(out.contains(r#"placeholder="What to buy?""#));
    assert!(out.contains(r#"type="text""#));
    assert!(out.contains(r#"type="email""#));
}

#[derive(Clone)]
struct Todo {
    id: u32,
    text: &'static str,
}

#[test]
fn list_rows_get_key_based_ids_and_empty_state() {
    let out = html(|| {
        let items = RwSignal::new(vec![
            Todo {
                id: 7,
                text: "milk",
            },
            Todo {
                id: 9,
                text: "eggs",
            },
        ]);
        view! {
            <List
                ak_id="todo-list"
                items=items
                key=|t: &Todo| t.id
                row=|t: Todo| view! { <span>{t.text}</span> }
                empty="Nothing yet"
            />
        }
    });
    assert!(out.contains(r#"data-ak-id="todo-list""#), "{out}");
    assert!(out.contains(r#"data-ak-id="todo-list-7""#));
    assert!(out.contains(r#"data-ak-id="todo-list-9""#));
    assert!(out.contains("eggs"));
    assert!(!out.contains("Nothing yet"));

    let empty = html(|| {
        view! {
            <List
                ak_id="todo-list"
                items=Signal::derive(Vec::<Todo>::new)
                key=|t: &Todo| t.id
                row=|t: Todo| t.text
                empty="Nothing yet"
            />
        }
    });
    assert!(empty.contains(r#"data-ak-id="todo-list-empty""#), "{empty}");
    assert!(empty.contains("Nothing yet"));
}

#[test]
fn form_and_card_wrap_their_children() {
    let out = html(|| {
        view! {
            <Card ak_id="add-card" title="Add an item">
                <Form ak_id="add-form" on_submit=|()| {}>
                    <Button ak_id="add-button" submit=true>"Add"</Button>
                </Form>
            </Card>
        }
    });
    let card = out.find(r#"data-ak-id="add-card""#).unwrap();
    let form = out.find(r#"data-ak-id="add-form""#).unwrap();
    let button = out.find(r#"data-ak-id="add-button""#).unwrap();
    assert!(card < form && form < button, "{out}");
    assert!(out.contains("Add an item"));
    assert!(out.contains("<form"));
}

#[test]
fn dialog_renders_only_when_open() {
    let closed = html(|| {
        view! {
            <Dialog ak_id="confirm" open=RwSignal::new(false) title="Delete?" close_label="Close">
                <p>"Sure?"</p>
            </Dialog>
        }
    });
    assert!(!closed.contains("Sure?"), "{closed}");

    let open = html(|| {
        view! {
            <Dialog ak_id="confirm" open=RwSignal::new(true) title="Delete?" close_label="Close">
                <p>"Sure?"</p>
            </Dialog>
        }
    });
    assert!(open.contains(r#"role="dialog""#), "{open}");
    assert!(open.contains(r#"aria-modal="true""#));
    assert!(open.contains(r#"data-ak-id="confirm""#));
    assert!(open.contains(r#"data-ak-id="confirm-close""#));
    assert!(open.contains(r#"data-ak-id="confirm-backdrop""#));
    assert!(open.contains(r#"aria-label="Close""#));
    assert!(open.contains("Sure?"));
}

#[test]
fn nav_marks_the_current_item() {
    let out = html(|| {
        let active = RwSignal::new("done".to_owned());
        view! {
            <Nav
                ak_id="tabs"
                items=vec![NavItem::new("all", "All"), NavItem::new("done", "Done")]
                active=active
            />
        }
    });
    assert!(out.contains(r#"data-ak-id="tabs""#), "{out}");
    assert!(out.contains(r#"data-ak-id="tabs-all""#));
    // Each button's opening tag, by its id.
    let tag = |id: &str| {
        out.split("<button")
            .find(|b| b.contains(&format!(r#"data-ak-id="{id}""#)))
            .and_then(|b| b.split('>').next())
            .unwrap()
            .to_owned()
    };
    assert!(tag("tabs-done").contains(r#"aria-current="page""#), "{out}");
    assert!(tag("tabs-done").contains("bg-blue-600"));
    assert!(!tag("tabs-all").contains("aria-current"));
    assert_eq!(out.matches(r#"aria-current="page""#).count(), 1);
}

#[test]
fn toggle_is_a_switch() {
    let out = html(|| {
        view! {
            <Toggle ak_id="dark-mode" checked=RwSignal::new(true) label="Dark mode" />
            <Toggle ak_id="sound" checked=RwSignal::new(false) label="Sound" />
        }
    });
    assert!(out.contains(r#"role="switch""#), "{out}");
    assert!(out.contains(r#"data-ak-id="dark-mode""#));
    assert!(out.contains(r#"aria-checked="true""#));
    assert!(out.contains(r#"aria-checked="false""#));
    assert!(out.contains("Dark mode"));
}

#[test]
fn toasts_are_numbered_parts_of_their_host() {
    let out = html(|| {
        view! {
            <ToastHost ak_id="toasts">
                {move || {
                    let t = use_toaster();
                    t.success("Saved");
                    t.error("Could not sync");
                }}
            </ToastHost>
        }
    });
    assert!(out.contains(r#"data-ak-id="toasts""#), "{out}");
    assert!(out.contains(r#"aria-live="polite""#));
    assert!(out.contains(r#"data-ak-id="toasts-1""#));
    assert!(out.contains(r#"data-ak-id="toasts-2""#));
    assert!(out.contains(r#"role="status""#));
    assert!(out.contains(r#"role="alert""#));
    assert!(out.contains("Could not sync"));
}

#[test]
fn text_props_follow_signals() {
    let out = html(|| {
        let editing = RwSignal::new(true);
        let name = RwSignal::new("photo.jpg".to_string());
        let value = RwSignal::new(String::new());
        let checked = RwSignal::new(false);
        view! {
            <Input
                ak_id="name-input"
                value=value
                label=Signal::derive(move || if editing.get() { "Rename" } else { "Name" }.to_string())
                placeholder=move || name.get()
            />
            <Card ak_id="info-card" title=move || format!("About {}", name.get())>
                <p>"x"</p>
            </Card>
            <Toggle ak_id="keep-toggle" checked=checked label=move || format!("Keep {}", name.get()) />
            <Dialog ak_id="rename" open=RwSignal::new(true) title=move || format!("Rename {}", name.get()) close_label="Close">
                <p>"y"</p>
            </Dialog>
        }
    });
    for text in [
        "Rename",
        "placeholder=\"photo.jpg\"",
        "About photo.jpg",
        "Keep photo.jpg",
        "Rename photo.jpg",
    ] {
        assert!(out.contains(text), "{text} in {out}");
    }
}

#[test]
fn class_adds_to_each_components_own() {
    let out = html(|| {
        let open = RwSignal::new(true);
        view! {
            <Button ak_id="start" class="w-full">"Start"</Button>
            <Card ak_id="score" class="bg-slate-800!">"1"</Card>
            <Input ak_id="name" value=RwSignal::new(String::new()) class="text-lg" />
            <List ak_id="rows" items=RwSignal::new(vec![1u32]) key=|n: &u32| *n row=|n: u32| n class="divide-slate-700!" />
            <Toggle ak_id="dark" checked=RwSignal::new(false) label="Dark" class="mt-2" />
            <Nav ak_id="tabs" items=vec![NavItem::new("a", "A")] active=RwSignal::new("a".to_owned()) class="mb-4" />
            <Dialog ak_id="wide" open=open title="Wide" close_label="Close" class="max-w-2xl!">"x"</Dialog>
        }
    });
    for (own, extra) in [
        ("bg-blue-600", "w-full"),
        ("rounded-xl", "bg-slate-800!"),
        ("rounded-lg", "text-lg"),
        ("divide-y", "divide-slate-700!"),
        ("inline-flex", "mt-2"),
        ("max-w-md", "max-w-2xl!"),
    ] {
        assert!(out.contains(&format!(" {extra}\"")), "{extra}: {out}");
        assert!(out.contains(own), "{own}");
    }
    assert!(out.contains(r#"class="ak-nav mb-4""#), "{out}");
}

#[test]
fn every_part_has_a_marker_class() {
    let out = html(|| {
        let open = RwSignal::new(true);
        view! {
            <ToastHost ak_id="toasts">
                <Button ak_id="a">"A"</Button>
                <Button ak_id="b" variant=ButtonVariant::Ghost>"B"</Button>
                <Card ak_id="c" title="Card">"c"</Card>
                <Input ak_id="i" value=RwSignal::new(String::new()) label="Name" />
                <List ak_id="l" items=RwSignal::new(vec![1u32]) key=|n: &u32| *n row=|n: u32| n />
                <Toggle ak_id="t" checked=RwSignal::new(true) label="On" />
                <Nav ak_id="n" items=vec![NavItem::new("x", "X")] active=RwSignal::new("x".to_owned()) />
                <Dialog ak_id="d" open=open title="D" close_label="Close">"d"</Dialog>
            </ToastHost>
        }
    });
    for marker in [
        "ak-button ak-button-primary",
        "ak-button ak-button-ghost",
        "ak-card ",
        "ak-card-title",
        "ak-input ",
        "ak-input-label",
        "ak-list ",
        "ak-list-row",
        "ak-toggle ",
        "ak-toggle-switch",
        "ak-toggle-knob",
        "ak-nav\"",
        "ak-nav-tab",
        "ak-dialog ",
        "ak-dialog-backdrop",
        "ak-dialog-title",
        "ak-dialog-close",
        "ak-toasts",
    ] {
        assert!(out.contains(marker), "{marker}: {out}");
    }
}
