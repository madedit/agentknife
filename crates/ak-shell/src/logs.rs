//! The Log tab: everything the open project's programs printed (the screens'
//! build, the app's background part, the real window, packaging, the AI's
//! steps), to look at or copy when something goes wrong.

use ak_types::log::{LogLine, LogSource};
use leptos::prelude::*;

use crate::backend::Backend;
use crate::chat::tools::copy;
use crate::i18n::t;

/// Lines the tab keeps (the app keeps as many).
const MAX_LINES: usize = 5_000;

/// The open project's log, collected from the moment the app starts: lines
/// arrive while the tab is hidden too.
#[derive(Clone, Copy)]
pub struct ProjectLog {
    pub lines: RwSignal<Vec<LogLine>>,
}

impl ProjectLog {
    pub fn new() -> ProjectLog {
        ProjectLog {
            lines: RwSignal::new(Vec::new()),
        }
    }

    /// Adds new lines, skipping those already shown (a batch may overlap the
    /// lines fetched when the tab opened).
    pub fn append(&self, batch: Vec<LogLine>) {
        self.lines.update(|lines| {
            let last = lines.last().map_or(0, |l| l.seq);
            lines.extend(batch.into_iter().filter(|l| l.seq > last));
            if lines.len() > MAX_LINES {
                lines.drain(..lines.len() - MAX_LINES);
            }
        });
    }
}

impl Default for ProjectLog {
    fn default() -> Self {
        ProjectLog::new()
    }
}

/// The lines as text, for copying: `[source] line`.
pub fn as_text(lines: &[LogLine]) -> String {
    lines
        .iter()
        .map(|l| format!("[{}] {}", l.source.key(), l.text))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Fluent id of a filter's or a line's source name.
fn source_label(source: Option<LogSource>) -> &'static str {
    match source {
        None => "log-source-all",
        Some(LogSource::Screens) => "log-source-screens",
        Some(LogSource::Backend) => "log-source-backend",
        Some(LogSource::Window) => "log-source-window",
        Some(LogSource::Bundle) => "log-source-bundle",
        Some(LogSource::Ai) => "log-source-ai",
    }
}

const CHIP: &str = "rounded-md px-2.5 py-1 text-sm text-gray-700 hover:bg-gray-100 dark:text-gray-300 dark:hover:bg-gray-800";
const CHIP_ON: &str = "rounded-md bg-blue-600 px-2.5 py-1 text-sm text-white";

