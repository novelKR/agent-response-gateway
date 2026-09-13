//! Optional Team Access. No automatic storage initialization, listener or background worker.
mod auth;
mod contract;
mod store;
pub use auth::Authenticator;
pub use contract::*;
pub use store::Manager;
