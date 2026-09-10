# ADR 0001: Compute cost from local transcripts, priced at the base tier

- Status: accepted
- Date: 2026-09-10

## Context

The goal is to show "the API estimate I spent" with Claude Code. Claude Code
subscription plans (Pro/Max) are not billed per token, and there is no account
API that returns a per-token cost for them. The only local source of truth is
the CLI's session transcripts under `~/.claude/projects/**/*.jsonl`, where each
`assistant` turn records a `usage` block (input, output, cache read, cache
creation tokens) and a model id. On current Claude Code versions the `costUSD`
field is absent, so cost must be computed.

This is the same approach `ccusage` and T3 Code's Usage page take.

## Decision

1. **Read and price the transcripts ourselves.** Parse each `assistant` line,
   sum the four token classes, and multiply by per-model rates.
2. **Source rates from LiteLLM's `model_prices_and_context_window.json`** — the
   same table `ccusage` uses — fetched and cached at runtime, with a
   Claude-only snapshot compiled into the binary as an offline fallback.
3. **Price at the base tier only.** LiteLLM publishes tiered variants
   (`*_above_272k_tokens`, `*_flex`, `*_priority`, `*_batches`) and separate 5m
   vs 1h cache-write rates. Transcripts do not record which tier served a
   request, so we price cache creation at the base `cache_creation_input_token_cost`
   and ignore the tiers. Matching `ccusage`/T3 Code matters more than a
   precision we cannot actually justify from the data.
4. **De-duplicate globally by `{message.id}:{requestId}`.** Resumed/forked
   sessions copy a turn's record into multiple files; counting each once is
   essential (thousands of duplicates on a real machine).
5. **Respect a subscription reality:** the figure is an *estimate of the
   equivalent pay-as-you-go API cost*, not a bill. The UI labels it "estimate".

## Consequences

- Fully offline-capable; network only improves price freshness.
- Numbers are directly comparable to `ccusage`, easing validation.
- A turn on an unknown/renamed model is reported as unpriced ($0, flagged)
  rather than guessed.
- If we ever want tier-accurate cache-write pricing, the transcript *does* carry
  an `ephemeral_1h_input_tokens` / `ephemeral_5m_input_tokens` split we could
  use — a future ADR could revisit decision (3) without touching the rest.
