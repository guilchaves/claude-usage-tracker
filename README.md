# claude-usage-tracker (`cusage`)

A terminal dashboard that estimates the **API-equivalent cost** of your Claude
Code usage, computed from the local session transcripts in `~/.claude/projects`.
Inspired by T3 Code's Usage page and `ccusage`.

> Claude Code subscriptions aren't billed per token, so this is an estimate of
> what the same work would cost on pay-as-you-go API pricing — not a bill.

## What it shows

- **Overview** — headline estimate, cache savings, tokens/turns, and a per-day
  cost trend.
- **By model** — cost, turns, tokens, and cache savings per model.
- **By project** — spend grouped by working directory.
- **Session** — a live meter for the current (most recent) session, plus a table
  of recent sessions.

Time ranges: Today / 7 days / 30 days / All time. The dashboard refreshes about
once a second, reading only newly appended transcript bytes, so it tracks a
running session live.

## Usage

```sh
cargo run --release            # launch the TUI
cargo run --release -- --once  # print a one-shot summary (good for scripts)
cargo run --release -- --render 0   # print one dashboard frame as text (0..3 = tab)
```

Keys: `←/→` (or Tab) switch tabs · `t` `w` `m` `a` set the range · `r` refresh ·
`q` quit.

Environment:

- `CLAUDE_PROJECTS` — override the transcript root (default `~/.claude/projects`).

## Pricing

Rates come from LiteLLM's `model_prices_and_context_window.json` (the same table
`ccusage` uses), fetched and cached under your cache dir, with a Claude-only
snapshot (`assets/litellm_prices.json`) compiled in as an offline fallback. The
footer shows which source is in use (`live` / `cached` / `bundled`).

## Design

Functional core, imperative shell:

- `src/core/` — pure parsing, pricing, and analysis (no I/O, no clock).
- `src/shell/` — filesystem scanning (with append-only resume offsets), price
  loading, config.
- `src/tui/` — ratatui dashboard; rendering is a pure function of app state.

See `CONTEXT.md` and `docs/adr/` for the domain model and key decisions.

## Tests

```sh
cargo test
```
