# claude-usage-tracker (`ctracker`)

A terminal dashboard that estimates the **API-equivalent cost** of your Claude
Code usage, computed from the local session transcripts in `~/.claude/projects`.
Inspired by T3 Code's Usage page and `ccusage`.

> Claude Code subscriptions aren't billed per token, so this is an estimate of
> what the same work would cost on pay-as-you-go API pricing — not a bill.

## What it shows

A single dashboard modeled on Claude Code's Usage panel:

- a headline **estimate** (or token count), `N sessions · API estimate`, and a
  Claude Code provider row;
- a **Daily cost** line chart;
- a **Totals** row — processed / cached / uncached / output tokens and cache
  savings;
- a **Breakdown** table (Cost · Share · Tokens) you can group by **Model**,
  **Day**, **Project**, or **Session**.

Toggle the headline between **Cost** and **Tokens**. Time ranges: Past 24h /
7 / 30 / 90 days / All. The dashboard refreshes about once a second, reading
only newly appended transcript bytes, so it tracks a running session live.

## Usage

```sh
cargo run --release            # launch the TUI
cargo run --release -- --once  # print a one-shot summary (good for scripts)
cargo run --release -- --render 0   # print one dashboard frame as text (0..3 = tab)
```

Keys: `←/→` (or Tab) change the breakdown grouping · `1`–`5` set the range ·
`space` toggle Cost/Tokens · `r` refresh · `q` quit.

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