#[component]
pub fn LogPane() -> impl IntoView {
    let log = expect_context::<ProjectLog>();
    let backend = expect_context::<Backend>();
    // `None`: every source.
    let filter = RwSignal::new(None::<LogSource>);
    let copied = RwSignal::new(false);
    // Follow new lines while the view is at the bottom.
    let follow = RwSignal::new(true);
    let list = NodeRef::<leptos::html::Div>::new();

    // Everything so far (the app has it all; this tab may open late).
    #[cfg(target_arch = "wasm32")]
    {
        let backend = backend.clone();
        leptos::task::spawn_local(async move {
            if let Ok(lines) = backend.get_log().await {
                log.lines.set(lines);
            }
        });
    }

    let shown = Memo::new(move |_| {
        let f = filter.get();
        log.lines.with(|lines| {
            lines
                .iter()
                .filter(|l| f.is_none_or(|s| l.source == s))
                .cloned()
                .collect::<Vec<_>>()
        })
    });

    #[cfg(target_arch = "wasm32")]
    Effect::new(move |_| {
        shown.track();
        if follow.get_untracked()
            && let Some(el) = list.get_untracked()
        {
            // After this update is drawn.
            let el = el.clone();
            leptos::task::spawn_local(async move {
                crate::backend::sleep(0).await;
                el.set_scroll_top(el.scroll_height());
            });
        }
    });
    let on_scroll = move |_| {
        if let Some(el) = list.get_untracked() {
            follow.set(el.scroll_top() + el.client_height() >= el.scroll_height() - 24);
        }
    };

    let chip = move |which: Option<LogSource>| {
        let on = move || filter.get() == which;
        let key = which.map_or("all", LogSource::key);
        view! {
            <button
                type="button"
                data-log-filter=key
                aria-pressed=move || on().to_string()
                class=move || if on() { CHIP_ON } else { CHIP }
                on:click=move |_| filter.set(which)
            >
                {t(source_label(which))}
            </button>
        }
    };
    let chips = std::iter::once(None)
        .chain(LogSource::ALL.map(Some))
        .map(chip)
        .collect_view();

    let (copy_label, copied_label) = (t("log-copy"), t("log-copied"));
    let copy_all = move |_| {
        copy(&shown.with_untracked(|l| as_text(l)));
        copied.set(true);
        leptos::task::spawn_local(async move {
            crate::backend::sleep(2_000).await;
            copied.set(false);
        });
    };
    let clear = move |_| {
        let backend = backend.clone();
        log.lines.set(Vec::new());
        leptos::task::spawn_local(async move {
            let _ = backend.clear_log().await;
        });
    };

    let row = move |l: LogLine| {
        let class = if l.is_error() {
            "whitespace-pre-wrap break-all text-red-700 dark:text-red-400"
        } else {
            "whitespace-pre-wrap break-all"
        };
        view! {
            <div data-log-line=l.seq data-source=l.source.key() class="flex gap-2 px-3">
                <span class="w-28 shrink-0 truncate text-gray-400 select-none dark:text-gray-500">
                    {t(source_label(Some(l.source)))}
                </span>
                <span class=class>{l.text}</span>
            </div>
        }
    };

    view! {
        <div data-pane="log" class="flex min-h-0 flex-1 flex-col bg-white dark:bg-gray-950">
            <div class="flex shrink-0 flex-wrap items-center gap-1 border-b border-gray-200 px-2 py-1 dark:border-gray-800">
                <div role="group" aria-label=t("log-sources") class="flex flex-wrap gap-0.5">{chips}</div>
                <span class="flex-1"></span>
                <button type="button" data-action="copy-log" class=CHIP on:click=copy_all>
                    {move || if copied.get() { copied_label() } else { copy_label() }}
                </button>
                <button type="button" data-action="clear-log" class=CHIP on:click=clear>
                    {t("log-clear")}
                </button>
            </div>
            <div
                node_ref=list
                data-log="lines"
                class="min-h-0 flex-1 overflow-auto py-2 font-mono text-xs leading-5 text-gray-800 dark:text-gray-200"
                on:scroll=on_scroll
            >
                <Show
                    when=move || shown.with(|l| !l.is_empty())
                    fallback=|| view! {
                        <p data-log-empty="" class="px-3 py-6 text-center font-sans text-sm text-gray-500 dark:text-gray-400">
                            {t("log-empty")}
                        </p>
                    }
                >
                    <For each=move || shown.get() key=|l| l.seq children=row />
                </Show>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(seq: u64, source: LogSource, text: &str) -> LogLine {
        LogLine {
            seq,
            at_ms: 0,
            source,
            text: text.into(),
        }
    }

    #[test]
    fn appended_batches_skip_lines_already_there() {
        let owner = Owner::new();
        owner.with(|| {
            let log = ProjectLog::new();
            log.append(vec![
                line(1, LogSource::Screens, "a"),
                line(2, LogSource::Ai, "b"),
            ]);
            log.append(vec![
                line(2, LogSource::Ai, "b"),
                line(3, LogSource::Window, "c"),
            ]);
            let seqs: Vec<u64> = log
                .lines
                .with_untracked(|l| l.iter().map(|l| l.seq).collect());
            assert_eq!(seqs, [1, 2, 3]);
        });
    }

    #[test]
    fn copied_text_names_the_source() {
        assert_eq!(
            as_text(&[
                line(1, LogSource::Backend, "Compiling app"),
                line(2, LogSource::Ai, "> Reading main.rs")
            ]),
            "[backend] Compiling app\n[ai] > Reading main.rs"
        );
    }
}
