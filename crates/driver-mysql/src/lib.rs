//! MySQL and MariaDB driver.

mod connect;
mod decode;
mod execute;
mod metadata;
mod page;
mod tls;

pub use connect::MySqlDriver;
