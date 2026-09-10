//! View state and input handling for the dashboard.
//!
//! The heavy lifting (scanning, analysis) happens in the run loop; `App` holds
//! only what the screen needs and how keystrokes change it. Keeping `on_key`
//! pure over `App` — it returns an [`Outcome`] rather than doing I/O — keeps the
//! interaction rules testable.

use crate::core::analysis::Analysis;
use crate::shell::prices::PriceSource;

/// Milliseconds in a day, for range cutoffs.
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// The dashboard's tabs. Overview leads; the rest are breakdowns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Model,
    Project,
    Session,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Overview, Tab::Model, Tab::Project, Tab::Session];

    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Model => "By model",
            Tab::Project => "By project",
            Tab::Session => "Session",
        }
    }

    #[must_use]
    pub fn index(self) -> usize {
        Tab::ALL.iter().position(|&t| t == self).unwrap_or(0)
    }

    #[must_use]
    fn step(self, delta: isize) -> Tab {
        let len = Tab::ALL.len() as isize;
        let next = (self.index() as isize + delta).rem_euclid(len);
        Tab::ALL[next as usize]
    }
}

/// The time window the figures cover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Range {
    Today,
    Week,
    Month,
    All,
}

impl Range {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Range::Today => "Today",
            Range::Week => "7 days",
            Range::Month => "30 days",
            Range::All => "All time",
        }
    }

    /// The earliest timestamp (Unix ms) that falls inside this range.
    ///
    /// `start_of_today_ms` is supplied by the shell because "the start of
    /// today" depends on the timezone, which the core does not read.
    #[must_use]
    pub fn cutoff_ms(self, now_ms: i64, start_of_today_ms: i64) -> i64 {
        match self {
            Range::Today => start_of_today_ms,
            Range::Week => now_ms - 7 * DAY_MS,
            Range::Month => now_ms - 30 * DAY_MS,
            Range::All => i64::MIN,
        }
    }
}

/// What a keystroke asks the run loop to do next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing further; just redraw.
    None,
    /// The window changed; re-fold the records.
    Recompute,
    /// Leave.
    Quit,
}

/// A logical key, decoupled from crossterm so `on_key` is testable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Quit,
    NextTab,
    PrevTab,
    Range(Range),
    Refresh,
    Other,
}

/// The dashboard's mutable view state.
pub struct App {
    pub analysis: Analysis,
    pub source: PriceSource,
    pub tab: Tab,
    pub range: Range,
    pub tz_label: String,
    /// Wall-clock of the last refresh, `HH:MM:SS`, for the footer.
    pub updated_at: String,
}

impl App {
    #[must_use]
    pub fn new(source: PriceSource, tz_label: String) -> Self {
        App {
            analysis: Analysis::default(),
            source,
            tab: Tab::Overview,
            range: Range::All,
            tz_label,
            updated_at: String::from("—"),
        }
    }

    /// Applies a key and reports what the run loop should do.
    pub fn on_key(&mut self, key: Key) -> Outcome {
        match key {
            Key::Quit => Outcome::Quit,
            Key::NextTab => {
                self.tab = self.tab.step(1);
                Outcome::None
            }
            Key::PrevTab => {
                self.tab = self.tab.step(-1);
                Outcome::None
            }
            Key::Range(range) => {
                if self.range == range {
                    Outcome::None
                } else {
                    self.range = range;
                    Outcome::Recompute
                }
            }
            Key::Refresh => Outcome::Recompute,
            Key::Other => Outcome::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_stepping_wraps_both_ways() {
        assert_eq!(Tab::Overview.step(-1), Tab::Session);
        assert_eq!(Tab::Session.step(1), Tab::Overview);
    }

    #[test]
    fn switching_tab_needs_no_recompute_but_changing_range_does() {
        let mut app = App::new(PriceSource::Bundled, "UTC".into());
        assert_eq!(app.on_key(Key::NextTab), Outcome::None);
        assert_eq!(app.tab, Tab::Model);
        assert_eq!(app.on_key(Key::Range(Range::Week)), Outcome::Recompute);
        assert_eq!(app.on_key(Key::Range(Range::Week)), Outcome::None); // unchanged
        assert_eq!(app.on_key(Key::Quit), Outcome::Quit);
    }

    #[test]
    fn range_cutoffs_are_ordered() {
        let now = 1_000 * DAY_MS;
        let sot = now - DAY_MS / 2;
        assert_eq!(Range::All.cutoff_ms(now, sot), i64::MIN);
        assert!(Range::Month.cutoff_ms(now, sot) < Range::Week.cutoff_ms(now, sot));
        assert!(Range::Week.cutoff_ms(now, sot) < Range::Today.cutoff_ms(now, sot));
    }
}
