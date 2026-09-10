//! Rendering — a pure function of [`App`] onto a ratatui frame.
//!
//! Modeled on Claude Code's Usage panel: a borderless dark layout with a big
//! headline figure, an orange daily-cost line chart, a row of total tiles, and
//! a grouped breakdown table. Nothing here mutates state or does I/O.

use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Axis, Cell, Chart, Dataset, GraphType, Paragraph, Row, Table};
use ratatui::Frame;

use crate::core::analysis::Line as Group;
use crate::tui::app::{App, Breakdown, Metric};

const ORANGE: Color = Color::Rgb(0xD9, 0x77, 0x57);
const WHITE: Color = Color::Rgb(0xEC, 0xEC, 0xEC);
const MUTED: Color = Color::Rgb(0x8A, 0x8A, 0x8A);
const FAINT: Color = Color::Rgb(0x5A, 0x5A, 0x5A);
const PILL_BG: Color = Color::Rgb(0x33, 0x33, 0x33);

/// The little six-pointed mark next to Claude models in the reference UI.
const STAR: &str = "✳ ";

/// Draws the whole dashboard.
pub fn draw(frame: &mut Frame, app: &App) {
    let cols = Layout::horizontal([Constraint::Length(2), Constraint::Min(0), Constraint::Length(2)])
        .split(frame.area());
    let body = cols[1];

    let rows = Layout::vertical([
        Constraint::Length(1), // header
        Constraint::Length(1), // spacer
        Constraint::Length(11), // headline + chart
        Constraint::Length(1), // spacer
        Constraint::Length(3), // totals
        Constraint::Length(1), // spacer
        Constraint::Min(4),    // breakdown
        Constraint::Length(1), // footer
    ])
    .split(body);

    draw_header(frame, rows[0], app);
    draw_upper(frame, rows[2], app);
    draw_totals(frame, rows[4], app);
    draw_breakdown(frame, rows[6], app);
    draw_footer(frame, rows[7], app);
}

// ---- header -------------------------------------------------------------

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let split = Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).split(area);

    let title = Line::from(vec![
        Span::styled("Usage", Style::default().fg(WHITE).add_modifier(Modifier::BOLD)),
        Span::styled("  /  ", Style::default().fg(FAINT)),
        Span::styled(date_range_label(app), Style::default().fg(MUTED)),
    ]);
    frame.render_widget(Paragraph::new(title), split[0]);

    let mut spans = Vec::new();
    for (metric, label) in [(Metric::Cost, "Cost"), (Metric::Tokens, "Tokens")] {
        spans.push(pill(label, app.metric == metric));
    }
    spans.push(Span::raw("   "));
    for range in crate::tui::app::Range::ALL {
        spans.push(pill(range.label(), app.range == range));
        spans.push(Span::raw(" "));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)).alignment(Alignment::Right), split[1]);
}

/// A selector chip: filled when active, muted otherwise.
fn pill(label: &str, active: bool) -> Span<'static> {
    let text = format!(" {label} ");
    if active {
        Span::styled(text, Style::default().fg(WHITE).bg(PILL_BG).add_modifier(Modifier::BOLD))
    } else {
        Span::styled(text, Style::default().fg(MUTED))
    }
}

// ---- headline + chart ---------------------------------------------------

fn draw_upper(frame: &mut Frame, area: Rect, app: &App) {
    let split = Layout::horizontal([Constraint::Percentage(40), Constraint::Min(0)]).split(area);
    draw_headline(frame, split[0], app);
    draw_chart(frame, split[1], app);
}

