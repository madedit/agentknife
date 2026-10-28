//! The files, version-history and code panes for a given state, rendered to
//! HTML natively. The editor itself (CodeMirror) runs in a browser only.

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]
// Leptos views nest deeply in types.
#![recursion_limit = "256"]

use std::collections::BTreeSet;

use ak_shell::backend::Backend;
use ak_shell::files::{CodePane, Files, FilesPane, OpenFile, TimelinePane};
use ak_shell::i18n;
use ak_shell::layout::Layout;
use ak_types::i18n::Locale;
use ak_types::ipc::{AssetKind, FileEntry, Media, TimelineRow};
use leptos::prelude::*;

fn with(locale: Locale, setup: impl FnOnce(Files), view: impl FnOnce() -> String) -> String {
    let owner = Owner::new();
    owner.with(|| {
        i18n::provide(locale);
        provide_context(Backend::demo());
        let files = Files::new();
        setup(files);
        provide_context(files);
        view()
    })
}

fn entries() -> Vec<FileEntry> {
    [
        "Cargo.toml",
        "src/main.rs",
        "src/screens/home.rs",
        "style/input.css",
    ]
    .into_iter()
    .map(|p| FileEntry {
        path: p.into(),
        dir: false,
    })
    .collect()
}

fn open(path: &str) -> OpenFile {
    OpenFile {
        path: path.into(),
        saved: "fn main() {}".into(),
        binary: false,
        media: None,
        version: 0,
    }
}

