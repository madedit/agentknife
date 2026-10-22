# Dates and times

Keep dates as text `"2026-10-22"` (year-month-day, zero-padded): it sorts and
compares correctly as text, a month filter is `date.starts_with("2026-10")`, and
`<Input kind="date">` reads and writes the same form. Save it like that too.

## Screens: `js_sys::Date`
`std::time::SystemTime::now()` crashes in the screens (WebAssembly). Use the
browser's clock through `js-sys` (`js-sys = "0.3.106"` in the screens'
`Cargo.toml`):
```rust
/// Today on the user's computer, as "2026-10-22".
fn today() -> String {
    let d = js_sys::Date::new_0();
    // get_month() counts from 0 (January is 0).
    format!("{:04}-{:02}-{:02}", d.get_full_year(), d.get_month() + 1, d.get_date())
}

/// "2026-10" moved by `by` months ("2026-01", -1 → "2025-12").
fn shift_month(month: &str, by: i32) -> String {
    let (y, m) = month
        .split_once('-')
        .and_then(|(y, m)| Some((y.parse::<i32>().ok()?, m.parse::<i32>().ok()?)))
        .unwrap_or((1970, 1));
    let n = y * 12 + (m - 1) + by;
    format!("{:04}-{:02}", n.div_euclid(12), n.rem_euclid(12) + 1)
}
```
- This month: `today()[..7].to_owned()`.
- Time of day: `d.get_hours()`, `d.get_minutes()`. A timestamp in milliseconds:
  `js_sys::Date::now()`.
- Not `to_iso_string()` for today's date: it is UTC (the wrong day near
  midnight) and returns a `JsString`, not a `String` (`String::from(…)` converts).

## Backend: chrono
`chrono = "0.4.45"` in `src-tauri/Cargo.toml`:
```rust
fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}
```
