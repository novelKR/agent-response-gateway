//! Optional management transport contracts and local authentication adapters.
mod auth;
mod contract;
pub use auth::*;
pub use contract::*;
mod server;
pub use server::{ApiError, Service};
