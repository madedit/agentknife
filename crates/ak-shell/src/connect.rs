//! The connect wizard: choose an AI, give it a key or an address, test it,
//! pick a model, check what it can do, save.

use ak_types::ipc::{
    ConnectError, ConnectRequest, ModelCheck, ModelChoice, PRESETS, PresetKind, ProviderPreset,
    SavedConnection,
};
use ak_types::settings::AiConnection;
use leptos::prelude::*;

use crate::backend::{Backend, CallError};
use crate::i18n::{t, t_with, tr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Choose,
    Details,
    Model,
    /// Another AI tool (MCP), by its id in `external::TOOLS`.
    External(&'static str),
}

/// The wizard's state, separate from the view so it can be tested.
#[derive(Debug, Clone, PartialEq)]
pub struct Wizard {
    pub step: Step,
    pub preset: Option<&'static ProviderPreset>,
    /// Typed by the user; only sent to the app, never kept after saving.
    pub key: String,
    pub address: String,
    pub models: Vec<ModelChoice>,
    pub model: String,
    pub check: Option<ModelCheck>,
    pub error: Option<CallError>,
    pub busy: bool,
    /// The user chose a model without tool calling on purpose.
    pub accept_no_tools: bool,
    /// What was saved before, per provider (filled in again when chosen).
    pub saved: Vec<SavedConnection>,
    /// The keychain has a key for the chosen provider: it may stay empty.
    pub has_key: bool,
    /// The model saved before for the chosen provider (picked again if listed).
    pub saved_model: Option<String>,
}

impl Default for Wizard {
    fn default() -> Self {
        Wizard {
            step: Step::Choose,
            preset: None,
            key: String::new(),
            address: String::new(),
            models: Vec::new(),
            model: String::new(),
            check: None,
            error: None,
            busy: false,
            accept_no_tools: false,
            saved: Vec::new(),
            has_key: false,
            saved_model: None,
        }
    }
}

impl Wizard {
    pub fn choose(&mut self, preset: &'static ProviderPreset) {
        let saved = std::mem::take(&mut self.saved);
        let before = saved.iter().find(|s| s.connection.provider == preset.id);
        *self = Wizard {
            step: Step::Details,
            preset: Some(preset),
            address: before
                .and_then(|s| s.connection.base_url.clone())
                .unwrap_or_else(|| preset.base_url.unwrap_or_default().to_owned()),
            has_key: before.is_some_and(|s| s.has_key),
            saved_model: before.map(|s| s.connection.model.clone()),
            saved: saved.clone(),
            ..Wizard::default()
        };
    }

    /// What was saved for a provider.
    pub fn saved_for(&self, provider: &str) -> Option<&SavedConnection> {
        self.saved
            .iter()
            .find(|s| s.connection.provider == provider)
    }

    /// The connection in use.
    pub fn current(&self) -> Option<&SavedConnection> {
        self.saved.iter().find(|s| s.current)
    }

    pub fn back(&mut self) {
        self.error = None;
        self.step = match self.step {
            Step::Model => Step::Details,
            Step::External(_) => Step::Choose,
            _ => Step::Choose,
        };
    }

    /// Asks for the address (local servers and other services).
    pub fn shows_address(&self) -> bool {
        self.preset
            .is_some_and(|p| p.kind == PresetKind::Local || p.asks_address())
    }

    pub fn shows_key(&self) -> bool {
        self.preset.is_some_and(|p| p.needs_key || p.asks_address())
    }

    pub fn can_test(&self) -> bool {
        let Some(p) = self.preset else {
            return false;
        };
        !self.busy
            && (!p.needs_key || self.has_key || !self.key.trim().is_empty())
            && (!self.shows_address() || !self.address.trim().is_empty())
    }

    pub fn request(&self) -> Option<ConnectRequest> {
        let p = self.preset?;
        let address = self.address.trim();
        Some(ConnectRequest {
            connection: AiConnection {
                provider: p.id.to_owned(),
                base_url: (self.shows_address() && !address.is_empty()).then(|| address.to_owned()),
                model: self.model.clone(),
                effort: None,
            },
            api_key: Some(self.key.trim().to_owned()).filter(|k| !k.is_empty()),
        })
    }

    pub fn models_loaded(&mut self, result: Result<Vec<ModelChoice>, CallError>) {
        self.busy = false;
        match result {
            Ok(models) => {
                // The model saved before, else the preset's suggestion, else the first.
                let listed = |id: &str| models.iter().any(|m| m.id == id);
                let before = self.saved_model.clone().filter(|m| listed(m));
                let suggested = self
                    .preset
                    .and_then(|p| p.suggested_model)
                    .filter(|s| listed(s))
                    .map(str::to_owned);
                self.model = before
                    .or(suggested)
                    .or_else(|| models.first().map(|m| m.id.clone()))
                    .unwrap_or_default();
                self.models = models;
                self.check = None;
                self.accept_no_tools = false;
                self.error = None;
                self.step = Step::Model;
            }
            Err(e) => self.error = Some(e),
        }
    }

    pub fn pick(&mut self, model: &str) {
        self.model = model.to_owned();
        self.check = None;
        self.accept_no_tools = false;
        self.error = None;
    }

    pub fn checked(&mut self, result: Result<ModelCheck, CallError>) {
        self.busy = false;
        match result {
            Ok(c) => self.check = Some(c),
            Err(e) => self.error = Some(e),
        }
    }

    pub fn can_save(&self) -> bool {
        !self.busy
            && self
                .check
                .is_some_and(|c| c.tool_calling || self.accept_no_tools)
    }
}

/// The wizard. `on_done` runs after the connection was saved, `on_external`
/// once another AI tool connected; `on_cancel` is offered when there is
/// somewhere to go back to.
#[component]
pub fn ConnectWizard(
    #[prop(optional)] initial: Option<Wizard>,
    on_done: Callback<AiConnection>,
    #[prop(optional)] on_external: Option<Callback<()>>,
    #[prop(optional)] on_cancel: Option<Callback<()>>,
) -> impl IntoView {
    let w = RwSignal::new(initial.unwrap_or_default());
    let backend = expect_context::<Backend>();

    // What was saved before: marks the one in use, fills in the details again.
    #[cfg(target_arch = "wasm32")]
    {
        let backend = backend.clone();
        leptos::task::spawn_local(async move {
            if let Ok(saved) = backend.saved_connections().await {
                w.update(|w| w.saved = saved);
            }
        });
    }

    let test = {
        let backend = backend.clone();
        move || {
            let Some(req) = w.with_untracked(Wizard::request) else {
                return;
            };
            w.update(|w| {
                w.busy = true;
                w.error = None;
            });
            let backend = backend.clone();
            leptos::task::spawn_local(async move {
                let result = backend.list_models(&req).await;
                w.update(|w| w.models_loaded(result));
                check_current(&backend, w).await;
            });
        }
    };
    let pick = {
        let backend = backend.clone();
        move |model: String| {
            w.update(|w| w.pick(&model));
            let backend = backend.clone();
            leptos::task::spawn_local(async move { check_current(&backend, w).await });
        }
    };
    let save = move || {
        let Some(req) = w.with_untracked(Wizard::request) else {
            return;
        };
        w.update(|w| w.busy = true);
        let backend = backend.clone();
        leptos::task::spawn_local(async move {
            match backend.save_connection(&req).await {
                Ok(()) => {
                    // The key is in the keychain now; forget it here.
                    w.update(|w| w.key.clear());
                    on_done.run(req.connection);
                }
                Err(e) => w.update(|w| {
                    w.busy = false;
                    w.error = Some(e);
                }),
            }
        });
    };

    view! {
        <div data-screen="connect" class="flex min-h-0 flex-1 justify-center overflow-auto bg-gray-50 p-6 dark:bg-gray-900">
            <div class="w-full max-w-2xl">
                <h1 class="text-2xl font-semibold">{t("connect-title")}</h1>
                <p class="mt-1 text-sm text-gray-600 dark:text-gray-400">{t("connect-intro")}</p>
                {move || match w.with(|w| w.step) {
                    Step::Choose => view! { <ChooseStep w on_cancel /> }.into_any(),
                    Step::Details => view! { <DetailsStep w test=test.clone() /> }.into_any(),
                    Step::Model => {
                        view! { <ModelStep w pick=pick.clone() save=save.clone() /> }.into_any()
                    }
                    Step::External(tool) => view! {
                        <crate::external::ExternalStep
                            tool
                            on_back=Callback::new(move |()| w.update(Wizard::back))
                            on_done=on_external
                        />
                    }.into_any(),
                }}
            </div>
        </div>
    }
}

async fn check_current(backend: &Backend, w: RwSignal<Wizard>) {
    let Some(req) = w.with_untracked(|w| (w.step == Step::Model).then(|| w.request()).flatten())
    else {
        return;
    };
    w.update(|w| w.busy = true);
    let result = backend.check_model(&req).await;
    // The user may have picked another model meanwhile.
    if w.with_untracked(|w| w.model == req.connection.model) {
        w.update(|w| w.checked(result));
    }
}

#[component]
fn ChooseStep(w: RwSignal<Wizard>, on_cancel: Option<Callback<()>>) -> impl IntoView {
    let group = move |kind: PresetKind, title: &'static str, about: &'static str| {
        let cards = PRESETS
            .iter()
            .filter(|p| p.kind == kind)
            .map(|p| {
                // In use, set up before, or new.
                let state = move || {
                    w.with(|w| match w.saved_for(p.id) {
                        Some(s) if s.current => "current",
                        Some(_) => "saved",
                        None => "new",
                    })
                };
                view! {
                    <button
                        title=t("connect-preset-tip")
                        type="button"
                        data-preset=p.id
                        data-state=state
                        class=move || if state() == "current" {
                            "rounded-xl border-2 border-blue-600 bg-blue-50 p-4 text-left hover:shadow-sm dark:bg-blue-950"
                        } else {
                            "rounded-xl border border-gray-200 bg-white p-4 text-left hover:border-blue-500 hover:shadow-sm dark:border-gray-700 dark:bg-gray-950"
                        }
                        on:click=move |_| w.update(|w| w.choose(p))
                    >
                        <span class="flex items-center gap-2">
                            <span class="font-medium">{choice_name(p)}</span>
                            {move || match state() {
                                "current" => Some(view! {
                                    <span data-badge="current" class="rounded-full bg-blue-600 px-2 py-0.5 text-xs text-white">{t("connect-in-use")}</span>
                                }.into_any()),
                                "saved" => Some(view! {
                                    <span data-badge="saved" class="rounded-full bg-gray-200 px-2 py-0.5 text-xs text-gray-700 dark:bg-gray-700 dark:text-gray-200">{t("connect-set-up")}</span>
                                }.into_any()),
                                _ => None,
                            }}
                        </span>
                        {move || w.with(|w| w.saved_for(p.id).map(|s| s.connection.model.clone())).filter(|m| !m.is_empty()).map(|m| view! {
                            <span data-saved-model="" class="mt-1 block truncate font-mono text-xs text-gray-500 dark:text-gray-400">{m}</span>
                        })}
                        <span class="mt-1 block text-sm text-gray-600 dark:text-gray-400">
                            {tr(format!("connect-preset-{}-about", p.id))}
                        </span>
                    </button>
                }
            })
            .collect_view();
        view! {
            <section class="mt-6">
                <h2 class="font-medium">{t(title)}</h2>
                <p class="text-sm text-gray-600 dark:text-gray-400">{t(about)}</p>
                <div class="mt-3 grid gap-3 sm:grid-cols-2">{cards}</div>
            </section>
        }
    };
    let current = move || {
        w.with(|w| {
            w.current().map(|s| {
                let name = tr(format!("connect-preset-{}", s.connection.provider))();
                (name, s.connection.model.clone())
            })
        })
    };
    view! {
        // At the top, staying in view while the page scrolls.
        {on_cancel.map(|cancel| view! {
            <div data-bar="step" class="sticky top-0 z-10 -mx-6 mt-4 bg-gray-50 px-6 py-2 dark:bg-gray-900">
                <button title=t("common-cancel-tip") type="button" class=SECONDARY on:click=move |_| cancel.run(())>
                    {t("common-cancel")}
                </button>
            </div>
        })}
        {move || current().map(|(name, model)| view! {
            <p data-current-ai="" class="mt-4 rounded-lg bg-blue-50 px-3 py-2 text-sm text-blue-900 dark:bg-blue-950 dark:text-blue-100">
                {t_with("connect-current", [("name", name), ("model", model)])}
            </p>
        })}
        {group(PresetKind::Cloud, "connect-cloud", "connect-cloud-about")}
        {group(PresetKind::Local, "connect-local", "connect-local-about")}
        <section data-group="external" class="mt-6">
            <h2 class="font-medium">{t("connect-external")}</h2>
            <p class="text-sm text-gray-600 dark:text-gray-400">{t("connect-external-about")}</p>
            <div class="mt-3 grid gap-3 sm:grid-cols-2">
                {crate::external::TOOLS.iter().map(|(id, _)| {
                    let id: &'static str = id;
                    view! {
                        <button
                            title=t("connect-preset-tip")
                            type="button"
                            data-tool=id
                            class="rounded-xl border border-gray-200 bg-white p-4 text-left hover:border-blue-500 hover:shadow-sm dark:border-gray-700 dark:bg-gray-950"
                            on:click=move |_| w.update(|w| {
                                w.error = None;
                                w.step = Step::External(id);
                            })
                        >
                            <span class="font-medium">{crate::external::tool_name(id)}</span>
                        </button>
                    }
                }).collect_view()}
            </div>
        </section>
    }
}

/// A preset's name while choosing: "Another program (enter its address)"; once
/// connected (top bar, settings) it is the short `connect-preset-<id>`.
fn choice_name(p: &ProviderPreset) -> impl Fn() -> String + Send + Sync + 'static {
    if p.asks_address() {
        tr(format!("connect-preset-{}-choice", p.id))
    } else {
        tr(format!("connect-preset-{}", p.id))
    }
}

