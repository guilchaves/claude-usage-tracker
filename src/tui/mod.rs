//! The terminal run loop — the imperative shell that drives the UI.
//!
//! It owns the scanner, the rates, and the terminal, and on each pass it does
//! the impure work (refresh the scan, read the clock) then hands immutable data
//! to the pure core ([`analyze`]) and the pure renderer ([`ui::draw`]). Input is
//! translated to a logical [`app::Key`] so [`App::on_key`] stays testable.

pub mod app;
pub mod ui;

use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::core::analysis::analyze;
use crate::shell::config::{clock_hms, now_ms, Config};
use crate::shell::prices::Prices;
use crate::shell::scan::Scanner;
use app::{App, Breakdown, Key, Outcome, Range};

/// How often the scanner is re-polled. Cheap because unchanged files are
/// skipped by size and changed files read only their appended tail.
const REFRESH_EVERY: Duration = Duration::from_millis(1000);

/// How long to block waiting for input before looping (bounds redraw latency).
const INPUT_POLL: Duration = Duration::from_millis(250);

/// Runs the dashboard until the user quits, restoring the terminal on the way out.
pub fn run(config: Config, prices: Prices, mut scanner: Scanner) -> io::Result<()> {
    let mut terminal = setup_terminal()?;
    let mut app = App::new(prices.source, crate::shell::config::zone_label(&config.tz));

    let mut last_refresh: Option<Instant> = None;
    loop {
        let due = last_refresh.is_none_or(|t| t.elapsed() >= REFRESH_EVERY);
        if due {
            scanner.refresh();
            recompute(&mut app, &scanner, &config, &prices);
            app.updated_at = clock_hms(&config.tz);
            last_refresh = Some(Instant::now());
        }

        terminal.draw(|frame| ui::draw(frame, &app))?;

        if event::poll(INPUT_POLL)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match app.on_key(to_key(key.code)) {
                        Outcome::Quit => break,
                        Outcome::Recompute => {
                            recompute(&mut app, &scanner, &config, &prices);
                            app.updated_at = clock_hms(&config.tz);
                        }
                        Outcome::None => {}
                    }
                }
            }
        }
    }

    restore_terminal(&mut terminal)
}

/// Re-folds the scanned records for the app's current time range. All the
/// impurity (the clock, the timezone) is resolved here and handed to the pure
/// core as plain values.
fn recompute(app: &mut App, scanner: &Scanner, config: &Config, prices: &Prices) {
    let cutoff = app.range.cutoff_ms(now_ms());
    let to_day = config.day_mapper();
    app.analysis = analyze(
        scanner.records().filter(|record| record.timestamp_ms >= cutoff),
        &prices.table,
        to_day,
    );
}

/// Maps a physical key to a logical one. `←/→` and `Tab` move between tabs;
/// `t/w/m/a` pick a range; `r` refreshes; `q`/`Esc` quit.
fn to_key(code: KeyCode) -> Key {
    match code {
        KeyCode::Char('q') | KeyCode::Esc => Key::Quit,
        KeyCode::Right | KeyCode::Tab | KeyCode::Char('l') => Key::NextBreakdown,
        KeyCode::Left | KeyCode::BackTab | KeyCode::Char('h') => Key::PrevBreakdown,
        KeyCode::Char('1') => Key::Range(Range::Day),
        KeyCode::Char('2') => Key::Range(Range::Week),
        KeyCode::Char('3') => Key::Range(Range::Month),
        KeyCode::Char('4') => Key::Range(Range::Quarter),
        KeyCode::Char('5') => Key::Range(Range::All),
        KeyCode::Char(' ') | KeyCode::Char('x') => Key::ToggleMetric,
        KeyCode::Char('r') => Key::Refresh,
        _ => Key::Other,
    }
}

/// Renders a single frame into an off-screen buffer and returns it as text.
/// Used by `--render` to preview the dashboard without a live terminal.
pub fn render_to_string(config: &Config, prices: &Prices, scanner: &Scanner, breakdown_index: usize, w: u16, h: u16) -> String {
    let mut app = App::new(prices.source, crate::shell::config::zone_label(&config.tz));
    app.breakdown = Breakdown::ALL[breakdown_index % Breakdown::ALL.len()];
    app.updated_at = clock_hms(&config.tz);
    recompute(&mut app, scanner, config, prices);

    let backend = ratatui::backend::TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).expect("test backend");
    terminal.draw(|frame| ui::draw(frame, &app)).expect("draw");

    let buffer = terminal.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..h {
        for x in 0..w {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()
}
