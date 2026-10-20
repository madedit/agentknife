//! Token and cost tracking (AGENTS.md §4 "Usage").
//!
//! Costs come only from a dated price table for models on their official
//! endpoints. Local and compatible servers are free; anything else is "unknown"
//! and never guessed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ak_types::agent::{TokenUsage, UsageSummary};
use serde::{Deserialize, Serialize};

/// US dollars per million tokens.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    /// Different prices once the prompt exceeds this many tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub long_context: Option<(u64, PriceTier)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PriceTier {
    pub input: f64,
    pub output: f64,
}

impl Price {
    /// Cache reads at 0.1x and cache writes at 1.25x input (prompt-caching rates).
    pub const fn standard(input: f64, output: f64) -> Price {
        Price {
            input,
            output,
            cache_read: input * 0.1,
            cache_write: input * 1.25,
            long_context: None,
        }
    }

    pub fn cost(&self, u: &TokenUsage) -> f64 {
        let prompt = u.input_tokens + u.cache_read_tokens + u.cache_write_tokens;
        let (input, output) = match self.long_context {
            Some((threshold, tier)) if prompt > threshold => (tier.input, tier.output),
            _ => (self.input, self.output),
        };
        (u.input_tokens as f64 * input
            + u.output_tokens as f64 * output
            + u.cache_read_tokens as f64 * self.cache_read
            + u.cache_write_tokens as f64 * self.cache_write)
            / 1_000_000.0
    }
}

/// How a session's cost is known.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CostBasis {
    Priced(Price),
    /// A model running on the user's own computer.
    Free,
    /// A paid service whose prices AgentKnife does not know.
    Unknown,
}

/// Anthropic first-party prices, from the claude-api skill (cached 2026-10-06).
/// Update together with that source; never extrapolate to unlisted models.
pub const ANTHROPIC_PRICES_AS_OF: &str = "2026-10-06";

fn anthropic_price(model: &str) -> Option<Price> {
    let p = match model {
        "claude-fable-5-1" => Price {
            cache_read: 0.25,
            ..Price::standard(10.0, 50.0)
        },
        "claude-fable-5" => Price {
            cache_read: 1.0,
            ..Price::standard(10.0, 50.0)
        },
        "claude-opus-5-5" => Price {
            cache_read: 0.20,
            ..Price::standard(4.0, 20.0)
        },
        "claude-opus-5" | "claude-opus-4-8" | "claude-opus-4-7" | "claude-opus-4-6" => {
            Price::standard(5.0, 25.0)
        }
        "claude-sonnet-5-5" | "claude-sonnet-5" => Price {
            cache_read: 0.20,
            ..Price::standard(2.0, 10.0)
        },
        "claude-sonnet-4-6" => Price::standard(3.0, 15.0),
        "claude-haiku-5-5" => Price {
            long_context: Some((
                100_000,
                PriceTier {
                    input: 0.50,
                    output: 2.50,
                },
            )),
            ..Price::standard(0.10, 0.50)
        },
        "claude-haiku-4-5" => Price::standard(1.0, 5.0),
        _ => return None,
    };
    Some(p)
}

/// Decides how cost is known for a provider, endpoint and model.
pub fn cost_basis(provider: &str, base_url: Option<&str>, model: &str) -> CostBasis {
    if base_url.is_some_and(is_local) || matches!(provider, "ollama" | "lmstudio") {
        return CostBasis::Free;
    }
    let official_anthropic =
        base_url.is_none_or(|u| u.trim_end_matches('/') == "https://api.anthropic.com");
    match provider {
        "anthropic" if official_anthropic => {
            anthropic_price(model).map_or(CostBasis::Unknown, CostBasis::Priced)
        }
        _ => CostBasis::Unknown,
    }
}

fn is_local(url: &str) -> bool {
    let host = url
        .split_once("://")
        .map_or(url, |(_, r)| r)
        .split(['/', '?'])
        .next()
        .unwrap_or("");
    let host = host.rsplit_once(':').map_or(host, |(h, port)| {
        if port.chars().all(|c| c.is_ascii_digit()) {
            h
        } else {
            host
        }
    });
    matches!(
        host,
        "localhost" | "127.0.0.1" | "[::1]" | "::1" | "0.0.0.0"
    ) || host.ends_with(".localhost")
}

/// Running totals for one span (a session or a day).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Totals {
    pub tokens: TokenUsage,
    pub cost_usd: f64,
    /// Some usage had an unknown price, so `cost_usd` is a lower bound.
    pub unknown: bool,
    /// All usage so far was free (local models).
    pub free: bool,
}

