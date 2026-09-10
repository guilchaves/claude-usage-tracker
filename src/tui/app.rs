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

/// The time window the figures cover, mirroring the reference dashboard's
/// selector (Past 24h / 7 / 30 / 90 days), plus an all-time option.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Range {
    Day,
    Week,
    Month,
    Quarter,
    All,
}

impl Range {
    pub const ALL: [Range; 5] = [Range::Day, Range::Week, Range::Month, Range::Quarter, Range::All];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Range::Day => "Past 24h",
            Range::Week => "7 days",
            Range::Month => "30 days",
            Range::Quarter => "90 days",
            Range::All => "All",
        }
    }

    /// The earliest timestamp (Unix ms) inside this rolling window.
    #[must_use]
    pub fn cutoff_ms(self, now_ms: i64) -> i64 {
        match self {
            Range::Day => now_ms - DAY_MS,
            Range::Week => now_ms - 7 * DAY_MS,
            Range::Month => now_ms - 30 * DAY_MS,
            Range::Quarter => now_ms - 90 * DAY_MS,
            Range::All => i64::MIN,
        }
    }
}

/// Whether the dashboard leads with dollars or tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Metric {
    Cost,
    Tokens,
}

impl Metric {
    #[must_use]
    pub fn toggled(self) -> Metric {
        match self {
            Metric::Cost => Metric::Tokens,
            Metric::Tokens => Metric::Cost,
        }
    }
}

/// Which dimension the bottom breakdown table groups by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Breakdown {
    Model,
    Day,
    Project,
    Session,
}

impl Breakdown {
    pub const ALL: [Breakdown; 4] = [Breakdown::Model, Breakdown::Day, Breakdown::Project, Breakdown::Session];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Breakdown::Model => "Model",
            Breakdown::Day => "Day",
            Breakdown::Project => "Project",
            Breakdown::Session => "Session",
        }
    }

    #[must_use]
    pub fn index(self) -> usize {
        Breakdown::ALL.iter().position(|&b| b == self).unwrap_or(0)
    }

    #[must_use]
    fn step(self, delta: isize) -> Breakdown {
        let len = Breakdown::ALL.len() as isize;
        let next = (self.index() as isize + delta).rem_euclid(len);
        Breakdown::ALL[next as usize]
    }
}

/// What a keystroke asks the run loop to do next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    None,
    Recompute,
    Quit,
}

/// A logical key, decoupled from crossterm so `on_key` is testable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Quit,
    NextBreakdown,
    PrevBreakdown,
    Range(Range),
    ToggleMetric,
    Refresh,
    Other,
}

/// The dashboard's mutable view state.
pub struct App {
    pub analysis: Analysis,
    pub source: PriceSource,
    pub range: Range,
    pub metric: Metric,
    pub breakdown: Breakdown,
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
            range: Range::Month, // matches the reference default (30 days)
            metric: Metric::Cost,
            breakdown: Breakdown::Model,
            tz_label,
            updated_at: String::from("—"),
        }
    }

    /// Applies a key and reports what the run loop should do. Only a range
    /// change needs a re-fold; metric and breakdown are derived from the
    /// analysis already in hand.
    pub fn on_key(&mut self, key: Key) -> Outcome {
        match key {
            Key::Quit => Outcome::Quit,
            Key::NextBreakdown => {
                self.breakdown = self.breakdown.step(1);
                Outcome::None
            }
            Key::PrevBreakdown => {
                self.breakdown = self.breakdown.step(-1);
                Outcome::None
            }
            Key::ToggleMetric => {
                self.metric = self.metric.toggled();
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
    fn breakdown_stepping_wraps_both_ways() {
        assert_eq!(Breakdown::Model.step(-1), Breakdown::Session);
        assert_eq!(Breakdown::Session.step(1), Breakdown::Model);
    }

    #[test]
    fn metric_and_breakdown_need_no_recompute_but_range_does() {
        let mut app = App::new(PriceSource::Bundled, "UTC".into());
        assert_eq!(app.on_key(Key::ToggleMetric), Outcome::None);
        assert_eq!(app.metric, Metric::Tokens);
        assert_eq!(app.on_key(Key::NextBreakdown), Outcome::None);
        assert_eq!(app.breakdown, Breakdown::Day);
        assert_eq!(app.on_key(Key::Range(Range::Week)), Outcome::Recompute);
        assert_eq!(app.on_key(Key::Range(Range::Week)), Outcome::None); // unchanged
        assert_eq!(app.on_key(Key::Quit), Outcome::Quit);
    }

    #[test]
    fn range_cutoffs_are_ordered() {
        let now = 1_000 * DAY_MS;
        assert_eq!(Range::All.cutoff_ms(now), i64::MIN);
        assert!(Range::Quarter.cutoff_ms(now) < Range::Month.cutoff_ms(now));
        assert!(Range::Month.cutoff_ms(now) < Range::Week.cutoff_ms(now));
        assert!(Range::Week.cutoff_ms(now) < Range::Day.cutoff_ms(now));
    }
}