/// The step's buttons: at the top of the card, staying in view while the page
/// scrolls.
pub(crate) const STEP_BAR: &str = "sticky top-0 z-10 -mx-5 -mt-5 mb-4 flex flex-wrap gap-2 rounded-t-xl border-b border-gray-200 bg-white px-5 py-3 dark:border-gray-700 dark:bg-gray-950";

const PRIMARY: &str = "rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50";
const SECONDARY: &str = "rounded-lg border border-gray-300 px-4 py-2 text-sm hover:bg-gray-100 dark:border-gray-700 dark:hover:bg-gray-800";
const FIELD: &str = "mt-1 w-full rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm dark:border-gray-700 dark:bg-gray-950";

#[component]
fn DetailsStep(
    w: RwSignal<Wizard>,
    test: impl Fn() + Clone + Send + Sync + 'static,
) -> impl IntoView {
    let preset = move || w.with(|w| w.preset);
    let name = move || preset().map(|p| choice_name(p)());
    let test2 = test.clone();
    view! {
        <section data-step="details" class="mt-6 rounded-xl border border-gray-200 bg-white p-5 dark:border-gray-700 dark:bg-gray-950">
            <div data-bar="step" class=STEP_BAR>
                <button title=t("connect-back-tip") type="button" class=SECONDARY on:click=move |_| w.update(Wizard::back)>
                    {t("connect-back")}
                </button>
                <button
                    title=t("connect-test-tip")
                    type="button"
                    data-action="test"
                    class=PRIMARY
                    disabled=move || !w.with(Wizard::can_test)
                    on:click=move |_| test2()
                >
                    {move || if w.with(|w| w.busy) { t("connect-testing")() } else { t("connect-test")() }}
                </button>
            </div>
            <h2 class="text-lg font-medium">{name}</h2>
            <Show when=move || w.with(Wizard::shows_address)>
                <label class="mt-4 block text-sm font-medium">
                    {t("connect-address")}
                    <input
                        type="url"
                        spellcheck="false"
                        class=FIELD
                        prop:value=move || w.with(|w| w.address.clone())
                        on:input=move |e| w.update(|w| w.address = event_target_value(&e))
                    />
                </label>
            </Show>
            <Show when=move || w.with(Wizard::shows_key)>
                <label class="mt-4 block text-sm font-medium">
                    // Most local programs need none: then it comes last and says so.
                    {move || if preset().is_some_and(|p| p.needs_key) {
                        t("connect-key")()
                    } else {
                        t("connect-key-optional")()
                    }}
                    <input
                        type="password"
                        data-field="key"
                        placeholder=move || w.with(|w| w.has_key).then(|| t("connect-key-saved")())
                        autocomplete="off"
                        spellcheck="false"
                        class=FIELD
                        prop:value=move || w.with(|w| w.key.clone())
                        on:input=move |e| w.update(|w| w.key = event_target_value(&e))
                        on:keydown={
                            let test = test.clone();
                            move |e| if e.key() == "Enter" { test() }
                        }
                    />
                </label>
                <p class="mt-1 text-xs text-gray-500 dark:text-gray-400">{t("connect-key-safe")}</p>
            </Show>
            <Show when=move || preset().is_some_and(|p| p.kind == PresetKind::Local)>
                <p class="mt-3 text-sm text-gray-600 dark:text-gray-400">
                    {move || {
                        if preset().is_some_and(|p| p.asks_address()) {
                            return t("connect-other-running")();
                        }
                        let name = name().unwrap_or_default();
                        crate::i18n::t_with("connect-local-running", [("name", name)])()
                    }}
                </p>
            </Show>
            {move || preset().and_then(|p| p.page).map(|page| view! {
                <a
                    href=page
                    target="_blank"
                    rel="noreferrer"
                    class="mt-3 inline-block text-sm text-blue-700 underline dark:text-blue-300"
                    on:click=move |e| {
                        e.prevent_default();
                        expect_context::<Backend>().open_url(page);
                    }
                >
                    {move || if preset().is_some_and(|p| p.needs_key) {
                        t("connect-get-key")()
                    } else {
                        t("connect-get-program")()
                    }}
                </a>
            })}
            <ErrorBox w />
        </section>
    }
}

