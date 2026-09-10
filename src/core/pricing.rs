//! Model rate lookup and cost arithmetic — all pure.
//!
//! Rates come from LiteLLM's `model_prices_and_context_window.json`, the same
//! table `ccusage` prices against. Fetching and caching that document is the
//! shell's job; this module only projects it into a [`RateTable`] and does the
//! arithmetic. We price at the base tier: transcripts don't record which tier
//! (>272k, flex, priority, batch) served a request, so anything finer would be
//! a guess dressed up as precision.

use std::collections::HashMap;

use serde_json::Value;

use super::record::TokenTotals;

/// USD-per-token rates for one model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelRate {
    pub input: f64,
    pub output: f64,
    /// Discounted rate for cache reads.
    pub cache_read: f64,
    /// Premium rate for cache writes.
    pub cache_creation: f64,
}

/// Normalized (lowercased) model id -> its rate.
pub type RateTable = HashMap<String, ModelRate>;

/// Where a priced figure came from, so the UI can be honest about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CostSource {
    /// The transcript itself carried a `costUSD`.
    ProviderReported,
    /// Computed from token counts and a matched rate.
    ModelPriced,
    /// No rate matched; reported as `$0` and flagged rather than guessed.
    Unpriced,
}

/// A priced turn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Priced {
    pub cost_usd: f64,
    pub source: CostSource,
}

/// Bare family names are genuinely ambiguous across generations, and
/// `<synthetic>` marks locally generated turns that were never billed. We
/// report these as unpriced instead of guessing a generation.
const UNPRICEABLE: &[&str] = &["<synthetic>", "synthetic", "opus", "sonnet", "haiku", "fable"];

/// Drops a bracketed variant suffix such as `claude-fable-5-1[1m]` (the 1M
/// context tier). The table only knows the base name, and we price at base.
fn strip_variant_suffix(key: &str) -> &str {
    match key.find('[') {
        Some(i) => &key[..i],
        None => key,
    }
}

/// The last path segment of a possibly provider-prefixed key (`anthropic/foo` -> `foo`).
fn bare_name(key: &str) -> &str {
    match key.rfind('/') {
        Some(i) => &key[i + 1..],
        None => key,
    }
}

/// Looks up a rate for `model`, applying suffix-stripping and the unpriceable
/// guard. Returns `None` when the model is unpriceable or simply absent.
#[must_use]
pub fn lookup_rate<'t>(table: &'t RateTable, model: &str) -> Option<&'t ModelRate> {
    let key = strip_variant_suffix(model.trim()).to_lowercase();
    let bare = bare_name(&key);
    if bare.is_empty() || UNPRICEABLE.contains(&bare) {
        return None;
    }
    table.get(&key)
}

/// Prices one turn's tokens.
///
/// Precedence mirrors T3 Code: an explicit `reported_cost_usd` wins, then a
/// matched rate, else the turn is [`CostSource::Unpriced`].
#[must_use]
pub fn price_usage(
    table: &RateTable,
    model: &str,
    totals: TokenTotals,
    reported_cost_usd: Option<f64>,
) -> Priced {
    if let Some(cost) = reported_cost_usd.filter(|c| c.is_finite()) {
        return Priced { cost_usd: cost, source: CostSource::ProviderReported };
    }
    match lookup_rate(table, model) {
        None => Priced { cost_usd: 0.0, source: CostSource::Unpriced },
        Some(rate) => {
            let cost_usd = totals.uncached_input as f64 * rate.input
                + totals.cached_input as f64 * rate.cache_read
                + totals.cache_creation as f64 * rate.cache_creation
                + totals.output as f64 * rate.output;
            Priced { cost_usd, source: CostSource::ModelPriced }
        }
    }
}

/// What the cached input would have cost at full input rates minus what it
/// actually cost — the "cache savings" figure. Zero for an unpriced model.
#[must_use]
pub fn cache_savings_usd(table: &RateTable, model: &str, totals: TokenTotals) -> f64 {
    match lookup_rate(table, model) {
        None => 0.0,
        Some(rate) => totals.cached_input as f64 * (rate.input - rate.cache_read),
    }
}

fn finite(value: &Value) -> Option<f64> {
    value.as_f64().filter(|n| n.is_finite())
}

