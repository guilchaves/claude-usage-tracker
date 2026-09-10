//! The value types the pure core folds over.
//!
//! A [`UsageRecord`] is one billable assistant turn, distilled from a single
//! transcript line. Everything here is plain data with value semantics: the
//! parser produces records, the aggregator folds them, and nothing in between
//! touches the filesystem or the clock.

/// The four token counts Anthropic bills against, split by how each is priced.
///
/// Reasoning/thinking tokens are deliberately absent: Anthropic folds them into
/// `output_tokens` and does not bill them separately, so counting them again
/// would double-charge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TokenTotals {
    /// Fresh input tokens, billed at the full input rate.
    pub uncached_input: u64,
    /// Tokens served from cache, billed at the discounted cache-read rate.
    pub cached_input: u64,
    /// Tokens written into the cache, billed at the premium cache-write rate.
    pub cache_creation: u64,
    /// Generated tokens (thinking already folded in), billed at the output rate.
    pub output: u64,
}

impl TokenTotals {
    /// Component-wise sum. Pure; the identity is [`TokenTotals::default`].
    #[must_use]
    pub fn add(self, other: TokenTotals) -> TokenTotals {
        TokenTotals {
            uncached_input: self.uncached_input + other.uncached_input,
            cached_input: self.cached_input + other.cached_input,
            cache_creation: self.cache_creation + other.cache_creation,
            output: self.output + other.output,
        }
    }

    /// Every token that passed through the request, for display totals.
    #[must_use]
    pub fn grand_total(self) -> u64 {
        self.uncached_input + self.cached_input + self.cache_creation + self.output
    }
}

/// One billable assistant turn.
#[derive(Clone, Debug, PartialEq)]
pub struct UsageRecord {
    /// Wall-clock instant of the turn, in Unix milliseconds.
    pub timestamp_ms: i64,
    /// The model id exactly as the transcript recorded it (e.g. `claude-opus-4-8`).
    pub model: String,
    /// The session the turn belongs to; empty when the transcript omitted it.
    pub session_id: String,
    /// The working directory the turn ran in (`cwd`), used to group by project.
    /// Empty when the transcript omitted it; the shell can fall back to the
    /// transcript's directory name.
    pub project: String,
    pub totals: TokenTotals,
    /// A `costUSD` the CLI itself recorded, when present. Newer Claude Code
    /// versions omit this, in which case the cost is computed from `totals`.
    pub reported_cost_usd: Option<f64>,
    /// Key for cross-file de-duplication, or `None` when the record carries
    /// neither a message id nor a request id and so cannot be de-duplicated.
    ///
    /// Claude Code copies a turn's record forward into every transcript that
    /// resumes or forks the session, so the same key legitimately recurs across
    /// files; the aggregator keeps only the first sighting.
    pub dedupe_key: Option<String>,
}
