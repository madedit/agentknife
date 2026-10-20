use leptos::prelude::*;

#[component]
fn SignupForm() -> impl IntoView {
    let email = RwSignal::new(String::new());
    let agree = RwSignal::new(false);
    view! {
        <section data-ak-id="signup-form-section" class="mx-auto max-w-md p-6">
            <h2 data-ak-id="create-your-account-title" class="text-xl font-bold">"Create your account"</h2>
            <form data-ak-id="signup-form" on:submit=move |ev| {
                ev.prevent_default();
                submit();
            }>
                <label data-ak-id="email-label" class="block">"Email"</label>
                <input data-ak-id="email-input" type="email" placeholder="you@example.com" bind:value=email />
                <label data-ak-id="i-agree-label">
                    <input data-ak-id="terms-checkbox" type="checkbox" name="terms" bind:checked=agree />
                    "I agree"
                </label>
                <select data-ak-id="plan-select" name="plan">
                    <option value="free">"Free"</option>
                </select>
                <button data-ak-id="sign-up-button" type="submit" disabled=move || !agree.get()>"Sign up"</button>
            </form>
            <p data-ak-id="already-have-one-text" class="text-sm">"Already have one?" <a data-ak-id="log-in-link" href="/login">"Log in"</a></p>
        </section>
    }
}
