//! The functional core: pure, total, and free of I/O and the clock.
//!
//! The data flows one way — [`parse`] a transcript line into a
//! [`record::UsageRecord`], then [`analysis::analyze`] folds a stream of them
//! into every breakdown the UI shows (pricing each with [`pricing`]). Every
//! function here is deterministic given its inputs, which is what lets the
//! whole pipeline be tested without a filesystem, a network, or a wall clock.

pub mod analysis;
pub mod parse;
pub mod pricing;
pub mod record;