/// Projects a LiteLLM pricing document into a [`RateTable`].
///
/// An entry is dropped unless it has both an input and an output rate: a
/// half-priced model would silently under-report. Cache rates fall back to the
/// input rate when absent (cached input is priced as plain input, never free).
/// A bare model name is aliased to a qualified entry only when no canonical
/// entry claims it and every qualified entry agrees on the rate.
#[must_use]
pub fn parse_rate_table(document: &Value) -> RateTable {
    let Some(entries) = document.as_object() else {
        return RateTable::new();
    };

    let mut table = RateTable::new();
    for (name, raw) in entries {
        let Some(obj) = raw.as_object() else { continue };
        let (Some(input), Some(output)) = (
            obj.get("input_cost_per_token").and_then(finite),
            obj.get("output_cost_per_token").and_then(finite),
        ) else {
            continue;
        };
        let key = name.trim().to_lowercase();
        if key.is_empty() {
            continue;
        }
        table.insert(
            key,
            ModelRate {
                input,
                output,
                cache_read: obj.get("cache_read_input_token_cost").and_then(finite).unwrap_or(input),
                cache_creation: obj
                    .get("cache_creation_input_token_cost")
                    .and_then(finite)
                    .unwrap_or(input),
            },
        );
    }

    // Alias bare names (`anthropic/foo` -> `foo`) only when unambiguous.
    // `None` marks a bare name claimed at conflicting rates.
    let mut aliases: HashMap<String, Option<ModelRate>> = HashMap::new();
    for (key, rate) in &table {
        let alias = bare_name(key);
        if alias.is_empty() || alias == key || table.contains_key(alias) {
            continue;
        }
        aliases
            .entry(alias.to_string())
            .and_modify(|held| {
                if matches!(held, Some(r) if r != rate) {
                    *held = None;
                }
            })
            .or_insert(Some(*rate));
    }
    for (alias, rate) in aliases {
        if let Some(rate) = rate {
            table.insert(alias, rate);
        }
    }

    table
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> RateTable {
        let doc = serde_json::json!({
            "claude-opus-4-8": {
                "input_cost_per_token": 0.000015,
                "output_cost_per_token": 0.000075,
                "cache_read_input_token_cost": 0.0000015,
                "cache_creation_input_token_cost": 0.00001875
            }
        });
        parse_rate_table(&doc)
    }

    #[test]
    fn prices_each_token_class_at_its_own_rate() {
        let totals = TokenTotals {
            uncached_input: 1000,
            cached_input: 1000,
            cache_creation: 1000,
            output: 1000,
        };
        let priced = price_usage(&table(), "claude-opus-4-8", totals, None);
        assert_eq!(priced.source, CostSource::ModelPriced);
        let expected = 1000.0 * (0.000015 + 0.0000015 + 0.00001875 + 0.000075);
        assert!((priced.cost_usd - expected).abs() < 1e-12);
    }

    #[test]
    fn reported_cost_takes_precedence() {
        let priced = price_usage(&table(), "claude-opus-4-8", TokenTotals::default(), Some(9.99));
        assert_eq!(priced.source, CostSource::ProviderReported);
        assert_eq!(priced.cost_usd, 9.99);
    }

    #[test]
    fn strips_the_1m_variant_suffix() {
        let doc = serde_json::json!({
            "claude-fable-5-1": { "input_cost_per_token": 1.0, "output_cost_per_token": 2.0 }
        });
        let t = parse_rate_table(&doc);
        assert!(lookup_rate(&t, "claude-fable-5-1[1m]").is_some());
    }

    #[test]
    fn bare_family_names_are_unpriceable() {
        assert!(lookup_rate(&table(), "opus").is_none());
        let priced = price_usage(&table(), "opus", TokenTotals { output: 100, ..Default::default() }, None);
        assert_eq!(priced.source, CostSource::Unpriced);
        assert_eq!(priced.cost_usd, 0.0);
    }

    #[test]
    fn cache_savings_is_the_discount_on_cached_input() {
        let totals = TokenTotals { cached_input: 1_000_000, ..Default::default() };
        let saved = cache_savings_usd(&table(), "claude-opus-4-8", totals);
        let expected = 1_000_000.0 * (0.000015 - 0.0000015);
        assert!((saved - expected).abs() < 1e-9);
    }

    #[test]
    fn entries_without_both_core_rates_are_dropped() {
        let doc = serde_json::json!({ "half": { "input_cost_per_token": 1.0 } });
        assert!(parse_rate_table(&doc).is_empty());
    }
}
