//! DTOs shared between the Rust backend and the frontend.
//!
//! Everything here crosses the IPC boundary, so nothing in this crate may carry a secret.
//! There is deliberately no password, passphrase or API key field on any type.

pub mod ai;
pub mod error;
pub mod ids;
pub mod profile;
pub mod query;
pub mod schema;
pub mod settings;
pub mod value;

pub use ai::*;
pub use error::*;
pub use ids::*;
pub use profile::*;
pub use query::*;
pub use schema::*;
pub use settings::*;
pub use value::*;
