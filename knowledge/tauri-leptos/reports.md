# Charts, printing and PDF

Reports need three things: charts drawn in the screens, a layout that prints well,
and a way to keep the result (PDF, a picture). No chart library is needed: draw
the chart as SVG in `view!`.

## Charts in SVG
`rect`, `circle`, `polyline`, `path`, `text` and `g` are SVG elements wherever they
are in `view!`, also inside closures and `For`. Do not use `<title>` in SVG: `view!`
makes it the page's HTML title. Write a value on the chart or below it instead.

```rust
/// Colours for the series, in order. Written as colours, not Tailwind classes:
/// a chart saved as a picture has no Tailwind.
const COLOURS: [&str; 6] = ["#0ea5e9", "#f59e0b", "#10b981", "#ef4444", "#8b5cf6", "#64748b"];

/// Bars for (label, value) pairs; values from 0 up.
#[component]
fn BarChart(#[prop(into)] ak_id: String, #[prop(into)] data: Signal<Vec<(String, f64)>>) -> impl IntoView {
    const W: f64 = 400.0; // drawing units, not pixels: the chart scales to its box
    const H: f64 = 200.0;
    const TOP: f64 = 16.0; // room for the value above the tallest bar
    view! {
        <svg data-ak-id=ak_id xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 240" class="w-full h-auto">
            {move || {
                let rows = data.get();
                let max = rows.iter().map(|r| r.1).fold(0.0, f64::max);
                let slot = W / rows.len().max(1) as f64;
                rows.into_iter()
                    .enumerate()
                    .map(|(i, (label, value))| {
                        let h = if max > 0.0 { value / max * H } else { 0.0 };
                        let mid = (i as f64 + 0.5) * slot;
                        view! {
                            <rect x={mid - slot * 0.35} y={TOP + H - h} width={slot * 0.7} height=h fill={COLOURS[0]} />
                            <text x=mid y={TOP + H - h - 4.0} text-anchor="middle" font-size="11" fill="#111827">
                                {format!("{value:.0}")}
                            </text>
                            <text x=mid y={TOP + H + 16.0} text-anchor="middle" font-size="12" fill="#4b5563">
                                {label}
                            </text>
                        }
                    })
                    .collect_view()
            }}
        </svg>
    }
}

/// Shares of a whole as a ring: each slice is a circle whose dashed border shows
/// only its part (no angles to compute; one slice of 100% works too).
#[component]
fn PieChart(#[prop(into)] ak_id: String, #[prop(into)] data: Signal<Vec<(String, f64)>>) -> impl IntoView {
    view! {
        <div class="flex items-center gap-4">
            // r = 15.915 makes the circle 100 units around: a slice of 30% is 30 long.
            <svg data-ak-id=ak_id xmlns="http://www.w3.org/2000/svg" viewBox="0 0 42 42" class="w-40 h-40 -rotate-90">
                {move || {
                    let rows = data.get();
                    let total: f64 = rows.iter().map(|r| r.1).sum();
                    let mut start = 0.0;
                    rows.iter()
                        .enumerate()
                        .map(|(i, (_, value))| {
                            let part = if total > 0.0 { value / total * 100.0 } else { 0.0 };
                            let offset = -start;
                            start += part;
                            view! {
                                <circle cx="21" cy="21" r="15.915" fill="none" stroke-width="6"
                                    stroke={COLOURS[i % COLOURS.len()]}
                                    stroke-dasharray={format!("{part} {}", 100.0 - part)}
                                    stroke-dashoffset=offset />
                            }
                        })
                        .collect_view()
                }}
            </svg>
            <ul class="text-sm">
                {move || {
                    let rows = data.get();
                    let total: f64 = rows.iter().map(|r| r.1).sum();
                    rows.into_iter()
                        .enumerate()
                        .map(|(i, (label, value))| {
                            let pct = if total > 0.0 { value / total * 100.0 } else { 0.0 };
                            view! {
                                <li class="flex items-center gap-2">
                                    <span class="inline-block w-3 h-3 rounded-sm" style:background={COLOURS[i % COLOURS.len()]}></span>
                                    {format!("{label} {pct:.0}%")}
                                </li>
                            }
                        })
                        .collect_view()
                }}
            </ul>
        </div>
    }
}

/// A line through (label, value) points, e.g. one per month.
#[component]
fn LineChart(#[prop(into)] ak_id: String, #[prop(into)] data: Signal<Vec<(String, f64)>>) -> impl IntoView {
    const W: f64 = 400.0;
    const H: f64 = 200.0;
    const TOP: f64 = 10.0;
    view! {
        <svg data-ak-id=ak_id xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 236" class="w-full h-auto">
            {move || {
                let rows = data.get();
                let max = rows.iter().map(|r| r.1).fold(0.0, f64::max);
                let step = W / rows.len().max(1) as f64;
                let at = |i: usize, v: f64| {
                    let y = if max > 0.0 { TOP + H - v / max * H } else { TOP + H };
                    ((i as f64 + 0.5) * step, y)
                };
                let points: Vec<String> = rows
                    .iter()
                    .enumerate()
                    .map(|(i, r)| {
                        let (x, y) = at(i, r.1);
                        format!("{x:.1},{y:.1}")
                    })
                    .collect();
                let labels = rows
                    .iter()
                    .enumerate()
                    .map(|(i, (label, value))| {
                        let (x, y) = at(i, *value);
                        view! {
                            <circle cx=x cy=y r="3" fill={COLOURS[0]} />
                            <text x=x y={TOP + H + 18.0} text-anchor="middle" font-size="12" fill="#4b5563">
                                {label.clone()}
                            </text>
                        }
                    })
                    .collect_view();
                view! {
                    <polyline points={points.join(" ")} fill="none" stroke={COLOURS[0]} stroke-width="2" />
                    {labels}
                }
            }}
        </svg>
    }
}
```
Use them with a `Memo` that adds up the data, so the chart follows every change:
```rust
#[derive(Clone, PartialEq)]
struct Expense {
    category: String,
    amount: f64,
}

#[component]
fn Report() -> impl IntoView {
    let expenses = RwSignal::new(vec![
        Expense { category: "Food".into(), amount: 320.0 },
        Expense { category: "Bus".into(), amount: 80.0 },
    ]);
    // One (label, total) per category, biggest first.
    let by_category = Memo::new(move |_| {
        let mut totals: Vec<(String, f64)> = Vec::new();
        for e in expenses.get() {
            match totals.iter_mut().find(|t| t.0 == e.category) {
                Some(t) => t.1 += e.amount,
                None => totals.push((e.category, e.amount)),
            }
        }
        totals.sort_by(|a, b| b.1.total_cmp(&a.1));
        totals
    });
    view! {
        <BarChart ak_id="chart-by-category" data=by_category />
        <PieChart ak_id="chart-shares" data=by_category />
    }
}
```
- Many labels (30 days): show every 5th label, or turn the chart into a table.
- Check a chart with `dom_summary` (SVG parts with a `data-ak-id` are listed) and
  `read_state`; its look only in the preview's picture.

