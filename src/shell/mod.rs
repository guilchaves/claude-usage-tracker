//! The imperative shell: everything the pure core refuses to touch — the
//! filesystem, the network, the clock, and configuration.

pub mod config;
pub mod prices;
pub mod scan;
