# Styling with Tailwind CSS 4

Style with utility classes in `class="…"`. There is no `tailwind.config.js` (Tailwind 4
is configured in `style/input.css`); do not add one, and do not add Node/npm.

## Class names must be written out in full
Tailwind finds classes by reading the source. It cannot see classes built at runtime:
```rust
// Wrong: "bg-red-500" never appears in the code.
let class = format!("bg-{}-500", color);
// Right: every full class name is written somewhere.
let class = if urgent { "bg-red-500 text-white" } else { "bg-gray-100 text-gray-900" };
```

## Everyday classes
- Spacing: `p-4` (padding), `px-3 py-2`, `m-2`, `gap-3` (between flex/grid children),
  `space-y-2`.
- Layout: `flex items-center justify-between`, `flex-col`, `grid grid-cols-2`,
  `w-full`, `max-w-lg mx-auto`, `min-h-screen`.
- Text: `text-sm` / `text-lg` / `text-2xl`, `font-medium` / `font-bold`,
  `text-gray-600`, `text-center`, `truncate`.
- Colors: `bg-white`, `bg-blue-600`, `text-white`, `border border-gray-200`;
  shades 50–950.
- Shape: `rounded-lg`, `rounded-full`, `shadow-sm`, `shadow-lg`.
- States: `hover:bg-blue-700`, `focus:ring-2`, `disabled:opacity-50`.
- Arbitrary values when needed: `w-[420px]`, `text-[#0a7]`.

## Dark mode
Out of the box, dark styles follow the computer's setting: add `dark:` variants,
e.g. `bg-white dark:bg-gray-900 text-gray-900 dark:text-gray-100`. ak-ui components
and the page's `<body>` already have them.

**An app that is always dark, or has its own light/dark switch**: on a computer
set to light, `dark:` styles never apply, so ak-ui stays light. Make `dark:`
follow a `dark` class instead. In `style/input.css`, after `@import
"tailwindcss";` (keep the `@source` lines):
```css
@custom-variant dark (&:where(.dark, .dark *));
```
- Always dark: `<html lang="…" class="dark">` in `index.html` (only the `<html>`
  tag; the trunk links stay as they are).
- A switch (light, dark, like the computer): set the class from the screens, at
  start and when the user changes it, and keep the choice (topic `storage`,
  localStorage is enough). web-sys features `Window`, `Document`, `Element`,
  `DomTokenList`, `MediaQueryList`:
```rust
/// Dark or light for the whole app: the `dark` class on `<html>` (with the
/// `@custom-variant` line in style/input.css).
fn set_dark(on: bool) {
    let root = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element());
    if let Some(root) = root {
        let _ = root.class_list().toggle_with_force("dark", on);
    }
}

/// Whether the computer is set to dark: the choice before the user makes one.
fn system_is_dark() -> bool {
    web_sys::window()
        .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten())
        .is_some_and(|m| m.matches())
}
```
At start: `set_dark(match saved.as_deref() { Some("dark") => true, Some("light") => false, _ => system_is_dark() })`.
Then every ak-ui component, the `<body>` and your own `dark:` classes switch
together: no `class="bg-slate-900!"` on each component.

## Window sizes
Apps are resizable, and AgentKnife's preview is often narrower than a desktop
window: about 700–900 px wide and 500–700 px high (`dom_summary` ends with the size
now). Make the layout work there:
- Wider layouts switch on at `sm:` (≥640px) or `md:` (≥768px), e.g.
  `flex flex-col md:flex-row`. A layout that only appears at `lg:` (≥1024px) or
  wider is not seen in the preview.
- A side panel next to the main area: `flex` with `w-64 shrink-0` and
  `min-w-0 flex-1`, rather than a 12-column grid.
- App-like screens (toolbars, panels, an editor) fill the window and scroll
  inside: the outer element `flex h-screen flex-col overflow-hidden`, the part that
  grows `min-h-0 flex-1 overflow-auto`. Then the page itself never scrolls.
- A list or a form may instead be one page that scrolls as a whole
  (`min-h-screen`, as in the starter app).
- Check the layout at the preview's small size (about 800×530; `dom_summary`
  and `screenshot_preview` give the size): everything must be reachable. With
  `overflow-hidden` outside, the part that grows must itself scroll
  (`min-h-0 flex-1 overflow-y-auto`), or nothing below the fold can be seen. Two
  large panels (now playing and the list, a form and its preview) go side by
  side when there is room (`grid gap-3 md:grid-cols-2`). Pictures and
  animations that only decorate take a height that shrinks with the window
  (`h-[clamp(48px,10vh,120px)]`), not a fixed `h-48`.
- Short labels must not wrap into a second line when space is tight: titles
  and buttons in a toolbar `shrink-0 whitespace-nowrap`; in table cells, numbers,
  sizes, dates and types `whitespace-nowrap`, and long names `truncate` (with
  `min-w-0` on their cell).