#[component]
fn ModelStep(
    w: RwSignal<Wizard>,
    pick: impl Fn(String) + Clone + Send + Sync + 'static,
    save: impl Fn() + Clone + Send + Sync + 'static,
) -> impl IntoView {
    let options = move || {
        w.with(|w| {
            w.models
                .iter()
                .map(|m| {
                    let label = m.display_name.clone().unwrap_or_else(|| m.id.clone());
                    let selected = m.id == w.model;
                    view! { <option value=m.id.clone() selected=selected>{label}</option> }
                })
                .collect_view()
        })
    };
    view! {
        <section data-step="model" class="mt-6 rounded-xl border border-gray-200 bg-white p-5 dark:border-gray-700 dark:bg-gray-950">
            <div data-bar="step" class=STEP_BAR>
                <button title=t("connect-back-tip") type="button" class=SECONDARY on:click=move |_| w.update(Wizard::back)>
                    {t("connect-back")}
                </button>
                <button
                    title=t("connect-save-tip")
                    type="button"
                    data-action="save"
                    class=PRIMARY
                    disabled=move || !w.with(Wizard::can_save)
                    on:click=move |_| save()
                >
                    {t("connect-save")}
                </button>
            </div>
            <p class="text-sm text-green-700 dark:text-green-400">{t("connect-ok")}</p>
            <label class="mt-4 block text-sm font-medium">
                {t("connect-model")}
                <select class=FIELD on:change=move |e| pick(event_target_value(&e))>
                    {options}
                </select>
            </label>
            {move || {
                let (busy, check) = w.with(|w| (w.busy, w.check));
                match check {
                    None if busy => view! {
                        <p class="mt-3 text-sm text-gray-600 dark:text-gray-400">{t("connect-checking")}</p>
                    }.into_any(),
                    None => ().into_any(),
                    Some(c) => view! { <CheckResult w check=c /> }.into_any(),
                }
            }}
            <ErrorBox w />
        </section>
    }
}

