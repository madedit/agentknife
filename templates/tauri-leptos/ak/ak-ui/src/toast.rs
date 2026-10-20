use leptos::prelude::*;

use crate::part_id;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToastKind {
    #[default]
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToastMessage {
    id: u64,
    kind: ToastKind,
    text: String,
}

/// Shows short messages ("Saved", "Could not save"). Get it with
/// [`use_toaster`] anywhere inside a [`ToastHost`].
#[derive(Clone, Copy)]
pub struct Toaster {
    toasts: RwSignal<Vec<ToastMessage>>,
    next: RwSignal<u64>,
    // Timers only exist in the browser.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    duration_ms: u32,
}

impl Toaster {
    pub fn show(&self, kind: ToastKind, text: impl Into<String>) {
        let id = self.next.get_untracked() + 1;
        self.next.set(id);
        self.toasts.update(|t| {
            t.push(ToastMessage {
                id,
                kind,
                text: text.into(),
            })
        });
        #[cfg(target_arch = "wasm32")]
        {
            let this = *self;
            set_timeout(
                move || this.dismiss(id),
                std::time::Duration::from_millis(u64::from(self.duration_ms)),
            );
        }
    }

    pub fn info(&self, text: impl Into<String>) {
        self.show(ToastKind::Info, text);
    }

    pub fn success(&self, text: impl Into<String>) {
        self.show(ToastKind::Success, text);
    }

    pub fn error(&self, text: impl Into<String>) {
        self.show(ToastKind::Error, text);
    }

    pub fn dismiss(&self, id: u64) {
        self.toasts.update(|t| t.retain(|m| m.id != id));
    }
}

/// The [`Toaster`] of the surrounding [`ToastHost`].
///
/// # Panics
/// Outside a `ToastHost`.
pub fn use_toaster() -> Toaster {
    expect_context::<Toaster>()
}

/// Wrap the app (or a screen) in this to use toasts inside it. Each toast has
/// `data-ak-id` `<ak_id>-<n>` and disappears after `duration_ms` (default 4 s).
#[component]
pub fn ToastHost(
    #[prop(into)] ak_id: String,
    #[prop(optional)] duration_ms: Option<u32>,
    children: Children,
) -> impl IntoView {
    let toaster = Toaster {
        toasts: RwSignal::new(Vec::new()),
        next: RwSignal::new(0),
        duration_ms: duration_ms.unwrap_or(4000),
    };
    provide_context(toaster);
    let base = ak_id.clone();
    view! {
        {children()}
        <div
            data-ak-id=ak_id
            aria-live="polite"
            class="pointer-events-none fixed bottom-4 right-4 z-50 flex w-80 max-w-[calc(100vw-2rem)] flex-col gap-2"
        >
            <For
                each=move || toaster.toasts.get()
                key=|m| m.id
                children=move |m| {
                    let class = match m.kind {
                        ToastKind::Info => {
                            "pointer-events-auto rounded-lg bg-gray-900 px-4 py-3 text-sm text-white shadow-lg"
                        }
                        ToastKind::Success => {
                            "pointer-events-auto rounded-lg bg-green-600 px-4 py-3 text-sm text-white shadow-lg"
                        }
                        ToastKind::Error => {
                            "pointer-events-auto rounded-lg bg-red-600 px-4 py-3 text-sm text-white shadow-lg"
                        }
                    };
                    let role = if m.kind == ToastKind::Error { "alert" } else { "status" };
                    let id = m.id;
                    view! {
                        <div
                            data-ak-id=part_id(&base, id)
                            role=role
                            class=class
                            on:click=move |_| toaster.dismiss(id)
                        >
                            {m.text}
                        </div>
                    }
                }
            />
        </div>
    }
}