impl Totals {
    pub fn add(&mut self, u: &TokenUsage, basis: CostBasis) {
        let first = self.tokens == TokenUsage::default() && !self.unknown && self.cost_usd == 0.0;
        self.tokens.input_tokens += u.input_tokens;
        self.tokens.output_tokens += u.output_tokens;
        self.tokens.cache_read_tokens += u.cache_read_tokens;
        self.tokens.cache_write_tokens += u.cache_write_tokens;
        match basis {
            CostBasis::Priced(p) => {
                self.cost_usd += p.cost(u);
                self.free = false;
            }
            CostBasis::Free => self.free = first || self.free,
            CostBasis::Unknown => {
                self.unknown = true;
                self.free = false;
            }
        }
    }

    pub fn summary(&self) -> UsageSummary {
        UsageSummary {
            tokens: self.tokens,
            cost_usd: if self.unknown && self.cost_usd == 0.0 {
                None
            } else {
                Some(self.cost_usd)
            },
            cost_is_lower_bound: self.unknown,
            free: self.free,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct DayRecord {
    totals: Totals,
    /// The 80% warning was shown today.
    warned: bool,
    /// The user chose to keep going past the cap today.
    cap_overridden: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct LedgerFile {
    schema_version: u32,
    /// Keyed by local calendar date, `YYYY-MM-DD`.
    days: BTreeMap<String, DayRecord>,
}

/// What `UsageLedger::record` reports about today's spending.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CapStatus {
    Fine,
    /// Crossed 80% of the cap just now (reported once a day).
    Warn {
        spent: f64,
        cap: f64,
    },
}

/// Per-day usage across all projects, persisted in AgentKnife's data folder.
pub struct UsageLedger {
    file: Option<PathBuf>,
    inner: Mutex<LedgerFile>,
}

/// Rounding slack for cap comparisons (sums of float prices).
const EPSILON_USD: f64 = 1e-9;

/// Days of history kept in the ledger file.
const KEEP_DAYS: usize = 400;

impl UsageLedger {
    /// Not persisted (tests, scratch chats).
    pub fn in_memory() -> Self {
        Self {
            file: None,
            inner: Mutex::new(LedgerFile::default()),
        }
    }

    /// Loads `usage.json` from `state_dir`; a damaged file starts a new ledger.
    pub fn load(state_dir: &Path) -> Self {
        let file = state_dir.join("usage.json");
        let data = std::fs::read_to_string(&file)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Self {
            file: Some(file),
            inner: Mutex::new(data),
        }
    }

    /// Today's local calendar date.
    pub fn today() -> String {
        chrono::Local::now().format("%Y-%m-%d").to_string()
    }

    pub fn day(&self, day: &str) -> Totals {
        self.inner
            .lock()
            .ok()
            .and_then(|l| l.days.get(day).map(|d| d.totals))
            .unwrap_or_default()
    }

    /// Adds usage to `day` and reports whether the 80% warning is due.
    pub fn record(
        &self,
        day: &str,
        u: &TokenUsage,
        basis: CostBasis,
        cap: Option<f64>,
    ) -> CapStatus {
        let status = {
            let Ok(mut l) = self.inner.lock() else {
                return CapStatus::Fine;
            };
            let rec = l.days.entry(day.to_owned()).or_default();
            rec.totals.add(u, basis);
            let spent = rec.totals.cost_usd;
            let status = match cap {
                Some(cap) if cap > 0.0 && !rec.warned && spent + EPSILON_USD >= 0.8 * cap => {
                    rec.warned = true;
                    CapStatus::Warn { spent, cap }
                }
                _ => CapStatus::Fine,
            };
            while l.days.len() > KEEP_DAYS {
                let Some(oldest) = l.days.keys().next().cloned() else {
                    break;
                };
                l.days.remove(&oldest);
            }
            status
        };
        self.save();
        status
    }

    /// Whether `day` has reached the cap and the user has not chosen to go on.
    pub fn over_cap(&self, day: &str, cap: Option<f64>) -> Option<f64> {
        let cap = cap.filter(|c| *c > 0.0)?;
        let l = self.inner.lock().ok()?;
        let rec = l.days.get(day)?;
        (rec.totals.cost_usd + EPSILON_USD >= cap && !rec.cap_overridden)
            .then_some(rec.totals.cost_usd)
    }

    /// The user chose to keep going today.
    pub fn override_cap(&self, day: &str) {
        if let Ok(mut l) = self.inner.lock() {
            l.days.entry(day.to_owned()).or_default().cap_overridden = true;
        }
        self.save();
    }

    fn save(&self) {
        let Some(file) = &self.file else { return };
        let json = match self.inner.lock() {
            Ok(mut l) => {
                l.schema_version = 1;
                serde_json::to_string_pretty(&*l).ok()
            }
            Err(_) => None,
        };
        if let Some(json) = json {
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let tmp = file.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() && std::fs::rename(&tmp, file).is_err() {
                tracing::warn!("could not save the usage ledger");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(i: u64, o: u64, cr: u64, cw: u64) -> TokenUsage {
        TokenUsage {
            input_tokens: i,
            output_tokens: o,
            cache_read_tokens: cr,
            cache_write_tokens: cw,
        }
    }

    #[test]
    fn prices_and_bases() {
        let CostBasis::Priced(opus) = cost_basis("anthropic", None, "claude-opus-5-5") else {
            panic!()
        };
        // 1M in + 1M out on Opus 5.5 = $4 + $20; cache read 1M = $0.20.
        assert!((opus.cost(&usage(1_000_000, 1_000_000, 0, 0)) - 24.0).abs() < 1e-9);
        assert!((opus.cost(&usage(0, 0, 1_000_000, 0)) - 0.20).abs() < 1e-9);
        let CostBasis::Priced(haiku) = cost_basis("anthropic", None, "claude-haiku-5-5") else {
            panic!()
        };
        assert!(
            (haiku.cost(&usage(100_000, 0, 0, 0)) - 0.01).abs() < 1e-9,
            "<=100K tier"
        );
        assert!(
            (haiku.cost(&usage(200_000, 0, 0, 0)) - 0.10).abs() < 1e-9,
            ">100K tier"
        );

        assert_eq!(
            cost_basis("anthropic", None, "claude-9-future"),
            CostBasis::Unknown
        );
        assert_eq!(
            cost_basis("anthropic", Some("http://127.0.0.1:8080"), "qwen3"),
            CostBasis::Free
        );
        assert_eq!(
            cost_basis("openai-compatible", Some("http://localhost:1234/v1"), "x"),
            CostBasis::Free
        );
        assert_eq!(
            cost_basis("ollama", Some("https://ollama.com/v1"), "x"),
            CostBasis::Free
        );
        assert_eq!(
            cost_basis("openai", Some("https://api.openai.com/v1"), "gpt-test"),
            CostBasis::Unknown
        );
        assert_eq!(
            cost_basis(
                "anthropic",
                Some("https://proxy.example.com"),
                "claude-opus-5-5"
            ),
            CostBasis::Unknown,
            "a proxy may bill differently"
        );
    }

    #[test]
    fn ledger_days_warning_cap_and_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let price = CostBasis::Priced(Price::standard(1.0, 1.0)); // $1 per 1M tokens
        let ledger = UsageLedger::load(dir.path());
        let cap = Some(1.0);
        assert_eq!(
            ledger.record("2026-10-20", &usage(500_000, 0, 0, 0), price, cap),
            CapStatus::Fine
        );
        assert!(matches!(
            ledger.record("2026-10-20", &usage(300_000, 0, 0, 0), price, cap),
            CapStatus::Warn { .. }
        ));
        assert_eq!(
            ledger.record("2026-10-20", &usage(10_000, 0, 0, 0), price, cap),
            CapStatus::Fine,
            "warned once a day"
        );
        assert_eq!(ledger.over_cap("2026-10-20", cap), None);
        ledger.record("2026-10-20", &usage(200_000, 0, 0, 0), price, cap);
        assert!(ledger.over_cap("2026-10-20", cap).is_some());
        ledger.override_cap("2026-10-20");
        assert_eq!(
            ledger.over_cap("2026-10-20", cap),
            None,
            "user chose to go on"
        );
        assert_eq!(
            ledger.over_cap("2026-10-21", cap),
            None,
            "a new day starts fresh"
        );

        let reloaded = UsageLedger::load(dir.path());
        assert!((reloaded.day("2026-10-20").cost_usd - 1.01).abs() < 1e-9);
        assert_eq!(reloaded.day("2026-10-20").tokens.input_tokens, 1_010_000);
    }

    #[test]
    fn totals_track_free_and_unknown() {
        let mut t = Totals::default();
        t.add(&usage(10, 5, 0, 0), CostBasis::Free);
        assert!(t.summary().free);
        assert_eq!(t.summary().cost_usd, Some(0.0));
        t.add(&usage(10, 5, 0, 0), CostBasis::Unknown);
        let s = t.summary();
        assert!(!s.free && s.cost_is_lower_bound && s.cost_usd.is_none());
    }
}
