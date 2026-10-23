# The data-ak-id convention

Every meaningful element carries `data-ak-id`. The user points at elements in the
preview ("make this bigger"); AgentKnife reports the id, and you find it in the code
by searching for it. AgentKnife also uses ids to click, type and read the screen when
checking your work.

## Which elements
- Everything the user can click or type into: buttons, inputs, links, toggles.
- Things the user may want to change: headings, important text, images, cards,
  sections, lists and their rows, messages.
- Not needed: purely structural wrappers (a flex row inside a card).

## Naming
- kebab-case, describing what it is: `add-button`, `todo-input`, `settings-card`,
  `total-amount`.
- Unique on the screen. If two places would clash, prefix by area: `header-title`,
  `footer-title`.
- Stable: do not rename ids when changing looks; the user may refer to them.
- With ak-ui, pass it as `ak_id="…"`; on plain elements write `data-ak-id="…"`.

## Repeated elements
Derive from a stable item id, never the position:
```rust
view! { <li data-ak-id=format!("todo-{}", t.id)> … <button data-ak-id=format!("todo-{}-delete", t.id)>"✕"</button></li> }
```
ak-ui's `List` names rows `<list id>-<key>` for you, cleaning the key. Ids you
build yourself are used as they are: from text that may hold spaces, `#` or
other signs (a colour code, a name), build them with
`ak_ui::part_id("highlight", &colour)` (signs become `-`), or use a plain
name (`highlight-yellow`).

## Finding an element the user pointed at
Search the code for the id (`search_files` for `add-button`). If the user points at
an element without an id, AgentKnife reports the nearest parent with one; add an id
to the element itself when you change it.