#[component]
fn CheckResult(w: RwSignal<Wizard>, check: ModelCheck) -> impl IntoView {
    view! {
        {if check.tool_calling {
            view! {
                <p data-check="ok" class="mt-3 text-sm text-green-700 dark:text-green-400">
                    {t("connect-check-tools")}
                </p>
            }.into_any()
        } else {
            view! {
                <div data-check="no-tools" role="alert" class="mt-3 rounded-lg border border-amber-300 bg-amber-50 p-3 text-sm text-amber-900 dark:border-amber-700 dark:bg-amber-950 dark:text-amber-100">
                    <p>{t("connect-check-no-tools")}</p>
                    <label class="mt-2 flex items-center gap-2">
                        <input
                            type="checkbox"
                            prop:checked=move || w.with(|w| w.accept_no_tools)
                            on:change=move |e| w.update(|w| w.accept_no_tools = event_target_checked(&e))
                        />
                        {t("connect-check-use-anyway")}
                    </label>
                </div>
            }.into_any()
        }}
        {(!check.vision).then(|| view! {
            <p data-check="no-vision" class="mt-2 text-sm text-gray-600 dark:text-gray-400">
                {t("connect-check-no-vision")}
            </p>
        })}
    }
}

#[component]
fn ErrorBox(w: RwSignal<Wizard>) -> impl IntoView {
    move || {
        w.with(|w| w.error.clone()).map(|e| {
            let (id, detail) = match &e {
                CallError::Connect(c) => (
                    format!("connect-error-{}", c.key()),
                    c.detail().map(str::to_owned),
                ),
                CallError::Other(d) => ("connect-error-other".to_owned(), Some(d.clone())),
            };
            let text = tr(id.clone());
            view! {
                <div data-error=id role="alert" class="mt-4 rounded-lg border border-red-300 bg-red-50 p-3 text-sm text-red-900 dark:border-red-800 dark:bg-red-950 dark:text-red-100">
                    <p>{text}</p>
                    {detail.map(|d| view! {
                        <details class="mt-2">
                            <summary class="cursor-pointer">{t("common-details")}</summary>
                            <pre class="mt-1 text-xs whitespace-pre-wrap">{d}</pre>
                        </details>
                    })}
                </div>
            }
        })
    }
}

