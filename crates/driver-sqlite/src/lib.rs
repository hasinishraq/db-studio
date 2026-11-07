//! SQLite driver. `rusqlite` is synchronous, so all work runs on blocking threads.

mod execute;
mod metadata;
mod session;

pub use session::SqliteDriver;