fn draw_headline(frame: &mut Frame, area: Rect, app: &App) {
    let o = &app.analysis.overall;
    let sessions = app.analysis.by_session.len();

    let headline = match app.metric {
        Metric::Cost => fmt_usd(o.cost_usd),
        Metric::Tokens => fmt_tokens(o.totals.grand_total()),
    };
    let subtitle = match app.metric {
        Metric::Cost => format!("{sessions} sessions · API estimate"),
        Metric::Tokens => format!("{sessions} sessions · tokens processed"),
    };
    let provider_value = match app.metric {
        Metric::Cost => fmt_usd(o.cost_usd),
        Metric::Tokens => fmt_tokens(o.totals.grand_total()),
    };

    let lines = vec![
        Line::from(Span::styled(headline, Style::default().fg(WHITE).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled(subtitle, Style::default().fg(MUTED))),
        Line::from(""),
        Line::from(vec![
            Span::styled("● ", Style::default().fg(ORANGE)),
            Span::styled(STAR, Style::default().fg(ORANGE)),
            Span::styled("Claude Code  ", Style::default().fg(WHITE)),
            Span::styled(format!("{sessions} sessions   "), Style::default().fg(MUTED)),
            Span::styled(provider_value, Style::default().fg(WHITE).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(Span::styled(
            format!("100.0% of cost · {} tokens", fmt_tokens(o.totals.grand_total())),
            Style::default().fg(MUTED),
        )),
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_chart(frame: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
    let title = if app.metric == Metric::Cost { "Daily cost" } else { "Daily tokens" };
    frame.render_widget(Paragraph::new(Span::styled(title, Style::default().fg(MUTED))), rows[0]);

    let days: Vec<(&String, &Group)> = app.analysis.by_day.iter().collect();
    if days.len() < 2 {
        frame.render_widget(Paragraph::new(Span::styled("  not enough data to chart", Style::default().fg(FAINT))), rows[1]);
        return;
    }

    let data: Vec<(f64, f64)> = days
        .iter()
        .enumerate()
        .map(|(i, (_, g))| (i as f64, metric_value(app.metric, g)))
        .collect();
    let y_max = data.iter().map(|&(_, y)| y).fold(0.0_f64, f64::max).max(1.0) * 1.1;
    let x_max = (days.len() - 1) as f64;

    let y_labels = vec![
        axis_span(fmt_axis(app.metric, 0.0)),
        axis_span(fmt_axis(app.metric, y_max / 2.0)),
        axis_span(fmt_axis(app.metric, y_max)),
    ];
    let x_labels = vec![
        axis_span(upper_day(days[0].0)),
        axis_span(upper_day(days[days.len() / 2].0)),
        axis_span(upper_day(days[days.len() - 1].0)),
    ];

    let dataset = Dataset::default()
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(ORANGE))
        .data(&data);

    let chart = Chart::new(vec![dataset])
        .x_axis(Axis::default().style(Style::default().fg(FAINT)).bounds([0.0, x_max]).labels(x_labels))
        .y_axis(Axis::default().style(Style::default().fg(FAINT)).bounds([0.0, y_max]).labels(y_labels))
        .legend_position(None);
    frame.render_widget(chart, rows[1]);
}

fn axis_span(text: String) -> Line<'static> {
    Line::from(Span::styled(text, Style::default().fg(MUTED)))
}

// ---- totals -------------------------------------------------------------

fn draw_totals(frame: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(2)]).split(area);
    frame.render_widget(
        Paragraph::new(Span::styled("Totals", Style::default().fg(WHITE).add_modifier(Modifier::BOLD))),
        rows[0],
    );

    let t = app.analysis.overall.totals;
    let tiles = [
        ("Processed tokens", fmt_tokens(t.grand_total())),
        ("Cached input", fmt_tokens(t.cached_input)),
        ("Uncached input", fmt_tokens(t.uncached_input)),
        ("Output", fmt_tokens(t.output)),
        ("Cache savings", fmt_usd(app.analysis.overall.cache_savings_usd)),
    ];

    let cells = Layout::horizontal([Constraint::Ratio(1, 5); 5]).spacing(1).split(rows[1]);
    for (cell, (label, value)) in cells.iter().zip(tiles) {
        let block = Paragraph::new(vec![
            Line::from(Span::styled(label, Style::default().fg(MUTED))),
            Line::from(Span::styled(value, Style::default().fg(WHITE).add_modifier(Modifier::BOLD))),
        ]);
        frame.render_widget(block, *cell);
    }
}

// ---- breakdown ----------------------------------------------------------

fn draw_breakdown(frame: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(0)]).split(area);

    // Title with the Model/Day/Project/Session toggle on the right.
    let title_cols = Layout::horizontal([Constraint::Percentage(40), Constraint::Min(0)]).split(rows[0]);
    frame.render_widget(
        Paragraph::new(Span::styled("Breakdown", Style::default().fg(WHITE).add_modifier(Modifier::BOLD))),
        title_cols[0],
    );
    let mut toggle = Vec::new();
    for mode in Breakdown::ALL {
        toggle.push(pill(mode.label(), app.breakdown == mode));
        toggle.push(Span::raw(" "));
    }
    frame.render_widget(Paragraph::new(Line::from(toggle)).alignment(Alignment::Right), title_cols[1]);

    // Column headers depend on the active metric.
    let (primary, secondary) = match app.metric {
        Metric::Cost => ("Cost", "Tokens"),
        Metric::Tokens => ("Tokens", "Cost"),
    };
    let header = Row::new(vec![
        muted_cell(app.breakdown.label(), Alignment::Left),
        muted_cell(primary, Alignment::Right),
        muted_cell("Share", Alignment::Right),
        muted_cell(secondary, Alignment::Right),
    ]);

    let widths = [Constraint::Min(20), Constraint::Length(14), Constraint::Length(10), Constraint::Length(14)];
    let table = Table::new(breakdown_rows(app), widths).header(header).column_spacing(2);
    frame.render_widget(table, rows[2]);
}

