//! Fixed-string localization (AGENTS.md §5, "Two-layer i18n").
//!
//! Fluent files live in `crates/ak-types/locales/<locale>/*.ftl` and are embedded at
//! compile time, so the same API works natively and in the WASM UI. `cargo xtask ci`
//! fails if a message exists in one locale but not the other.
//!
//! AI-generated text never goes through this module.

use std::fmt;
use std::str::FromStr;

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentResource, FluentValue};
use serde::{Deserialize, Serialize};
use unic_langid::LanguageIdentifier;

pub use fluent_bundle::FluentArgs;

/// UI locales AgentKnife ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Locale {
    #[serde(rename = "en")]
    En,
    #[serde(rename = "zh-TW")]
    ZhTw,
}

impl Locale {
    pub const ALL: [Locale; 2] = [Locale::En, Locale::ZhTw];
    /// Used when nothing better matches, and for keys missing from a bundle.
    pub const FALLBACK: Locale = Locale::En;

    /// BCP 47 tag, also the directory name under `locales/`.
    pub fn tag(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::ZhTw => "zh-TW",
        }
    }

    /// Picks the best supported locale for a BCP 47 tag (`zh-Hant-TW`, `en_US`, ...).
    /// Every Chinese variant maps to zh-TW: Traditional Chinese is the only Chinese
    /// we ship, and it is more readable to Simplified readers than English.
    pub fn from_tag(tag: &str) -> Option<Locale> {
        let lang = tag.split(['-', '_']).next()?.to_ascii_lowercase();
        match lang.as_str() {
            "en" => Some(Locale::En),
            "zh" => Some(Locale::ZhTw),
            _ => None,
        }
    }

    /// The settings override wins; otherwise the first OS preference we support.
    pub fn resolve<S: AsRef<str>>(overridden: Option<Locale>, os_preferences: &[S]) -> Locale {
        overridden
            .or_else(|| {
                os_preferences
                    .iter()
                    .find_map(|t| Locale::from_tag(t.as_ref()))
            })
            .unwrap_or(Locale::FALLBACK)
    }

    /// OS locale preferences, most preferred first. The WASM UI gets its locale
    /// from `ak-app` instead (settings are the source of truth).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn os_preferences() -> Vec<String> {
        sys_locale::get_locales().collect()
    }

    fn sources(self) -> &'static [&'static str] {
        match self {
            Locale::En => &[include_str!("../locales/en/ak.ftl")],
            Locale::ZhTw => &[include_str!("../locales/zh-TW/ak.ftl")],
        }
    }
}

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.tag())
    }
}

impl FromStr for Locale {
    type Err = I18nError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Locale::ALL
            .into_iter()
            .find(|l| l.tag().eq_ignore_ascii_case(s))
            .ok_or_else(|| I18nError::UnknownLocale(s.to_owned()))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum I18nError {
    #[error("unknown locale `{0}`")]
    UnknownLocale(String),
    #[error("invalid language tag for {0}")]
    BadTag(Locale),
    #[error("{locale}: fluent syntax errors: {details}")]
    Parse { locale: Locale, details: String },
    #[error("{locale}: duplicate message ids: {details}")]
    Duplicate { locale: Locale, details: String },
}

/// Localizer for one UI locale, falling back to English for missing keys.
pub struct I18n {
    locale: Locale,
    primary: FluentBundle<FluentResource>,
    fallback: Option<FluentBundle<FluentResource>>,
}

impl I18n {
    pub fn new(locale: Locale) -> Result<Self, I18nError> {
        let fallback = if locale == Locale::FALLBACK {
            None
        } else {
            Some(bundle(Locale::FALLBACK)?)
        };
        Ok(Self {
            locale,
            primary: bundle(locale)?,
            fallback,
        })
    }

    pub fn locale(&self) -> Locale {
        self.locale
    }

    /// Formats a message without arguments.
    pub fn t(&self, id: &str) -> String {
        self.format(id, None)
    }

    /// Formats a message with arguments.
    pub fn t_args(&self, id: &str, args: &FluentArgs<'_>) -> String {
        self.format(id, Some(args))
    }

