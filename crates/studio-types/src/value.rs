//! Engine-neutral cell values.

use serde::{Deserialize, Serialize};
use specta::Type;

/// One cell.
///
/// Types JavaScript cannot represent exactly travel as strings with a type tag, so nothing is
/// silently rounded and no timezone conversion happens.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "t", content = "v")]
pub enum Value {
    /// SQL NULL.
    Null,
    /// Boolean.
    Bool(bool),
    /// An integer that fits a JavaScript number.
    Int(i32),
    /// A 64-bit or wider integer, as text.
    BigInt(String),
    /// A floating point number.
    Float(f64),
    /// An exact decimal, as text.
    Decimal(String),
    /// Text.
    Text(String),
    /// JSON, as text.
    Json(String),
    /// Binary data. Large blobs are never shipped to the grid.
    Bytes {
        /// Total length in bytes.
        len: u32,
        /// Hex of at most the first 64 bytes.
        preview_hex: String,
    },
    /// A date or time in the database's own text form.
    Temporal(String),
    /// A value we cannot decode; carries the type name for display.
    Unsupported(String),
}

impl Value {
    /// Builds a [`Value::Bytes`] from raw bytes, keeping only a short preview.
    pub fn bytes(data: &[u8]) -> Self {
        const PREVIEW: usize = 64;
        let mut preview_hex = String::with_capacity(PREVIEW * 2);
        for byte in data.iter().take(PREVIEW) {
            preview_hex.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
            preview_hex.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
        }
        Value::Bytes {
            len: u32::try_from(data.len()).unwrap_or(u32::MAX),
            preview_hex,
        }
    }
}