## Printing and PDF
`window.print()` opens the system's print dialog with a preview. Its printer list
has "Microsoft Print to PDF" (or "Save as PDF"): that is how the user makes a PDF.
No PDF crate is needed.
```rust
let print = move |_| {
    if let Some(w) = web_sys::window() {
        let _ = w.print();
    }
};
// <Button ak_id="print-report" on_click=print>"Print / PDF"</Button>
```
The printout is the page itself, styled for paper with Tailwind's `print:` classes:
- `print:hidden` on everything that is not the report: the toolbar, the sidebar,
  buttons, the navigation.
- `hidden print:block` for what only paper needs: a title with the date, a footer.
- Full width on paper: `w-80 print:w-full`; no shadows or scrolling boxes:
  `overflow-auto print:overflow-visible`, `h-[600px] print:h-auto`.
- Rows that must stay together: `break-inside-avoid`; a new page before a section:
  `break-before-page`.
- Paper size and margins go in `style/input.css`:
  ```css
  @page { size: A4; margin: 15mm; }
  ```
- Browsers leave out background colours when printing. Bars drawn with `fill`
  print; a coloured box (`bg-…`) needs `print-color-adjust: exact` in its `style`.

Printing only works in the real window, not in the preview: do not press the print
button with `interact` (the dialog would stay open). Check the layout by reading
your `print:` classes, then ask the user to try "Print" in the real window and pick
"Microsoft Print to PDF".

## Keeping a chart as a picture
Save the chart's SVG as a file: browsers, Word and PowerPoint open it. The chart
must use colour values (`fill="#0ea5e9"`), not classes, as above. Screens (needs the
web-sys features `Document` and `Element`):
```rust
/// The SVG text of the element with this `data-ak-id`.
fn svg_text(ak_id: &str) -> Option<String> {
    let doc = web_sys::window()?.document()?;
    let el = doc.query_selector(&format!("[data-ak-id='{ak_id}']")).ok()??;
    Some(el.outer_html())
}
```
Send it to a backend command that writes it to the Downloads folder like
`export_csv` (topic `storage`), with the extension `.svg` and no BOM. A picture of
the whole window is not possible from the app: the user can press Win+Shift+S.