/// One breakdown entry, before formatting into a row.
struct Entry {
    name: Line<'static>,
    cost: f64,
    tokens: u64,
}

fn breakdown_rows(app: &App) -> Vec<Row<'static>> {
    let a = &app.analysis;
    let mut entries: Vec<Entry> = match app.breakdown {
        Breakdown::Model => a.by_model.iter().map(|(k, g)| named_entry(starred(k), g)).collect(),
        Breakdown::Project => a.by_project.iter().map(|(k, g)| named_entry(plain(short_project(k)), g)).collect(),
        Breakdown::Session => a.by_session.iter().map(|(k, g)| named_entry(plain(short_id(k)), g)).collect(),
        Breakdown::Day => a.by_day.iter().map(|(k, g)| named_entry(plain(pretty_day(k)), g)).collect(),
    };

    // Day reads chronologically; every other dimension ranks by the active metric.
    match (app.breakdown, app.metric) {
        (Breakdown::Day, _) => {}
        (_, Metric::Cost) => entries.sort_by(|x, y| y.cost.total_cmp(&x.cost)),
        (_, Metric::Tokens) => entries.sort_by_key(|e| std::cmp::Reverse(e.tokens)),
    }

    let total_cost = a.overall.cost_usd.max(f64::MIN_POSITIVE);
    let total_tokens = a.overall.totals.grand_total().max(1) as f64;

    entries
        .into_iter()
        .take(200)
        .map(|e| {
            let (primary, share, secondary) = match app.metric {
                Metric::Cost => (fmt_usd(e.cost), e.cost / total_cost, fmt_tokens(e.tokens)),
                Metric::Tokens => (fmt_tokens(e.tokens), e.tokens as f64 / total_tokens, fmt_usd(e.cost)),
            };
            Row::new(vec![
                Cell::from(e.name),
                value_cell(primary),
                value_cell(format!("{:.1}%", share * 100.0)),
                value_cell(secondary),
            ])
        })
        .collect()
}

fn named_entry(name: Line<'static>, g: &Group) -> Entry {
    Entry { name, cost: g.cost_usd, tokens: g.totals.grand_total() }
}

fn starred(model: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(STAR, Style::default().fg(ORANGE)),
        Span::styled(model.to_string(), Style::default().fg(WHITE)),
    ])
}

fn plain(text: String) -> Line<'static> {
    Line::from(Span::styled(text, Style::default().fg(WHITE)))
}

fn muted_cell(text: &str, align: Alignment) -> Cell<'static> {
    Cell::from(Text::from(text.to_string()).alignment(align).style(Style::default().fg(MUTED)))
}

fn value_cell(text: String) -> Cell<'static> {
    Cell::from(Text::from(text).alignment(Alignment::Right).style(Style::default().fg(WHITE)))
}

// ---- footer -------------------------------------------------------------

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let text = Line::from(vec![
        Span::styled(" ←/→", Style::default().fg(ORANGE)),
        Span::styled(" breakdown  ", Style::default().fg(FAINT)),
        Span::styled("1-5", Style::default().fg(ORANGE)),
        Span::styled(" range  ", Style::default().fg(FAINT)),
        Span::styled("space", Style::default().fg(ORANGE)),
        Span::styled(" cost/tokens  ", Style::default().fg(FAINT)),
        Span::styled("q", Style::default().fg(ORANGE)),
        Span::styled(" quit    ", Style::default().fg(FAINT)),
        Span::styled(
            format!("prices: {} · {} · {}", app.source.label(), app.tz_label, app.updated_at),
            Style::default().fg(FAINT),
        ),
    ]);
    frame.render_widget(Paragraph::new(text), area);
}

// ---- helpers ------------------------------------------------------------

fn metric_value(metric: Metric, g: &Group) -> f64 {
    match metric {
        Metric::Cost => g.cost_usd,
        Metric::Tokens => g.totals.grand_total() as f64,
    }
}

fn fmt_usd(value: f64) -> String {
    if value >= 1000.0 {
        // Thousands separator, to echo "$2,129.07".
        let whole = value.trunc() as i64;
        let cents = ((value - whole as f64) * 100.0).round() as i64;
        format!("${}.{:02}", group_thousands(whole), cents)
    } else {
        format!("${value:.2}")
    }
}

fn group_thousands(mut n: i64) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let mut parts = Vec::new();
    while n > 0 {
        parts.push(format!("{:03}", n % 1000));
        n /= 1000;
    }
    parts.reverse();
    let mut joined = parts.join(",");
    // Trim leading zeros of the most-significant group.
    while joined.starts_with('0') && joined.len() > 1 && joined.as_bytes()[1] != b',' {
        joined.remove(0);
    }
    joined
}

