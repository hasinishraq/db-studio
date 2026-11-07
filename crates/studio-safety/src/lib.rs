//! Decides, for every statement, whether it is a read, whether the AI may run it unattended and
//! how risky it is, and issues and checks approval tokens. Pure: this crate performs no I/O.
//!
//! The classifier is an allow-list. Only statements it positively recognises as reads are reads;
//! everything else, including anything that fails to parse, is treated as a write.

pub mod approval;
pub mod classify;
pub mod dialect;
pub mod functions;
pub mod lexer;
pub mod outline;
pub mod risk;
pub mod split;

pub use approval::{sha256_hex, ApprovalBook, SafetyError};
pub use classify::{ai_auto_allowed, classify_script, verdicts, ClassifiedStatement, Risk, Verdict};
pub use lexer::has_executable_comment;
pub use outline::schema_outline;
