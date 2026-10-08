//! Internal canonical types, adapter contracts, and the local migration engine.

pub mod canonical;
pub mod engine;
pub mod gate;
pub mod governance;
pub mod jcs;
pub mod okf;
pub mod plugins;
pub mod reader;
pub mod reports;
pub mod satellite;
pub mod schema;
pub mod source;
pub mod writer;

pub use anyhow::Result;