/// Unused errors still need words: keeps the `ConnectError` keys and the
/// fluent ids in step (tested).
pub fn error_ids() -> Vec<String> {
    [
        ConnectError::Unreachable {
            detail: String::new(),
        },
        ConnectError::WrongKey,
        ConnectError::WrongAddress {
            detail: String::new(),
        },
        ConnectError::NoModels,
        ConnectError::RateLimited,
        ConnectError::Other {
            detail: String::new(),
        },
    ]
    .iter()
    .map(|e| format!("connect-error-{}", e.key()))
    .collect()
}

#[cfg(test)]
mod tests {
    use ak_types::ipc::preset;

    use super::*;

    fn models(ids: &[&str]) -> Vec<ModelChoice> {
        ids.iter()
            .map(|id| ModelChoice {
                id: (*id).into(),
                display_name: None,
            })
            .collect()
    }

    fn chosen(id: &str) -> Wizard {
        let mut w = Wizard::default();
        w.choose(preset(id).unwrap_or(&PRESETS[0]));
        w
    }

    #[test]
    fn cloud_needs_a_key_local_needs_an_address() {
        let mut w = chosen("anthropic");
        assert!(w.shows_key() && !w.shows_address());
        assert!(!w.can_test());
        w.key = " sk-ant-1\n".into();
        assert!(w.can_test());
        let r = w.request().unwrap_or_else(|| panic!("request"));
        assert_eq!(r.api_key.as_deref(), Some("sk-ant-1"));
        assert_eq!(r.connection.base_url, None);

        let mut w = chosen("ollama");
        assert!(!w.shows_key() && w.shows_address());
        assert_eq!(w.address, "http://localhost:11434/v1");
        assert!(w.can_test());
        w.address = "  ".into();
        assert!(!w.can_test());

        // On this computer (llama.cpp's address), a key only if the server wants one.
        let w = chosen("openai-compatible");
        assert_eq!(w.preset.map(|p| p.kind), Some(PresetKind::Local));
        assert!(w.shows_key() && w.shows_address());
        assert_eq!(w.address, "http://localhost:8080/v1");
        assert!(w.can_test());
    }