#[test]
fn the_tree_shows_folders_first_and_unsaved_files() {
    let html = with(
        Locale::En,
        |f| {
            f.entries.set(entries());
            f.expanded.set(BTreeSet::from(["src".to_owned()]));
            f.open.set(Some(open("src/main.rs")));
            f.drafts.update(|d| {
                d.insert("src/main.rs".into(), "fn main() { 1 }".into());
            });
        },
        || {
            let layout = RwSignal::new(Layout::default());
            view! { <FilesPane layout /> }.into_view().to_html()
        },
    );
    let order: Vec<usize> = [
        "data-file=\"src\"",
        "data-file=\"src/screens\"",
        "data-file=\"src/main.rs\"",
        "data-file=\"style\"",
        "data-file=\"Cargo.toml\"",
    ]
    .iter()
    .map(|needle| {
        html.find(needle)
            .unwrap_or_else(|| panic!("{needle} in {html}"))
    })
    .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
    assert!(
        !html.contains("src/screens/home.rs"),
        "closed folder shows its files"
    );
    assert_eq!(html.matches("data-dirty").count(), 1);
    assert!(html.contains(r#"aria-expanded="true""#));
}

#[test]
fn the_timeline_offers_going_back_and_marking() {
    let row = |id: &str, kind: &str, label: &str, current: bool| TimelineRow {
        id: id.into(),
        kind: kind.into(),
        label: label.into(),
        created_at: 0,
        changed_files: Vec::new(),
        current,
    };
    let html = with(
        Locale::ZhTw,
        |f| {
            f.timeline.set(vec![
                row("t3", "turn", "把按鈕改成藍色", true),
                row("t2", "marked", "第一個能用的版本", false),
                row("t1", "initial", "My App", false),
            ])
        },
        || view! { <TimelinePane /> }.into_view().to_html(),
    );
    assert!(html.contains("目前在這一版") && html.contains("把按鈕改成藍色"));
    // Every older entry can be restored; the current one cannot.
    assert_eq!(html.matches(r#"data-action="restore""#).count(), 2);
    assert!(html.contains(r#"data-action="mark""#) && html.contains("標記這一版"));
    assert!(html.contains(r#"data-entry="marked""#) && html.contains("天前"));
    assert!(!html.contains(">timeline-"), "raw fluent id in {html}");
}

#[test]
fn the_code_pane_shows_the_file_and_its_state() {
    let empty = with(
        Locale::En,
        |_| {},
        || view! { <CodePane /> }.into_view().to_html(),
    );
    assert!(empty.contains("Pick a file on the left"));

    let html = with(
        Locale::En,
        |f| {
            f.open.set(Some(open("src/main.rs")));
            f.drafts.update(|d| {
                d.insert("src/main.rs".into(), "x".into());
            });
            f.changed_outside.set(true);
        },
        || view! { <CodePane /> }.into_view().to_html(),
    );
    assert!(html.contains("src/main.rs") && html.contains(r#"data-dirty"#));
    assert!(
        html.contains(r#"data-action="save""#) && !html.contains(r#"data-action="save" disabled"#)
    );
    assert!(html.contains(r#"data-notice="changed-outside""#));
    assert!(html.contains(r#"data-editor="""#));

    let picture = with(
        Locale::En,
        |f| {
            f.open.set(Some(OpenFile {
                binary: true,
                ..open("icons/icon.png")
            }))
        },
        || view! { <CodePane /> }.into_view().to_html(),
    );
    assert!(picture.contains("cannot be shown or edited here"));

    // Pictures, sounds and fonts show as themselves; models get a note.
    let media = |kind: AssetKind, path: &str| {
        with(
            Locale::En,
            |f| {
                f.open.set(Some(OpenFile {
                    binary: true,
                    media: Some(Media {
                        kind,
                        url: format!("http://akfile.localhost/t/{path}?v=1"),
                        size: 1536,
                    }),
                    ..open(path)
                }))
            },
            || view! { <CodePane /> }.into_view().to_html(),
        )
    };
    let image = media(AssetKind::Image, "assets/images/cat.png");
    assert!(
        image.contains(r#"data-media="image""#)
            && image.contains(r#"src="http://akfile.localhost/t/assets/images/cat.png?v=1""#),
        "{image}"
    );
    assert!(image.contains("2 KB") && !image.contains("cannot be shown"));
    let sound = media(AssetKind::Sound, "assets/sounds/jump.wav");
    assert!(
        sound.contains("<audio") && sound.contains("controls"),
        "{sound}"
    );
    let font = media(AssetKind::Font, "assets/fonts/round.ttf");
    assert!(
        font.contains("@font-face") && font.contains("quick brown fox"),
        "{font}"
    );
    let model = media(AssetKind::Model, "assets/models/plane.glb");
    assert!(
        model.contains(r#"data-media="model""#) && model.contains("<canvas"),
        "{model}"
    );
    assert!(model.contains("Loading the model… 0%"), "{model}");
    let part = media(AssetKind::Model, "assets/models/plane.bin");
    assert!(part.contains(r#"data-media="model-part""#) && !part.contains("<canvas"));
    assert_eq!(
        ak_shell::files::url_extension("http://x/a/Plane.GLB?v=1"),
        "glb"
    );
    assert_eq!(
        ak_shell::files::url_extension("akfile://localhost/t/a/README"),
        ""
    );
}

#[test]
fn files_in_assets_can_be_renamed() {
    let code = |path: &str| {
        with(
            Locale::ZhTw,
            |f| f.open.set(Some(open(path))),
            || view! { <CodePane /> }.into_view().to_html(),
        )
    };
    let asset = code("assets/files/notes.txt");
    assert!(
        asset.contains(r#"data-action="rename-file""#) && asset.contains("改名…"),
        "{asset}"
    );
    assert!(
        !asset.contains(r#"data-rename="""#),
        "the form only after a click"
    );
    let source = code("src/main.rs");
    assert!(
        !source.contains(r#"data-action="rename-file""#),
        "only the user's files"
    );
    assert_eq!(
        ak_shell::files::stem_and_extension("assets/images/my.cat.png"),
        ("my.cat".to_owned(), ".png".to_owned())
    );
    assert_eq!(
        ak_shell::files::stem_and_extension("assets/files/README"),
        ("README".to_owned(), String::new())
    );
}
