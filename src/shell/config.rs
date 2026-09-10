//! Ambient facts the core refuses to read: where transcripts live and which
//! timezone defines a "day".

use std::path::PathBuf;

use jiff::tz::TimeZone;

/// Runtime configuration, resolved once at startup.
pub struct Config {
    /// Directories to scan for `.jsonl` transcripts.
    pub roots: Vec<PathBuf>,
    /// The timezone in which a turn's calendar day is decided.
    pub tz: TimeZone,
}

impl Config {
    /// Resolves configuration from the environment.
    ///
    /// `CLAUDE_PROJECTS` overrides the transcript root (mainly for tests);
    /// otherwise the default `~/.claude/projects` is used. The timezone is the
    /// host's system zone, falling back to UTC when it cannot be determined.
    #[must_use]
    pub fn load() -> Self {
        let roots = match std::env::var_os("CLAUDE_PROJECTS") {
            Some(path) => vec![PathBuf::from(path)],
            None => dirs::home_dir()
                .map(|home| home.join(".claude").join("projects"))
                .into_iter()
                .collect(),
        };
        Config { roots, tz: TimeZone::system() }
    }

    /// A pure day-mapper bound to this config's timezone, for the core to fold with.
    pub fn day_mapper(&self) -> impl Fn(i64) -> String + '_ {
        move |ms| day_in_zone(ms, &self.tz)
    }
}

/// Formats a Unix-millisecond instant as its `YYYY-MM-DD` day in `tz`.
#[must_use]
pub fn day_in_zone(ms: i64, tz: &TimeZone) -> String {
    match jiff::Timestamp::from_millisecond(ms) {
        Ok(ts) => ts.to_zoned(tz.clone()).strftime("%Y-%m-%d").to_string(),
        Err(_) => "unknown".to_string(),
    }
}

/// The current instant in Unix milliseconds.
#[must_use]
pub fn now_ms() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}

/// Midnight (local start of today) as Unix milliseconds, for the "Today" range.
#[must_use]
pub fn start_of_today_ms(now_ms: i64, tz: &TimeZone) -> i64 {
    let Ok(ts) = jiff::Timestamp::from_millisecond(now_ms) else {
        return i64::MIN;
    };
    let today = ts.to_zoned(tz.clone()).date();
    match today.to_zoned(tz.clone()) {
        Ok(midnight) => midnight.timestamp().as_millisecond(),
        Err(_) => i64::MIN,
    }
}

/// The wall-clock time in `tz` as `HH:MM:SS`, for the footer's "updated" label.
#[must_use]
pub fn clock_hms(tz: &TimeZone) -> String {
    jiff::Timestamp::now().to_zoned(tz.clone()).strftime("%H:%M:%S").to_string()
}

/// A short label for `tz` (its IANA name, or `local`).
#[must_use]
pub fn zone_label(tz: &TimeZone) -> String {
    tz.iana_name().unwrap_or("local").to_string()
}
