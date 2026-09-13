//! Optional management foundation. No listener, credential discovery or process startup.
//! Trusted adapters own effects and hold target locks while the journal records intent.
mod contract;
mod journal;

pub use contract::*;
pub use journal::{Journal, Reader};
