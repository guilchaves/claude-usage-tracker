//! The one pure fold: a stream of records -> every breakdown the UI shows.
//!
//! De-duplication is global and happens exactly once here. Claude Code copies a
//! turn's record forward when a session is resumed or forked, so the same
//! dedupe key legitimately appears across transcripts; only the first sighting
//! counts. Each surviving turn is priced once and added into every breakdown.
//!
//! The day a turn belongs to depends on a timezone — an ambient fact the core
//! refuses to read — so the caller injects `to_day`. That is the only seam to
//! the outside world, and it is a pure function of the timestamp, leaving the
//! whole fold deterministic and unit-testable.

use std::collections::{BTreeMap, HashSet};

use super::pricing::{cache_savings_usd, price_usage, CostSource, RateTable};
use super::record::{TokenTotals, UsageRecord};

/// A calendar day key, `YYYY-MM-DD`, resolved in the caller's timezone.
pub type Day = String;

/// Accumulated usage for one group (overall, a model, a day, a project, …).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Line {
    pub totals: TokenTotals,
    pub cost_usd: f64,
    pub cache_savings_usd: f64,
    pub records: u64,
    /// Turns that matched no rate — surfaced so the UI can flag partial data.
    pub unpriced_records: u64,
}

impl Line {
    fn absorb(&mut self, totals: TokenTotals, cost_usd: f64, cache_savings_usd: f64, unpriced: u64) {
        self.totals = self.totals.add(totals);
        self.cost_usd += cost_usd;
        self.cache_savings_usd += cache_savings_usd;
        self.records += 1;
        self.unpriced_records += unpriced;
    }
}

/// Every breakdown the dashboard renders, derived in one pass.
#[derive(Clone, Debug, Default)]
pub struct Analysis {
    /// Headline figures across the whole window.
    pub overall: Line,
    /// Per-model rows, keyed by model id.
    pub by_model: BTreeMap<String, Line>,
    /// Per-day rows in date order — the trend series.
    pub by_day: BTreeMap<Day, Line>,
    /// Per-project rows, keyed by working directory.
    pub by_project: BTreeMap<String, Line>,
    /// Per-session rows, keyed by session id.
    pub by_session: BTreeMap<String, Line>,
    /// The session with the most recent turn — the "current" session for the
    /// live meter. `None` when no turn carried a session id.
    pub latest_session: Option<String>,
    /// Timestamp (Unix ms) of the most recent turn overall, if any.
    pub latest_activity_ms: Option<i64>,
    /// Turns skipped because an earlier turn shared their dedupe key.
    pub duplicates_dropped: u64,
}

/// The label used for turns whose working directory the transcript omitted.
pub const UNKNOWN_PROJECT: &str = "(unknown)";

/// Folds `records` into an [`Analysis`], pricing each surviving turn with
/// `table` and bucketing its day with `to_day`.
///
/// Takes borrowed records by iterator so a caller holding several per-file
/// record lists can fold them together without concatenating or cloning.
#[must_use]
pub fn analyze<'r, I, F>(records: I, table: &RateTable, to_day: F) -> Analysis
where
    I: IntoIterator<Item = &'r UsageRecord>,
    F: Fn(i64) -> Day,
{
    let mut seen: HashSet<&str> = HashSet::new();
    let mut analysis = Analysis::default();
    let mut latest_ms = i64::MIN;

    for record in records {
        if let Some(key) = record.dedupe_key.as_deref() {
            if !seen.insert(key) {
                analysis.duplicates_dropped += 1;
                continue;
            }
        }

        let priced = price_usage(table, &record.model, record.totals, record.reported_cost_usd);
        let savings = cache_savings_usd(table, &record.model, record.totals);
        let unpriced = u64::from(priced.source == CostSource::Unpriced);
        let absorb = |line: &mut Line| line.absorb(record.totals, priced.cost_usd, savings, unpriced);

        absorb(&mut analysis.overall);
        absorb(analysis.by_model.entry(record.model.clone()).or_default());
        absorb(analysis.by_day.entry(to_day(record.timestamp_ms)).or_default());

        let project = if record.project.is_empty() {
            UNKNOWN_PROJECT.to_string()
        } else {
            record.project.clone()
        };
        absorb(analysis.by_project.entry(project).or_default());

        if !record.session_id.is_empty() {
            absorb(analysis.by_session.entry(record.session_id.clone()).or_default());
            if record.timestamp_ms > latest_ms {
                latest_ms = record.timestamp_ms;
                analysis.latest_session = Some(record.session_id.clone());
            }
        }
    }

    analysis.latest_activity_ms = (latest_ms != i64::MIN).then_some(latest_ms);
    analysis
}

