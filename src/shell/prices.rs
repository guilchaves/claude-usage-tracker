//! Loading the model rate table: cache -> network -> bundled snapshot.
//!
//! The pure core only knows how to parse and price against a [`RateTable`];
//! deciding where the pricing document comes from, and tolerating a machine
//! that is offline, is this module's job.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use crate::core::pricing::{parse_rate_table, RateTable};

/// LiteLLM's canonical pricing document — the same one `ccusage` prices against.
const LITELLM_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

/// A Claude-only snapshot compiled into the binary, so the app prices correctly
/// with no network at all. Refreshed from LiteLLM at build time.
const BUNDLED: &str = include_str!("../../assets/litellm_prices.json");

/// How long a cached document is trusted before we try to refresh it.
const MAX_CACHE_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// Where the priced rates ultimately came from, so the UI can be honest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PriceSource {
    /// A fresh (or fallback) on-disk cache of a previous download.
    Cache,
    /// Just downloaded from LiteLLM.
    Network,
    /// The compiled-in snapshot — used offline or when everything else fails.
    Bundled,
}

impl PriceSource {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            PriceSource::Cache => "cached",
            PriceSource::Network => "live",
            PriceSource::Bundled => "bundled",
        }
    }
}

/// A loaded rate table and where it came from.
pub struct Prices {
    pub table: RateTable,
    pub source: PriceSource,
}

/// Loads rates, preferring a fresh cache, then a live fetch, then a stale
/// cache, and finally the bundled snapshot. Always succeeds.
#[must_use]
pub fn load() -> Prices {
    let cache = cache_path();

    if let Some(path) = &cache {
        if is_fresh(path) {
            if let Some(table) = read_table(path) {
                return Prices { table, source: PriceSource::Cache };
            }
        }
    }

    if let Some(document) = fetch() {
        let table = parse_rate_table(&document);
        if !table.is_empty() {
            if let Some(path) = &cache {
                write_cache(path, &document);
            }
            return Prices { table, source: PriceSource::Network };
        }
    }

    // Network failed or returned nothing usable — fall back to a stale cache.
    if let Some(path) = &cache {
        if let Some(table) = read_table(path) {
            return Prices { table, source: PriceSource::Cache };
        }
    }

    let document: Value = serde_json::from_str(BUNDLED).unwrap_or(Value::Null);
    Prices { table: parse_rate_table(&document), source: PriceSource::Bundled }
}

fn cache_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|dir| dir.join("claude-usage-tracker").join("litellm_prices.json"))
}

fn is_fresh(path: &PathBuf) -> bool {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age < MAX_CACHE_AGE)
}

fn read_table(path: &PathBuf) -> Option<RateTable> {
    let document: Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    let table = parse_rate_table(&document);
    (!table.is_empty()).then_some(table)
}

fn write_cache(path: &PathBuf, document: &Value) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec(document) {
        let _ = fs::write(path, bytes);
    }
}

fn fetch() -> Option<Value> {
    ureq::get(LITELLM_URL)
        .timeout(Duration::from_secs(6))
        .call()
        .ok()?
        .into_json()
        .ok()
}