/// Compact token count with the reference UI's graded precision: `491M`,
/// `3.07M`, `6.51K`, `1.23B`.
fn fmt_tokens(n: u64) -> String {
    let f = n as f64;
    if f >= 1e9 {
        format!("{:.2}B", f / 1e9)
    } else if f >= 1e6 {
        let m = f / 1e6;
        if m >= 100.0 {
            format!("{m:.0}M")
        } else if m >= 10.0 {
            format!("{m:.1}M")
        } else {
            format!("{m:.2}M")
        }
    } else if f >= 1e3 {
        format!("{:.2}K", f / 1e3)
    } else {
        format!("{n}")
    }
}

fn fmt_axis(metric: Metric, value: f64) -> String {
    match metric {
        Metric::Cost => format!("${value:.2}"),
        Metric::Tokens => fmt_tokens(value as u64),
    }
}

const MONTHS: [&str; 12] =
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// `2026-09-10` -> `Sep 10`.
fn pretty_day(day: &str) -> String {
    let (m, d) = month_day(day);
    format!("{m} {d}")
}

/// `2026-09-10` -> `SEP 10` (for chart axes).
fn upper_day(day: &str) -> String {
    let (m, d) = month_day(day);
    format!("{} {d}", m.to_uppercase())
}

fn month_day(day: &str) -> (&'static str, &str) {
    let month = day
        .get(5..7)
        .and_then(|mm| mm.parse::<usize>().ok())
        .and_then(|mm| MONTHS.get(mm.wrapping_sub(1)).copied())
        .unwrap_or("");
    let d = day.get(8..10).unwrap_or(day);
    (month, d)
}

fn date_range_label(app: &App) -> String {
    let mut days = app.analysis.by_day.keys();
    match (days.next(), app.analysis.by_day.keys().last()) {
        (Some(first), Some(last)) if first != last => format!("{} to {}", pretty_day(first), pretty_day(last)),
        (Some(only), _) => pretty_day(only),
        _ => app.range.label().to_string(),
    }
}

/// Keeps a project path readable in a narrow column: its last two segments.
fn short_project(path: &str) -> String {
    let parts: Vec<&str> = path.rsplit('/').filter(|s| !s.is_empty()).take(2).collect();
    if parts.is_empty() {
        return path.to_string();
    }
    let tail: Vec<&str> = parts.into_iter().rev().collect();
    format!(".../{}", tail.join("/"))
}

/// First 8 characters of a session id — enough to tell them apart.
fn short_id(id: &str) -> String {
    id.get(..8).unwrap_or(id).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::analysis::analyze;
    use crate::core::pricing::parse_rate_table;
    use crate::core::record::{TokenTotals, UsageRecord};
    use crate::shell::prices::PriceSource;
    use crate::tui::app::{Breakdown, Metric};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn sample_app() -> App {
        let table = parse_rate_table(&serde_json::json!({
            "claude-opus-4-8": { "input_cost_per_token": 1e-6, "output_cost_per_token": 2e-6 }
        }));
        let recs = [
            UsageRecord {
                timestamp_ms: 0,
                model: "claude-opus-4-8".into(),
                session_id: "session-abcdef".into(),
                project: "/home/me/proj".into(),
                totals: TokenTotals { uncached_input: 100, cached_input: 5000, cache_creation: 200, output: 400 },
                reported_cost_usd: None,
                dedupe_key: Some("m1:r1".into()),
            },
            UsageRecord {
                timestamp_ms: 90_000_000,
                model: "claude-sonnet-5".into(),
                session_id: "session-ghijkl".into(),
                project: "/home/me/other".into(),
                totals: TokenTotals { uncached_input: 50, cached_input: 1000, cache_creation: 0, output: 200 },
                reported_cost_usd: None,
                dedupe_key: Some("m2:r2".into()),
            },
        ];
        let mut app = App::new(PriceSource::Bundled, "UTC".into());
        app.analysis = analyze(recs.iter(), &table, |ms| if ms == 0 { "2026-09-09".into() } else { "2026-09-10".into() });
        app
    }

    #[test]
    fn renders_every_mode_at_many_sizes() {
        for breakdown in Breakdown::ALL {
            for metric in [Metric::Cost, Metric::Tokens] {
                for (w, h) in [(120, 40), (84, 26), (40, 12), (16, 6)] {
                    let mut app = sample_app();
                    app.breakdown = breakdown;
                    app.metric = metric;
                    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
                    term.draw(|f| draw(f, &app)).unwrap();
                }
            }
        }
    }

    #[test]
    fn renders_with_no_data() {
        let app = App::new(PriceSource::Bundled, "UTC".into());
        let mut term = Terminal::new(TestBackend::new(84, 26)).unwrap();
        term.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn groups_thousands_like_the_reference() {
        assert_eq!(fmt_usd(2129.07), "$2,129.07");
        assert_eq!(fmt_usd(398.62), "$398.62");
        assert_eq!(group_thousands(1_234_567), "1,234,567");
    }
}
