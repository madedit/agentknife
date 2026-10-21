//! Every button says what it does when the mouse rests on it: each `<button`
//! in the shell's views has a `title`.

use std::path::{Path, PathBuf};

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("src folder") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The opening tags of the buttons in `code` (skipping comments), with their
/// line numbers. A tag ends at the first `>` outside braces and parentheses
/// that is not part of `=>` or `->`.
fn button_tags(code: &str) -> Vec<(usize, &str)> {
    let mut tags = Vec::new();
    let mut from = 0;
    while let Some(at) = code[from..].find("<button").map(|i| from + i) {
        let line_start = code[..at].rfind('\n').map_or(0, |i| i + 1);
        let comment = code[line_start..at].trim_start().starts_with("//");
        let bytes = code.as_bytes();
        let (mut end, mut depth) = (at + "<button".len(), 0i32);
        while end < bytes.len() {
            match bytes[end] {
                b'{' | b'(' => depth += 1,
                b'}' | b')' => depth -= 1,
                b'>' if depth == 0 && !matches!(bytes[end - 1], b'=' | b'-') => break,
                _ => {}
            }
            end += 1;
        }
        if !comment {
            tags.push((code[..at].matches('\n').count() + 1, &code[at..end]));
        }
        from = end;
    }
    tags
}

#[test]
fn every_button_has_a_tooltip() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&src, &mut files);
    let mut missing = Vec::new();
    let mut seen = 0;
    for file in files {
        // The demo page stands in for the user's app, not AgentKnife's own UI.
        if file.file_name().is_some_and(|n| n == "demo.rs") {
            continue;
        }
        let code = std::fs::read_to_string(&file).expect("source");
        for (line, tag) in button_tags(&code) {
            seen += 1;
            if !tag.contains("title=") {
                missing.push(format!("{}:{line}", file.display()));
            }
        }
    }
    assert!(seen > 50, "found only {seen} buttons; the scan is broken");
    assert!(
        missing.is_empty(),
        "buttons without a title (what it does, on hover):\n{}",
        missing.join("\n")
    );
}

#[test]
fn tags_end_at_their_own_angle_bracket() {
    let code = "// <button>\n<button class=move || if x { A } else { B } on:click=|_| a => b>\n<button\n  title=t(\"x\")\n>";
    let tags = button_tags(code);
    assert_eq!(tags.len(), 2);
    assert_eq!(tags[0].0, 2);
    assert!(!tags[0].1.contains("title="));
    assert!(tags[1].1.contains("title="));
}
