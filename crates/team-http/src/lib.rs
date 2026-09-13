//! Optional team model entry point. Hosts explicitly provide authority, peer and request ledger.
mod contract;
mod ledger;
mod server;
mod usage;
pub use contract::*;
pub use ledger::{Ledger, Record};
pub use server::Service;
pub use usage::{SqliteUsage, UsageReader};

mod authority;
pub use authority::ModelAuthority;
