//! `cusage` — a TUI that estimates the API-equivalent cost of your Claude Code
//! usage, computed from the local session transcripts under `~/.claude/projects`.
//!
//! The code is split into a pure [`core`] (parse, price, analyze — no I/O, no
//! clock, exhaustively unit-tested) and an imperative [`shell`]/[`tui`] around
//! it (scanning, pricing lookup, the terminal). `main` just wires them.

mod core;
mod shell;
mod tui;

use std::io;

use crate::core::analysis::analyze;
use crate::shell::config::Config;
use crate::shell::{prices, scan::Scanner};

fn main() -> io::Result<()> {
    let config = Config::load();
    let prices = prices::load();
    let mut scanner = Scanner::new(config.roots.clone());
    scanner.refresh();

    let args: Vec<String> = std::env::args().collect();

    // `--once` prints a summary and exits — handy for scripts and for eyeballing
    // the numbers without entering the full-screen UI.
    if args.iter().any(|arg| arg == "--once") {
        print_once(&config, &prices, &scanner);
        return Ok(());
    }

    // `--render [tab]` prints one frame of the dashboard as text (no live TTY).
    if let Some(pos) = args.iter().position(|arg| arg == "--render") {
        let tab = args.get(pos + 1).and_then(|s| s.parse::<usize>().ok()).unwrap_or(0);
        print!("{}", tui::render_to_string(&config, &prices, &scanner, tab, 120, 30));
        return Ok(());
    }

    tui::run(config, prices, scanner)
}

fn print_once(config: &Config, prices: &prices::Prices, scanner: &Scanner) {
    let analysis = analyze(scanner.records(), &prices.table, config.day_mapper());
    let o = &analysis.overall;

    println!("Claude Code usage estimate  (prices: {})", prices.source.label());
    println!("────────────────────────────────────────");
    println!("Total estimate:  ${:.2}   cache saved: ${:.2}", o.cost_usd, o.cache_savings_usd);
    println!(
        "Tokens: {}   turns: {}   duplicates dropped: {}",
        o.totals.grand_total(),
        o.records,
        analysis.duplicates_dropped
    );
    if o.unpriced_records > 0 {
        println!("Unpriced turns: {}", o.unpriced_records);
    }

    let mut models: Vec<_> = analysis.by_model.iter().collect();
    models.sort_by(|a, b| b.1.cost_usd.total_cmp(&a.1.cost_usd));
    println!("\nBy model");
    for (model, line) in models {
        println!("  {model:<28} ${:>10.2}   {} turns", line.cost_usd, line.records);
    }
}
