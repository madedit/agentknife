//! Fixed UI text through fluent (`ak_types::i18n`), as a Leptos context.

use std::sync::Arc;

use ak_types::i18n::{I18n, Locale};
use leptos::prelude::*;

/// The UI's localizer; replaced as a whole when the language changes.
#[derive(Clone, Copy)]
pub struct Tr(pub RwSignal<Arc<I18n>>);

/// Makes `locale` the UI language for everything below.
pub fn provide(locale: Locale) -> Tr {
    let tr = Tr(RwSignal::new(Arc::new(localizer(locale))));
    provide_context(tr);
    tr
}

fn localizer(locale: Locale) -> I18n {
    I18n::new(locale)
        .or_else(|_| I18n::new(Locale::FALLBACK))
        // INVARIANT: the embedded English files parse (ak-types tests, xtask i18n).
        .expect("embedded English messages load")
}

/// A fixed text by fluent id, following language changes.
pub fn t(id: &'static str) -> impl Fn() -> String + Copy + Send + Sync + 'static {
    let Tr(i18n) = expect_context::<Tr>();
    move || i18n.with(|i| i.t(id))
}

/// The UI language: `?lang=` (for checking translations in a browser), then the
/// browser's languages. AgentKnife's settings take over once the app passes them.
pub fn browser_locale() -> Locale {
    #[cfg(target_arch = "wasm32")]
    {
        let Some(window) = web_sys::window() else {
            return Locale::FALLBACK;
        };
        let forced = window
            .location()
            .search()
            .ok()
            .and_then(|q| {
                q.trim_start_matches('?')
                    .split('&')
                    .find_map(|kv| kv.strip_prefix("lang=").map(str::to_owned))
            })
            .and_then(|tag| Locale::from_tag(&tag));
        let languages: Vec<String> = window
            .navigator()
            .languages()
            .iter()
            .filter_map(|v| v.as_string())
            .collect();
        Locale::resolve(forced, &languages)
    }
    #[cfg(not(target_arch = "wasm32"))]
    Locale::resolve::<&str>(None, &[])
}
