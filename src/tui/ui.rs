//! Rendering — a pure function of [`App`] onto a ratatui frame.
//!
//! Nothing here mutates state or does I/O; given the same `App` it draws the
//! same screen, which keeps the layout easy to reason about in isolation.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Tabs};
use ratatui::Frame;

use crate::core::analysis::{Analysis, Line as Group};
use crate::tui::app::{App, Tab};

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;
const GOOD: Color = Color::Green;

/// Draws the whole dashboard for the current `app`.
pub fn draw(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(1)])
        .split(frame.area());

    draw_tabs(frame, chunks[0], app);
    match app.tab {
        Tab::Overview => draw_overview(frame, chunks[1], app),
        Tab::Model => draw_breakdown(frame, chunks[1], app, "MODEL", model_rows(&app.analysis)),
        Tab::Project => draw_breakdown(frame, chunks[1], app, "PROJECT", project_rows(&app.analysis)),
        Tab::Session => draw_session(frame, chunks[1], app),
    }
    draw_footer(frame, chunks[2], app);
}

fn draw_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = Tab::ALL.iter().map(|t| Line::from(t.title())).collect();
    let tabs = Tabs::new(titles)
        .select(app.tab.index())
        .block(Block::default().borders(Borders::ALL).title(" Claude Code Usage "))
        .highlight_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .divider("│");
    frame.render_widget(tabs, area);
}

fn draw_overview(frame: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(0)])
        .split(area);

    draw_headline(frame, rows[0], app);
    draw_daily_bars(frame, rows[1], app);
}

fn draw_headline(frame: &mut Frame, area: Rect, app: &App) {
    let o = &app.analysis.overall;
    let big = Style::default().fg(ACCENT).add_modifier(Modifier::BOLD);

    let mut lines = vec![
        Line::from(vec![
            Span::raw("  Estimate "),
            Span::styled(fmt_usd(o.cost_usd), big),
            Span::raw("      Cache saved "),
            Span::styled(fmt_usd(o.cache_savings_usd), Style::default().fg(GOOD)),
        ]),
        Line::from(vec![
            Span::raw("  Tokens "),
            Span::styled(fmt_tokens(o.totals.grand_total()), Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(format!("   Turns {}", o.records)),
            Span::styled(
                format!("   Duplicates dropped {}", app.analysis.duplicates_dropped),
                Style::default().fg(MUTED),
            ),
        ]),
    ];
    if o.unpriced_records > 0 {
        lines.push(Line::from(Span::styled(
            format!("  {} turns could not be priced (unknown model)", o.unpriced_records),
            Style::default().fg(Color::Yellow),
        )));
    }

    let block = Block::default().borders(Borders::ALL).title(format!(" {} ", app.range.label()));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_daily_bars(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().borders(Borders::ALL).title(" Daily estimate ");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let days: Vec<(&String, &Group)> = app.analysis.by_day.iter().collect();
    if days.is_empty() {
        frame.render_widget(Paragraph::new("  no usage in range").style(Style::default().fg(MUTED)), inner);
        return;
    }

    let visible = days.iter().rev().take(inner.height as usize).rev();
    let max_cost = days.iter().map(|(_, g)| g.cost_usd).fold(0.0_f64, f64::max).max(f64::MIN_POSITIVE);

    // Reserve columns for the "MM-DD " prefix and " $amount" suffix.
    let prefix = 7usize;
    let suffix = 12usize;
    let bar_width = (inner.width as usize).saturating_sub(prefix + suffix).max(1);

    let lines: Vec<Line> = visible
        .map(|(day, group)| {
            let filled = ((group.cost_usd / max_cost) * bar_width as f64).round() as usize;
            let bar = "█".repeat(filled.min(bar_width));
            Line::from(vec![
                Span::styled(format!("{} ", short_day(day)), Style::default().fg(MUTED)),
                Span::styled(format!("{bar:<bar_width$}"), Style::default().fg(ACCENT)),
                Span::raw(format!(" {:>10}", fmt_usd(group.cost_usd))),
            ])
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), inner);
}

fn draw_breakdown(frame: &mut Frame, area: Rect, app: &App, first_col: &str, rows: Vec<Row<'static>>) {
    let header = Row::new(vec![
        Cell::from(first_col.to_string()),
        Cell::from("COST"),
        Cell::from("TURNS"),
        Cell::from("TOKENS"),
        Cell::from("CACHE SAVED"),
    ])
    .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));

    let widths = [
        Constraint::Min(20),
        Constraint::Length(12),
        Constraint::Length(8),
        Constraint::Length(12),
        Constraint::Length(13),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(format!(" {} — {} ", app.tab.title(), app.range.label())));
    frame.render_widget(table, area);
}

fn draw_session(frame: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(0)])
        .split(area);

    let block = Block::default().borders(Borders::ALL).title(" Current session (most recent) ");
    let inner = block.inner(rows[0]);
    frame.render_widget(block, rows[0]);

    match app.analysis.current_session() {
        Some((id, g)) => {
            let lines = vec![
                Line::from(vec![
                    Span::styled(
                        format!("  {}   ", fmt_usd(g.cost_usd)),
                        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!("{} turns", g.records), Style::default().fg(MUTED)),
                ]),
                Line::from(Span::styled(
                    format!(
                        "  {} in · {} out · {} cache read · session {}",
                        fmt_tokens(g.totals.uncached_input),
                        fmt_tokens(g.totals.output),
                        fmt_tokens(g.totals.cached_input),
                        short_id(id),
                    ),
                    Style::default().fg(MUTED),
                )),
            ];
            frame.render_widget(Paragraph::new(lines), inner);
        }
        None => {
            frame.render_widget(Paragraph::new("  no active session in range").style(Style::default().fg(MUTED)), inner);
        }
    }

    draw_breakdown_at(frame, rows[1], app, "SESSION", session_rows(&app.analysis));
}

