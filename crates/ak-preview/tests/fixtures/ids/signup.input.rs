use leptos::prelude::*;

#[component]
fn SignupForm() -> impl IntoView {
    let email = RwSignal::new(String::new());
    let agree = RwSignal::new(false);
    view! {
        <section class="mx-auto max-w-md p-6">
            <h2 class="text-xl font-bold">"Create your account"</h2>
            <form on:submit=move |ev| {
                ev.prevent_default();
                submit();
            }>
                <label class="block">"Email"</label>
                <input type="email" placeholder="you@example.com" bind:value=email />
                <label>
                    <input type="checkbox" name="terms" bind:checked=agree />
                    "I agree"
                </label>
                <select name="plan">
                    <option value="free">"Free"</option>
                </select>
                <button type="submit" disabled=move || !agree.get()>"Sign up"</button>
            </form>
            <p class="text-sm">"Already have one?" <a href="/login">"Log in"</a></p>
        </section>
    }
}