    #[test]
    fn the_suggested_model_is_picked_and_tool_less_models_need_consent() {
        let mut w = chosen("anthropic");
        w.models_loaded(Ok(models(&["claude-haiku-5-5", "claude-opus-5-5"])));
        assert_eq!(w.step, Step::Model);
        assert_eq!(w.model, "claude-opus-5-5");
        assert!(!w.can_save(), "not checked yet");
        w.checked(Ok(ModelCheck {
            tool_calling: true,
            vision: true,
        }));
        assert!(w.can_save());

        let mut w = chosen("ollama");
        w.models_loaded(Ok(models(&["llama3.2:3b", "qwen3:8b"])));
        assert_eq!(w.model, "llama3.2:3b");
        w.checked(Ok(ModelCheck::default()));
        assert!(!w.can_save());
        w.accept_no_tools = true;
        assert!(w.can_save());
        // Another model: consent and check start over.
        w.pick("qwen3:8b");
        assert!(w.check.is_none() && !w.accept_no_tools && !w.can_save());
    }

    fn saved(
        provider: &str,
        base_url: Option<&str>,
        model: &str,
        has_key: bool,
    ) -> SavedConnection {
        SavedConnection {
            connection: AiConnection {
                provider: provider.into(),
                base_url: base_url.map(Into::into),
                model: model.into(),
                effort: None,
            },
            has_key,
            current: false,
        }
    }