/// Like [`draw_breakdown`] but for an already-chosen area (used by the session tab).
fn draw_breakdown_at(frame: &mut Frame, area: Rect, _app: &App, first_col: &str, rows: Vec<Row<'static>>) {
    let header = Row::new(vec![
        Cell::from(first_col.to_string()),
        Cell::from("COST"),
        Cell::from("TURNS"),
        Cell::from("TOKENS"),
    ])
    .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD));
    let widths = [Constraint::Min(20), Constraint::Length(12), Constraint::Length(8), Constraint::Length(12)];
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(" Recent sessions "));
    frame.render_widget(table, area);
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let text = Line::from(vec![
        Span::styled(" ←/→", Style::default().fg(ACCENT)),
        Span::raw(" tabs  "),
        Span::styled("t/w/m/a", Style::default().fg(ACCENT)),
        Span::raw(" range  "),
        Span::styled("r", Style::default().fg(ACCENT)),
        Span::raw(" refresh  "),
        Span::styled("q", Style::default().fg(ACCENT)),
        Span::raw(" quit    "),
        Span::styled(
            format!("prices: {} · tz: {} · updated {}", app.source.label(), app.tz_label, app.updated_at),
            Style::default().fg(MUTED),
        ),
    ]);
    frame.render_widget(Paragraph::new(text).alignment(Alignment::Left), area);
}

// ---- row builders -------------------------------------------------------

/// Sorts a keyed breakdown by cost descending into full (5-column) rows.
fn ranked_rows(map: &std::collections::BTreeMap<String, Group>, label: impl Fn(&str) -> String) -> Vec<Row<'static>> {
    let mut entries: Vec<(&String, &Group)> = map.iter().collect();
    entries.sort_by(|a, b| b.1.cost_usd.total_cmp(&a.1.cost_usd));
    entries
        .into_iter()
        .map(|(key, g)| {
            Row::new(vec![
                Cell::from(label(key)),
                Cell::from(fmt_usd(g.cost_usd)),
                Cell::from(g.records.to_string()),
                Cell::from(fmt_tokens(g.totals.grand_total())),
                Cell::from(fmt_usd(g.cache_savings_usd)),
            ])
        })
        .collect()
}

fn model_rows(a: &Analysis) -> Vec<Row<'static>> {
    ranked_rows(&a.by_model, |k| k.to_string())
}

fn project_rows(a: &Analysis) -> Vec<Row<'static>> {
    ranked_rows(&a.by_project, short_project)
}

/// Sessions as 4-column rows, cost descending.
fn session_rows(a: &Analysis) -> Vec<Row<'static>> {
    let mut entries: Vec<(&String, &Group)> = a.by_session.iter().collect();
    entries.sort_by(|x, y| y.1.cost_usd.total_cmp(&x.1.cost_usd));
    entries
        .into_iter()
        .take(200)
        .map(|(id, g)| {
            Row::new(vec![
                Cell::from(short_id(id)),
                Cell::from(fmt_usd(g.cost_usd)),
                Cell::from(g.records.to_string()),
                Cell::from(fmt_tokens(g.totals.grand_total())),
            ])
        })
        .collect()
}

// ---- formatting ---------------------------------------------------------

fn fmt_usd(value: f64) -> String {
    format!("${value:.2}")
}

fn fmt_tokens(n: u64) -> String {
    let n = n as f64;
    if n >= 1e9 {
        format!("{:.1}B", n / 1e9)
    } else if n >= 1e6 {
        format!("{:.1}M", n / 1e6)
    } else if n >= 1e3 {
        format!("{:.1}K", n / 1e3)
    } else {
        format!("{n:.0}")
    }
}

/// `2026-09-10` -> `09-10`.
fn short_day(day: &str) -> String {
    day.get(5..).unwrap_or(day).to_string()
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
    use crate::tui::app::Tab;
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
                dedupe_key: Some("m:r".into()),
            },
        ];
        let mut app = App::new(PriceSource::Bundled, "UTC".into());
        app.analysis = analyze(recs.iter(), &table, |_| "2026-09-10".to_string());
        app
    }

    /// Every tab renders at a range of sizes — including tiny and huge — without
    /// panicking on slicing or width arithmetic.
    #[test]
    fn renders_all_tabs_at_many_sizes() {
        for tab in Tab::ALL {
            for (w, h) in [(80, 24), (20, 6), (200, 60), (12, 4)] {
                let mut app = sample_app();
                app.tab = tab;
                let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
                term.draw(|f| draw(f, &app)).unwrap();
            }
        }
    }

    /// The empty-data paths (no days, no session) also render cleanly.
    #[test]
    fn renders_with_no_data() {
        let app = App::new(PriceSource::Bundled, "UTC".into());
        for tab in Tab::ALL {
            let mut app = App { tab, ..app_clone(&app) };
            let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
            term.draw(|f| draw(f, &app)).unwrap();
            let _ = &mut app;
        }
    }

    fn app_clone(app: &App) -> App {
        App {
            analysis: app.analysis.clone(),
            source: app.source,
            tab: app.tab,
            range: app.range,
            tz_label: app.tz_label.clone(),
            updated_at: app.updated_at.clone(),
        }
    }
}