## Own colors or fonts
In `style/input.css`, after `@import "tailwindcss";` (keep the `@source` lines):
```css
@theme {
  --color-brand-500: #0d9488;
  --font-display: "Georgia", serif;
}
```
Then use `bg-brand-500`, `font-display`.

**Changing a colour everywhere at once** (ak-ui included): give one of
Tailwind's own colours a new value in `@theme`. ak-ui's accent is `blue-600`
(the main button, the chosen tab, a switch that is on, focus rings), its
surfaces `white` / `gray-900` (dark) and its lines `gray-200` / `gray-700`:
```css
@theme {
  --color-blue-600: #0d9488;  /* the accent, in every component and your own classes */
  --color-gray-900: #111827;  /* dark cards, lists and dialogs */
}
```
This changes that colour wherever it is used in the app, which is what keeps one
look. For one component only, use its `class` with `!`; for every component of
a kind (all buttons rounder, no shadows), a rule for its `ak-…` marker class
(topic `ak-ui`, "Restyling every one at once").

## Chinese text and fonts
- The whole app's font is `--font-sans`. For Traditional Chinese, name the
  systems' own fonts first:
  ```css
  @theme {
    --font-sans: "Microsoft JhengHei", "PingFang TC", "Noto Sans TC", ui-sans-serif, system-ui, sans-serif;
  }
  ```
- A font the user added (`assets/fonts/`, topic `overview`), in
  `style/input.css`:
  ```css
  @font-face {
    font-family: "Huninn";
    src: url("assets/fonts/huninn.ttf");
    font-display: swap;
  }
  @theme {
    --font-display: "Huninn", "Microsoft JhengHei", sans-serif;
  }
  ```
  then `font-display` on titles. Chinese fonts are large (several MB): use one
  for titles rather than the whole app, and ask the user before adding many.
- Chinese wraps between any two characters: short labels (buttons, tabs, table
  headers) get `whitespace-nowrap` so they do not break in the middle of a word.
- Full-width punctuation in Chinese text (，。：「」), and dates written the way
  the user writes them (topic `date-time`).

## Looks that hold together
The same request should look alike every time. Unless the user asks otherwise:
- The page: the template's `<body>` colours, content in a column
  (`mx-auto w-full max-w-3xl p-4 sm:p-6`, wider for tables and editors) with
  `flex flex-col gap-4` between the parts.
- Groups in ak-ui `Card`s; one accent colour (the `blue-600` above, or the
  user's); greys for the rest. Spacing from the same steps: `gap-2` inside a
  row, `gap-4` between parts, `p-4` in a card.
- A number to show (a total, a count): `<div class="rounded-xl border p-4">`
  with the label `text-sm text-gray-500 dark:text-gray-400` above the number
  `text-2xl font-semibold tabular-nums`; several side by side
  `grid gap-3 sm:grid-cols-3`.
- A table: `<table class="w-full text-sm">`, header cells `text-left font-medium
  text-gray-500`, rows `border-t border-gray-200 dark:border-gray-700`, numbers
  `text-right tabular-nums`.
- Nothing yet: a short sentence of what to do (`List`'s `empty`, or a
  `text-center text-sm text-gray-500 py-8` line with a button), never an empty
  box.

## Usable with the keyboard, easy to read
Some users do not use a mouse, see less well, or need larger text. The rules are
few:
- Anything clickable is a `<button>` (or ak-ui's `Button`), not a `<div on:click>`:
  buttons get Tab, Enter and Space for free. A link to another page in the app
  is a button too.
- Never remove the focus outline (`outline-none` alone). Own controls get the same
  ring as ak-ui's: `focus-visible:outline-2 focus-visible:outline-offset-2
  focus-visible:outline-blue-600`.
- A button with only an icon or a sign (×, ⋯, a picture) gets a name:
  `aria-label="Delete"` and `title="Delete"` (the mouse shows it), in the user's
  language.
- Every field has a visible label (`<label for=…>` or ak-ui's `Input label=…`);
  a placeholder is not a label.
- Text contrast: normal text `text-gray-700` or darker on white
  (`dark:text-gray-200` on dark); `text-gray-400` only for hints, never for
  content. Do not show a state by colour alone: add a word or a sign ("✓ Paid",
  "Late").
- Sizes: text at least `text-sm`; things to click at least `h-8` (32 px).
- Order: Tab moves through the screen in reading order; do not reorder with CSS
  what the user tabs through. A dialog puts the focus in its first field and
  closes with Esc (ak-ui's `Dialog` does).
- Check: press `Tab` with `interact` `key` a few times; each answer's `focus` is
  the `data-ak-id` that has the focus now. Ask the user to try the screen without
  the mouse too.
