//! Platform-independent logic for snisplit.
//!
//! Nothing here does I/O or needs admin rights, so everything is unit-testable.
//! Milestone 0 holds just enough to print the SNI of captured packets; the full
//! parsers, matcher and segment planner arrive in milestone 1.

#![forbid(unsafe_code)]

pub mod packet;
pub mod tls;

mod reader;
