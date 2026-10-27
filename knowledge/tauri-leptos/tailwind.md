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
Dark styles follow the computer's setting: add `dark:` variants,
e.g. `bg-white dark:bg-gray-900 text-gray-900 dark:text-gray-100`. ak-ui components
already have them.

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

## Own colors or fonts
In `style/input.css`, after `@import "tailwindcss";` (keep the `@source` lines):
```css
@theme {
  --color-brand-500: #0d9488;
  --font-display: "Georgia", serif;
}
```
Then use `bg-brand-500`, `font-display`.

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
