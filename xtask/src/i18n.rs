//! Fluent locale parity (AGENTS.md §5): every locale has the same files, and every
//! file defines the same messages, terms and attributes. Every file must also be
//! embedded by `ak-types` (`include_str!`), or it would silently never load.
//! AgentKnife's UI (`ak-shell`) must not write user-visible text in its views.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use fluent_syntax::ast::Entry;

use crate::Result;

const LOCALES_DIR: &str = "crates/ak-types/locales";
const EMBEDDING_SOURCE: &str = "crates/ak-types/src/i18n.rs";
const REFERENCE_LOCALE: &str = "en";
const SHELL_SOURCES: &str = "crates/ak-shell/src";

/// locale -> file name -> source text
type Tree = BTreeMap<String, BTreeMap<String, String>>;

pub fn check(root: &Path) -> Result<()> {
    let tree = load(&root.join(LOCALES_DIR))?;
    let embedding = std::fs::read_to_string(root.join(EMBEDDING_SOURCE))?;
    let mut errors = violations(&tree, &embedding);
    errors.extend(shell_text(&root.join(SHELL_SOURCES), root)?);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n").into())
    }
}

fn load(dir: &Path) -> Result<Tree> {
    let mut tree = Tree::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        let locale = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let files = tree.entry(locale).or_default();
        for file in std::fs::read_dir(&path)? {
            let file = file?.path();
            if file.extension().is_some_and(|e| e == "ftl") {
                let name = file
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                files.insert(name, std::fs::read_to_string(&file)?);
            }
        }
    }
    Ok(tree)
}

