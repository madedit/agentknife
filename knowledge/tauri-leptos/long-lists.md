# Long lists: search, sort and pages

A list of a few hundred rows needs nothing special. With thousands, two things make
the screen slow: building every row at once, and filtering again on each render.
Filter and sort once in a `Memo`, and show one page of it.

```rust
#[derive(Clone, PartialEq)]
struct Contact {
    id: u32,
    name: String,
    city: String,
    spent: f64,
}

const PAGE: usize = 50;

#[component]
fn Contacts() -> impl IntoView {
    let all = RwSignal::new(Vec::<Contact>::new()); // loaded from the backend
    let query = RwSignal::new(String::new());
    let sort = RwSignal::new("name".to_string());
    let page = RwSignal::new(0usize);

    // Runs again only when the rows, the search or the sort change.
    let found = Memo::new(move |_| {
        let q = query.get().trim().to_lowercase();
        let mut rows: Vec<Contact> = all.with(|rows| {
            rows.iter()
                .filter(|c| {
                    q.is_empty() || c.name.to_lowercase().contains(&q) || c.city.to_lowercase().contains(&q)
                })
                .cloned()
                .collect()
        });
        match sort.get().as_str() {
            // Numbers as numbers: as text, "100" comes before "9".
            "spent" => rows.sort_by(|a, b| b.spent.total_cmp(&a.spent)),
            _ => rows.sort_by(|a, b| a.name.cmp(&b.name)),
        }
        rows
    });
    let pages = move || found.with(|f| f.len().div_ceil(PAGE).max(1));
    let shown = move || {
        found.with(|f| f.iter().skip(page.get() * PAGE).take(PAGE).cloned().collect::<Vec<_>>())
    };

    view! {
        <input data-ak-id="contact-search" type="search" placeholder="Search"
            prop:value=move || query.get()
            // A new search starts on the first page.
            on:input=move |e| { query.set(event_target_value(&e)); page.set(0); } />
        <select data-ak-id="contact-sort" on:change=move |e| { sort.set(event_target_value(&e)); page.set(0); }>
            <option value="name">"Name"</option>
            <option value="spent">"Spent"</option>
        </select>
        <p data-ak-id="contact-count">{move || format!("{} found", found.with(|f| f.len()))}</p>
        <ul>
            <For
                each=shown
                key=|c| c.id
                children=move |c: Contact| view! {
                    <li data-ak-id=format!("contact-{}", c.id)>{c.name} " · " {c.city}</li>
                }
            />
        </ul>
        <Show when=move || found.with(|f| f.is_empty())>
            <p data-ak-id="contact-none">"Nothing matches the search."</p>
        </Show>
        <div class="flex gap-2 items-center">
            <button data-ak-id="page-prev" disabled=move || page.get() == 0
                on:click=move |_| page.update(|p| *p = p.saturating_sub(1))>"Previous"</button>
            <span data-ak-id="page-number">{move || format!("{} / {}", page.get() + 1, pages())}</span>
            <button data-ak-id="page-next" disabled=move || page.get() + 1 >= pages()
                on:click=move |_| page.update(|p| *p += 1)>"Next"</button>
        </div>
    }
}
```
- `Memo` needs `PartialEq` on the row type (`#[derive(Clone, PartialEq)]`).
- `all.with(|rows| …)` reads the list without copying all of it; only the matches
  are cloned.
- Reset the page where the search or sort changes (in its handler), not in an
  `Effect`.
- Search with `to_lowercase()` on both sides and `contains`; never cut the text at
  a position found in the lowercased copy (topic `rich-text`).
- "Show more" instead of pages: `take((page.get() + 1) * PAGE)` and a button that
  adds one to `page`.
- Up to about 20,000 rows the screens can search them like this. More (a log, years
  of records): let a backend command search and return one page, and send it
  `query` and `page`.
- Sorting text with `cmp` follows character codes: English A–Z works; Chinese is not
  in stroke or 注音 order. Offer the sort the user needs (date, amount) first.
- Test with many rows: add a backend command or a button in the preview that makes
  2,000 sample rows, check the count and the pages with `dom_summary`, then remove it.
