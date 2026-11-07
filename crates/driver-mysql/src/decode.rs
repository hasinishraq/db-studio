//! Turns wire values into [`Value`].
//!
//! The text protocol sends every value as the server's own text, so temporal values stay exactly as
//! the server printed them. The binary protocol, used for bound parameters, is formatted the same way.

use mysql_async::consts::{ColumnFlags, ColumnType};
use mysql_async::{Column, Value as Wire};
use studio_db::Value;

/// The character set number of binary data.
const BINARY_CHARSET: u16 = 63;

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn is_binary(column: &Column) -> bool {
    column.character_set() == BINARY_CHARSET
        && !column
            .flags()
            .intersects(ColumnFlags::ENUM_FLAG | ColumnFlags::SET_FLAG)
}

fn is_temporal(ty: ColumnType) -> bool {
    use ColumnType::*;
    matches!(
        ty,
        MYSQL_TYPE_DATE
            | MYSQL_TYPE_NEWDATE
            | MYSQL_TYPE_TIME
            | MYSQL_TYPE_TIME2
            | MYSQL_TYPE_DATETIME
            | MYSQL_TYPE_DATETIME2
            | MYSQL_TYPE_TIMESTAMP
            | MYSQL_TYPE_TIMESTAMP2
    )
}

fn integer_text(text: String) -> Value {
    text.parse().map_or(Value::BigInt(text), Value::Int)
}

fn decode_bit(bytes: &[u8]) -> Value {
    let value = bytes
        .iter()
        .fold(0u128, |acc, byte| (acc << 8) | u128::from(*byte));
    integer_text(value.to_string())
}

fn decode_bytes(bytes: &[u8], column: &Column) -> Value {
    use ColumnType::*;
    let ty = column.column_type();
    match ty {
        MYSQL_TYPE_TINY | MYSQL_TYPE_SHORT | MYSQL_TYPE_INT24 | MYSQL_TYPE_LONG | MYSQL_TYPE_YEAR => {
            integer_text(lossy(bytes))
        }
        MYSQL_TYPE_LONGLONG => Value::BigInt(lossy(bytes)),
        MYSQL_TYPE_DECIMAL | MYSQL_TYPE_NEWDECIMAL => Value::Decimal(lossy(bytes)),
        MYSQL_TYPE_FLOAT | MYSQL_TYPE_DOUBLE => {
            let text = lossy(bytes);
            text.parse().map_or(Value::Text(text), Value::Float)
        }
        MYSQL_TYPE_JSON => Value::Json(lossy(bytes)),
        MYSQL_TYPE_BIT => decode_bit(bytes),
        _ if is_temporal(ty) => Value::Temporal(lossy(bytes)),
        _ if is_binary(column) => Value::bytes(bytes),
        _ => Value::Text(lossy(bytes)),
    }
}

fn format_date(ty: ColumnType, parts: (u16, u8, u8, u8, u8, u8, u32)) -> String {
    let (year, month, day, hour, minute, second, micros) = parts;
    if matches!(ty, ColumnType::MYSQL_TYPE_DATE | ColumnType::MYSQL_TYPE_NEWDATE) {
        return format!("{year:04}-{month:02}-{day:02}");
    }
    let base = format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}");
    if micros == 0 {
        base
    } else {
        format!("{base}.{micros:06}")
    }
}

fn format_time(negative: bool, days: u32, hours: u8, minutes: u8, seconds: u8, micros: u32) -> String {
    let sign = if negative { "-" } else { "" };
    let total_hours = days * 24 + u32::from(hours);
    let base = format!("{sign}{total_hours:02}:{minutes:02}:{seconds:02}");
    if micros == 0 {
        base
    } else {
        format!("{base}.{micros:06}")
    }
}

/// Decodes one cell.
pub(crate) fn decode(value: &Wire, column: &Column) -> Value {
    match value {
        Wire::NULL => Value::Null,
        Wire::Bytes(bytes) => decode_bytes(bytes, column),
        Wire::Int(v) => integer_text(v.to_string()),
        Wire::UInt(v) => integer_text(v.to_string()),
        Wire::Float(v) => Value::Float(f64::from(*v)),
        Wire::Double(v) => Value::Float(*v),
        Wire::Date(y, mo, d, h, mi, s, us) => {
            Value::Temporal(format_date(column.column_type(), (*y, *mo, *d, *h, *mi, *s, *us)))
        }
        Wire::Time(neg, d, h, mi, s, us) => Value::Temporal(format_time(*neg, *d, *h, *mi, *s, *us)),
    }
}

/// A readable type name for a result column.
pub(crate) fn type_name(column: &Column) -> String {
    let name = format!("{:?}", column.column_type());
    name.trim_start_matches("MYSQL_TYPE_").to_lowercase()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn dates_format_like_the_text_protocol() {
        let date = (2026, 1, 2, 3, 4, 5, 0);
        assert_eq!(format_date(ColumnType::MYSQL_TYPE_DATE, date), "2026-01-02");
        assert_eq!(
            format_date(ColumnType::MYSQL_TYPE_DATETIME, date),
            "2026-01-02 03:04:05"
        );
        let fraction = (2026, 1, 2, 3, 4, 5, 120);
        assert_eq!(
            format_date(ColumnType::MYSQL_TYPE_DATETIME, fraction),
            "2026-01-02 03:04:05.000120"
        );
    }

    #[test]
    fn long_times_use_total_hours() {
        assert_eq!(format_time(true, 2, 1, 0, 0, 0), "-49:00:00");
        assert_eq!(format_time(false, 0, 1, 2, 3, 0), "01:02:03");
    }

    #[test]
    fn bit_values_are_numbers() {
        assert_eq!(decode_bit(&[1, 0]), Value::Int(256));
        assert_eq!(
            decode_bit(&[0xff; 8]),
            Value::BigInt("18446744073709551615".into())
        );
    }
}
