//! The data-ak-id pass on sample files (tests/fixtures/ids/*.input.rs against
//! *.expected.rs; `AK_BLESS=1` rewrites the expected files after a deliberate
//! change), and on the real template and docs, which already follow the
//! convention and must stay unchanged.

// Test code: clippy's allow-unwrap-in-tests does not cover helper fns here.
#![allow(clippy::unwrap_used)]

use std::path::Path;

use ak_preview::ids::add_missing_ids;
use ak_preview::profiles;

#[test]
fn sample_files_match_their_expected_output() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ids");
    let mut seen = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".input.rs") else {
            continue;
        };
        seen += 1;
        let input = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\r\n", "\n");
        let (out, added) = add_missing_ids(&input, stem).unwrap();
        assert!(!added.is_empty(), "{name}: nothing added");
        let expected_path = dir.join(format!("{stem}.expected.rs"));
        if std::env::var("AK_BLESS").is_ok() {
            std::fs::write(&expected_path, &out).unwrap();
        }
        let expected = std::fs::read_to_string(&expected_path)
            .unwrap_or_else(|_| panic!("{stem}.expected.rs missing (run with AK_BLESS=1)"))
            .replace("\r\n", "\n");
        assert_eq!(out, expected, "{name}");
        // Still valid Rust, and idempotent.
        syn::parse_file(&out).unwrap();
        let (again, more) = add_missing_ids(&out, stem).unwrap();
        assert_eq!(again, out, "{name}: second run changed it");
        assert!(more.is_empty());
    }
    assert!(seen >= 2);
}

#[test]
fn the_template_and_the_worked_example_already_follow_the_convention() {
    let profile = profiles::default_profile();
    let template = profile.template();
    let main = template
        .files
        .iter()
        .find(|f| f.path == "src/main.rs")
        .unwrap();
    let (_, added) = add_missing_ids(std::str::from_utf8(main.contents).unwrap(), "main").unwrap();
    assert!(
        added.is_empty(),
        "template src/main.rs lacks ids: {added:?}"
    );

    let example = profile.knowledge_pack().topic("example-todo").unwrap().body;
    let screens = example
        .split("```rust\n")
        .nth(2)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    let (_, added) = add_missing_ids(screens, "main").unwrap();
    assert!(added.is_empty(), "example-todo screens lack ids: {added:?}");
}

#[test]
fn the_stack_profile_runs_the_pass_on_screen_code_only() {
    let profile = profiles::default_profile();
    let src = "fn A() -> impl IntoView { view! { <button>\"Go\"</button> } }\n";
    let changed = profile.add_missing_ids("src/screens/home.rs", src).unwrap();
    assert!(
        changed.contains("<button data-ak-id=\"go-button\">"),
        "{changed}"
    );
    assert!(
        profile
            .add_missing_ids("src-tauri/src/main.rs", src)
            .is_none()
    );
    assert!(profile.add_missing_ids("README.md", src).is_none());
    // Nothing missing: no change reported.
    assert!(profile.add_missing_ids("src/main.rs", &changed).is_none());
    // A file that does not parse (mid-edit) is left alone.
    assert!(profile.add_missing_ids("src/main.rs", "fn x( {").is_none());
}
