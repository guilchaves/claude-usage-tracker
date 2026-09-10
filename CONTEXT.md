# Context: claude-usage-tracker (`ctracker`)

A terminal dashboard that estimates the **API-equivalent cost** of your Claude
Code usage. Claude Code subscriptions aren't billed per token, so there is no
account API for spend; instead we read the CLI's local session transcripts and
price the recorded tokens ourselves — the same approach `ccusage` and T3 Code's
Usage page take. See `docs/adr/0001-compute-cost-from-transcripts.md`.

## Architecture: functional core, imperative shell

- **`src/core/`** — pure, total, no I/O and no clock. Parsing, pricing, and the
  one-pass analysis fold live here and are exhaustively unit-tested. The only
  seam to the outside is an injected `to_day` function (bucketing depends on a
  timezone, which the core refuses to read).
- **`src/shell/`** — all side effects: scanning the filesystem, fetching and
  caching prices, resolving configuration and the timezone.
- **`src/tui/`** — a ratatui dashboard. Rendering is a pure function of `App`;
  the run loop is the only place that reads the clock and refreshes the scan.

## Glossary (ubiquitous language)

- **Turn** — one billable assistant response, recorded as one `assistant` line
  in a transcript. The unit everything is counted in (`UsageRecord`).
- **Transcript** — an append-only `.jsonl` session log under
  `~/.claude/projects/<encoded-dir>/<session>.jsonl`.
- **Token classes** — the four counts billed at different rates:
  *uncached input*, *cached input* (cache read, discounted), *cache creation*
  (cache write, premium), and *output* (thinking folded in, never counted
  separately).
- **Rate table** — normalized model id → per-token rates, projected from
  LiteLLM's `model_prices_and_context_window.json`.
- **Priced / unpriced** — a turn is *priced* from the rate table (or from a
  `costUSD` the transcript itself carried); an **unpriced** turn matched no rate
  (e.g. `<synthetic>`, or a bare family name like `opus`) and is reported as $0
  and flagged, never guessed.
- **Cache savings** — what cached input would have cost at full input rate minus
  what it actually cost: `cached_input × (input_rate − cache_read_rate)`.
- **Dedupe key** — `"{message.id}:{requestId}"`. Claude Code copies a turn's
  record forward when a session is **resumed** or **forked**, so the same key
  recurs across transcripts; only the first sighting counts. (Global dedup, not
  per file — it drops thousands of records on a real machine.)
- **Project** — the working directory a turn ran in (`cwd`), used to group
  spend; falls back to the decoded transcript directory name.
- **Session** — a Claude Code session id. The **current session** is the one
  with the most recent turn (the live meter).
- **Range** — the time window the figures cover: Today / 7 days / 30 days /
  All time.
- **Resume offset** — the byte position (plus a 64-byte tail hash guard) where
  the scanner stopped reading a file, so the next pass reads only appended
  bytes. Makes a live, once-a-second refresh cheap.