/// Message ids, `-term` ids and `id.attribute` keys defined in a Fluent source.
fn keys(source: &str) -> std::result::Result<BTreeSet<String>, String> {
    let resource = fluent_syntax::parser::parse(source).map_err(|(_, errs)| {
        errs.iter()
            .map(|e| format!("{:?} at {:?}", e.kind, e.pos))
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    let mut out = BTreeSet::new();
    for entry in resource.body {
        let (id, attrs) = match entry {
            Entry::Message(m) => (m.id.name.to_owned(), m.attributes),
            Entry::Term(t) => (format!("-{}", t.id.name), t.attributes),
            _ => continue,
        };
        for a in attrs {
            out.insert(format!("{id}.{}", a.id.name));
        }
        out.insert(id);
    }
    Ok(out)
}

fn violations(tree: &Tree, embedding: &str) -> Vec<String> {
    let mut errors = Vec::new();
    let Some(reference) = tree.get(REFERENCE_LOCALE) else {
        return vec![format!(
            "{LOCALES_DIR}: missing reference locale `{REFERENCE_LOCALE}`"
        )];
    };
    for (locale, files) in tree {
        for name in reference.keys().filter(|n| !files.contains_key(*n)) {
            errors.push(format!("{locale}: missing file {name}"));
        }
        for name in files.keys().filter(|n| !reference.contains_key(*n)) {
            errors.push(format!(
                "{locale}: {name} has no {REFERENCE_LOCALE} counterpart"
            ));
        }
        for name in files.keys() {
            let needle = format!("locales/{locale}/{name}");
            if !embedding.contains(&needle) {
                errors.push(format!("{needle}: not embedded in {EMBEDDING_SOURCE}"));
            }
        }
    }
    for (name, ref_src) in reference {
        let ref_keys = match keys(ref_src) {
            Ok(k) => k,
            Err(e) => {
                errors.push(format!("{REFERENCE_LOCALE}/{name}: {e}"));
                continue;
            }
        };
        for (locale, files) in tree.iter().filter(|(l, _)| *l != REFERENCE_LOCALE) {
            let Some(src) = files.get(name) else { continue };
            match keys(src) {
                Ok(k) => {
                    for missing in ref_keys.difference(&k) {
                        errors.push(format!("{locale}/{name}: missing `{missing}`"));
                    }
                    for extra in k.difference(&ref_keys) {
                        errors.push(format!(
                            "{locale}/{name}: `{extra}` not in {REFERENCE_LOCALE}"
                        ));
                    }
                }
                Err(e) => errors.push(format!("{locale}/{name}: {e}")),
            }
        }
    }
    errors
}

/// Text literals in ak-shell views that look like words for the user: a line
/// that is only a string literal, or a literal between `>` and `<`, holding an
/// uppercase or non-ASCII letter (Tailwind classes and data values are
/// lowercase ASCII). Test modules are skipped.
fn shell_text(dir: &Path, root: &Path) -> Result<Vec<String>> {
    let mut errors = Vec::new();
    let mut files = Vec::new();
    rust_files(dir, &mut files)?;
    for file in files {
        let source = std::fs::read_to_string(&file)?;
        let rel = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .display()
            .to_string();
        for (line, text) in literal_text(&source) {
            errors.push(format!(
                "{rel}:{line}: `{text}` is UI text; use a fluent message (i18n::t)"
            ));
        }
    }
    Ok(errors)
}

fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            rust_files(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

fn literal_text(source: &str) -> Vec<(usize, String)> {
    let alone = regex::Regex::new(r#"^\s*"((?:[^"\\]|\\.)*)"\s*$"#);
    let between = regex::Regex::new(r#">\s*"((?:[^"\\]|\\.)*)"\s*<"#);
    let (Ok(alone), Ok(between)) = (alone, between) else {
        return Vec::new();
    };
    let wordy = |t: &str| {
        t.chars()
            .any(|c| c.is_uppercase() || (c.is_alphabetic() && !c.is_ascii()))
    };
    let mut out = Vec::new();
    for (n, line) in source.lines().enumerate() {
        if line.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        if line.trim_start().starts_with("//") {
            continue;
        }
        for re in [&alone, &between] {
            for cap in re.captures_iter(line) {
                let text = &cap[1];
                if wordy(text) {
                    out.push((n + 1, text.to_owned()));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_ui_text_in_views_is_found() {
        let src = r#"
view! {
    <p class="text-sm text-gray-500">
        "Hello there"
    </p>
    <span>"待辦"</span>
    <b>{t("shell-files")}</b>
    <div class=move || {
        if on() {
            "rounded-md bg-white px-3 dark:hover:text-white"
        } else {
            "rounded-md px-3"
        }
    }></div>
}
// "Comments Are Fine"
#[cfg(test)]
mod tests { const X: &str = "Tests Are Fine"; }
"#;
        let found: Vec<_> = literal_text(src).into_iter().map(|(_, t)| t).collect();
        assert_eq!(found, ["Hello there", "待辦"]);
    }

    const EMBED: &str =
        r#"include_str!("../locales/en/ak.ftl") include_str!("../locales/zh-TW/ak.ftl")"#;

    fn tree(en: &str, zh: &str) -> Tree {
        let mut t = Tree::new();
        t.entry("en".into())
            .or_default()
            .insert("ak.ftl".into(), en.into());
        t.entry("zh-TW".into())
            .or_default()
            .insert("ak.ftl".into(), zh.into());
        t
    }

    #[test]
    fn matching_locales_pass() {
        let t = tree(
            "a = A\n-brand = B\nb = B\n    .title = T\n",
            "a = 甲\n-brand = 乙\nb = 乙\n    .title = 丙\n",
        );
        assert_eq!(violations(&t, EMBED), Vec::<String>::new());
    }

    #[test]
    fn missing_message_fails() {
        let t = tree("a = A\nb = B\n", "a = 甲\n");
        assert_eq!(violations(&t, EMBED).len(), 1);
    }

    #[test]
    fn extra_message_and_attribute_fail() {
        let t = tree("a = A\n", "a = 甲\n    .title = T\nz = 乙\n");
        assert_eq!(violations(&t, EMBED).len(), 2);
    }

    #[test]
    fn syntax_error_fails() {
        let t = tree("a = A\n", "a = {\n");
        assert!(!violations(&t, EMBED).is_empty());
    }

    #[test]
    fn missing_file_and_unembedded_file_fail() {
        let mut t = tree("a = A\n", "a = 甲\n");
        t.entry("en".into())
            .or_default()
            .insert("extra.ftl".into(), "x = X\n".into());
        // zh-TW lacks extra.ftl, and en/extra.ftl is not embedded.
        assert_eq!(violations(&t, EMBED).len(), 2);
    }
}