    fn format(&self, id: &str, args: Option<&FluentArgs<'_>>) -> String {
        std::iter::once(&self.primary)
            .chain(self.fallback.as_ref())
            .find_map(|b| format_in(b, id, args))
            .unwrap_or_else(|| {
                tracing::warn!(id, locale = %self.locale, "missing i18n message");
                id.to_owned()
            })
    }
}

/// Convenience for building [`FluentArgs`] from `(name, value)` pairs.
pub fn args<'a, V: Into<FluentValue<'a>>>(
    pairs: impl IntoIterator<Item = (&'a str, V)>,
) -> FluentArgs<'a> {
    let mut out = FluentArgs::new();
    for (k, v) in pairs {
        out.set(k, v);
    }
    out
}

fn bundle(locale: Locale) -> Result<FluentBundle<FluentResource>, I18nError> {
    let langid: LanguageIdentifier = locale
        .tag()
        .parse()
        .map_err(|_| I18nError::BadTag(locale))?;
    let mut bundle = FluentBundle::new_concurrent(vec![langid]);
    // Bidi isolation marks only matter for RTL locales, which we do not ship.
    bundle.set_use_isolating(false);
    for source in locale.sources() {
        let resource = FluentResource::try_new((*source).to_owned()).map_err(|(_, errs)| {
            I18nError::Parse {
                locale,
                details: format!("{errs:?}"),
            }
        })?;
        bundle
            .add_resource(resource)
            .map_err(|errs| I18nError::Duplicate {
                locale,
                details: format!("{errs:?}"),
            })?;
    }
    Ok(bundle)
}

fn format_in(
    bundle: &FluentBundle<FluentResource>,
    id: &str,
    args: Option<&FluentArgs<'_>>,
) -> Option<String> {
    let pattern = bundle.get_message(id)?.value()?;
    let mut errors = Vec::new();
    let out = bundle.format_pattern(pattern, args, &mut errors);
    if !errors.is_empty() {
        tracing::warn!(id, ?errors, "i18n formatting errors");
    }
    Some(out.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_locales_load() {
        for locale in Locale::ALL {
            let i18n = I18n::new(locale).unwrap();
            assert_ne!(i18n.t("app-name"), "app-name", "{locale}");
        }
    }

    #[test]
    fn formats_per_locale_with_args() {
        let a = args([
            ("tool", FluentValue::from("Rust")),
            ("step", 3.into()),
            ("total", 5.into()),
        ]);
        let en = I18n::new(Locale::En).unwrap();
        let zh = I18n::new(Locale::ZhTw).unwrap();
        assert_eq!(en.t_args("doctor-installing", &a), "Installing Rust (3/5)…");
        assert_eq!(zh.t_args("doctor-installing", &a), "正在安裝 Rust（3/5）…");
    }

    #[test]
    fn missing_key_returns_id() {
        let zh = I18n::new(Locale::ZhTw).unwrap();
        assert_eq!(zh.t("no-such-key"), "no-such-key");
    }

    #[test]
    fn tag_negotiation() {
        assert_eq!(Locale::from_tag("en-US"), Some(Locale::En));
        assert_eq!(Locale::from_tag("en_GB"), Some(Locale::En));
        assert_eq!(Locale::from_tag("zh-Hant-TW"), Some(Locale::ZhTw));
        assert_eq!(Locale::from_tag("zh-CN"), Some(Locale::ZhTw));
        assert_eq!(Locale::from_tag("ja-JP"), None);
    }

    #[test]
    fn resolve_prefers_override_then_os_then_fallback() {
        assert_eq!(Locale::resolve(Some(Locale::En), &["zh-TW"]), Locale::En);
        assert_eq!(
            Locale::resolve(None, &["ja-JP", "zh-TW", "en"]),
            Locale::ZhTw
        );
        assert_eq!(Locale::resolve::<&str>(None, &[]), Locale::FALLBACK);
    }

    #[test]
    fn parse_and_serde_use_bcp47_tags() {
        assert_eq!("zh-tw".parse::<Locale>().unwrap(), Locale::ZhTw);
        assert!("fr".parse::<Locale>().is_err());
        // Settings files store the BCP 47 tag.
        for locale in Locale::ALL {
            let json = serde_json::to_string(&locale).unwrap();
            assert_eq!(json, format!("\"{}\"", locale.tag()));
            assert_eq!(serde_json::from_str::<Locale>(&json).unwrap(), locale);
        }
    }

    #[test]
    fn localizer_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<I18n>();
    }
}
