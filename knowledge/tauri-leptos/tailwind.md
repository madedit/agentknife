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
Apps are resizable: use `sm:` (≥640px), `md:` (≥768px), `lg:` (≥1024px) for wider
layouts, e.g. `flex flex-col md:flex-row`.

## Own colors or fonts
In `style/input.css`, after `@import "tailwindcss";` (keep the `@source` lines):
```css
@theme {
  --color-brand-500: #0d9488;
  --font-display: "Georgia", serif;
}
```
Then use `bg-brand-500`, `font-display`.
