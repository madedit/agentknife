//! The app's screens (Leptos, runs in the window's web view).
//!
//! Backend calls go through `ak_bridge::invoke`, which works both in the real
//! app and in AgentKnife's live preview. Give every meaningful element a stable
//! `data-ak-id` (ak-ui components take it as `ak_id`).

use ak_ui::*;
use leptos::prelude::*;
use serde::Serialize;

fn main() {
    ak_bridge::start();
    leptos::mount::mount_to_body(App);
}

#[component]
fn App() -> impl IntoView {
    view! {
        <ToastHost ak_id="toasts">
            <Home />
        </ToastHost>
    }
}

/// Arguments of the backend's `greet` command (see src-tauri/src/main.rs).
#[derive(Serialize)]
struct GreetArgs {
    name: String,
}

#[component]
fn Home() -> impl IntoView {
    let toaster = use_toaster();
    let name = RwSignal::new(String::new());
    let greeting = RwSignal::new(None::<String>);

    let greet = move |()| {
        let args = GreetArgs {
            name: name.get_untracked(),
        };
        leptos::task::spawn_local(async move {
            match ak_bridge::invoke::<String>("greet", &args).await {
                Ok(text) => greeting.set(Some(text)),
                Err(e) => toaster.error(e.to_string()),
            }
        });
    };

    view! {
        <main class="mx-auto flex min-h-screen max-w-lg flex-col justify-center gap-6 p-6">
            <h1 data-ak-id="home-title" class="text-3xl font-bold">
                "My App"
            </h1>
            <Card ak_id="greet-card" title="Say hello">
                <Form ak_id="greet-form" on_submit=greet>
                    <Input ak_id="name-input" value=name label="Your name" placeholder="Ada" />
                    <Button ak_id="greet-button" submit=true>
                        "Greet"
                    </Button>
                </Form>
                <Show when=move || greeting.with(Option::is_some)>
                    <p data-ak-id="greeting" class="mt-4 text-lg font-medium text-blue-700 dark:text-blue-300">
                        {move || greeting.get()}
                    </p>
                </Show>
            </Card>
        </main>
    }
}
