use ak_ui::*;
use leptos::prelude::*;

#[component]
fn TodoScreen() -> impl IntoView {
    let todos = RwSignal::new(Vec::<Todo>::new());
    let filter = RwSignal::new("all".to_string());
    view! {
        <main class="p-4">
            <h1>"待辦清單"</h1>
            <Nav ak_id="filter-tabs" items=vec![NavItem::new("all", "All")] active=filter />
            <ul class="divide-y">
                <For
                    each=move || todos.get()
                    key=|t| t.id
                    children=move |t: Todo| view! { <li><button on:click=move |_| remove(t.id)>"✕"</button></li> }
                />
            </ul>
            <Show when=move || todos.with(Vec::is_empty) fallback=|| view! { <p>"never"</p> }>
                <p class="text-gray-500">"Nothing to do"</p>
            </Show>
            {move || (todos.with(Vec::len) > 3).then(|| view! { <p>"Busy day"</p> })}
            <footer><button data-ak-id="clear-button" on:click=move |_| todos.set(vec![])>"Clear"</button></footer>
        </main>
    }
}