#[cfg(test)]
mod tests {
    use super::super::pricing::parse_rate_table;
    use super::*;

    fn table() -> RateTable {
        parse_rate_table(&serde_json::json!({
            "claude-opus-4-8": { "input_cost_per_token": 1.0, "output_cost_per_token": 2.0 },
            "claude-sonnet-5": { "input_cost_per_token": 1.0, "output_cost_per_token": 1.0 }
        }))
    }

    fn rec(ts: i64, model: &str, session: &str, project: &str, output: u64, dedupe: Option<&str>) -> UsageRecord {
        UsageRecord {
            timestamp_ms: ts,
            model: model.into(),
            session_id: session.into(),
            project: project.into(),
            totals: TokenTotals { output, ..Default::default() },
            reported_cost_usd: None,
            dedupe_key: dedupe.map(str::to_string),
        }
    }

    fn day_ab(ms: i64) -> Day {
        if ms == 0 { "2026-09-09".into() } else { "2026-09-10".into() }
    }

    #[test]
    fn folds_every_breakdown_in_one_pass() {
        let recs = [
            rec(0, "claude-opus-4-8", "s1", "/a", 10, Some("x")), // day09 $20
            rec(1, "claude-opus-4-8", "s1", "/a", 5, Some("y")),  // day10 $10
            rec(1, "claude-sonnet-5", "s2", "/b", 100, Some("z")), // day10 $100
        ];
        let a = analyze(recs.iter(), &table(), day_ab);

        assert!((a.overall.cost_usd - 130.0).abs() < 1e-9);
        assert_eq!(a.overall.totals.output, 115);
        assert!((a.by_model["claude-opus-4-8"].cost_usd - 30.0).abs() < 1e-9);
        assert!((a.by_day["2026-09-09"].cost_usd - 20.0).abs() < 1e-9);
        assert!((a.by_day["2026-09-10"].cost_usd - 110.0).abs() < 1e-9);
        assert!((a.by_project["/a"].cost_usd - 30.0).abs() < 1e-9);
        assert!((a.by_project["/b"].cost_usd - 100.0).abs() < 1e-9);
    }

    #[test]
    fn dedups_globally_and_picks_the_latest_session() {
        let recs = [
            rec(100, "claude-opus-4-8", "old", "/a", 10, Some("dup")),
            rec(200, "claude-opus-4-8", "old", "/a", 10, Some("dup")), // dropped
            rec(300, "claude-sonnet-5", "new", "/a", 1, Some("k")),
        ];
        let a = analyze(recs.iter(), &table(), day_ab);
        assert_eq!(a.duplicates_dropped, 1);
        assert_eq!(a.overall.records, 2);
        assert_eq!(a.latest_session.as_deref(), Some("new"));
        assert!(a.by_session.contains_key("new"));
    }

    #[test]
    fn missing_project_falls_under_unknown() {
        let a = analyze([rec(0, "claude-opus-4-8", "s", "", 1, None)].iter(), &table(), day_ab);
        assert!(a.by_project.contains_key(UNKNOWN_PROJECT));
    }
}
