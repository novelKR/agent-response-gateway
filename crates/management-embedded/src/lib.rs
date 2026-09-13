//! Optional host delegation contracts. No default process, listener, user store or login system.
mod external;
mod host;
pub use external::*;
pub use host::*;
#[cfg(feature = "team")]
mod model;
#[cfg(feature = "team")]
pub use model::*;