    #[test]
    fn what_was_saved_is_filled_in_again() {
        let mut w = Wizard {
            saved: vec![
                saved("anthropic", None, "claude-sonnet-5-5", true),
                saved(
                    "ollama",
                    Some("http://10.0.0.5:11434/v1"),
                    "qwen3:8b",
                    false,
                ),
            ],
            ..Wizard::default()
        };
        w.saved[1].current = true;
        assert_eq!(
            w.current().map(|s| s.connection.provider.as_str()),
            Some("ollama")
        );

        // A saved key: testing works without typing it again (the app uses the
        // keychain's), and the model saved before is picked over the suggestion.
        w.choose(preset("anthropic").unwrap_or(&PRESETS[0]));
        assert!(w.has_key && w.key.is_empty() && w.can_test());
        assert_eq!(w.request().map(|r| r.api_key), Some(None));
        w.models_loaded(Ok(models(&["claude-opus-5-5", "claude-sonnet-5-5"])));
        assert_eq!(w.model, "claude-sonnet-5-5");
        assert_eq!(w.saved.len(), 2, "kept for going back");

        // The address saved before, not the preset's.
        w.back();
        w.back();
        w.choose(preset("ollama").unwrap_or(&PRESETS[0]));
        assert_eq!(w.address, "http://10.0.0.5:11434/v1");
        // A saved model no longer offered: the usual choice.
        w.models_loaded(Ok(models(&["llama3.2:3b"])));
        assert_eq!(w.model, "llama3.2:3b");

        // Never saved: as before.
        let w = chosen("openai");
        assert!(!w.has_key && !w.can_test() && w.saved_model.is_none());
    }

    #[test]
    fn errors_stay_on_the_step_and_back_clears_them() {
        let mut w = chosen("openai");
        w.key = "bad".into();
        w.busy = true;
        w.models_loaded(Err(CallError::Connect(ConnectError::WrongKey)));
        assert_eq!(w.step, Step::Details);
        assert!(!w.busy && w.error.is_some());
        w.back();
        assert_eq!(w.step, Step::Choose);
        assert!(w.error.is_none());
    }
}
