//! Turns the server's text form of a value into [`Value`].
//!
//! Results use the text format, so a date or timestamp is exactly what the server printed, with no
//! timezone conversion, and every type, including enums and domains, has a representation.

use studio_db::Value;
use tokio_postgres::types::{Kind, Type};

/// Decodes one cell.
pub(crate) fn decode(text: Option<&str>, ty: &Type) -> Value {
    let Some(text) = text else { return Value::Null };
    match *ty {
        Type::BOOL => Value::Bool(text == "t"),
        Type::INT2 | Type::INT4 => text
            .parse()
            .map_or_else(|_| Value::BigInt(text.to_owned()), Value::Int),
        Type::INT8 | Type::OID => Value::BigInt(text.to_owned()),
        Type::NUMERIC | Type::MONEY => Value::Decimal(text.to_owned()),
        Type::FLOAT4 | Type::FLOAT8 => match text.parse::<f64>() {
            // NaN and infinities have no JSON form.
            Ok(value) if value.is_finite() => Value::Float(value),
            _ => Value::Text(text.to_owned()),
        },
        Type::JSON | Type::JSONB => Value::Json(text.to_owned()),
        Type::BYTEA => decode_bytea(text),
        Type::DATE | Type::TIME | Type::TIMETZ | Type::TIMESTAMP | Type::TIMESTAMPTZ | Type::INTERVAL => {
            Value::Temporal(text.to_owned())
        }
        _ => match ty.kind() {
            Kind::Array(_) | Kind::Composite(_) | Kind::Range(_) => Value::Json(text.to_owned()),
            _ => Value::Text(text.to_owned()),
        },
    }
}

/// Decodes the `\x` hex output format.
fn decode_bytea(text: &str) -> Value {
    let Some(hex) = text.strip_prefix("\\x") else {
        return Value::Text(text.to_owned());
    };
    let digit = |b: u8| char::from(b).to_digit(16).and_then(|d| u8::try_from(d).ok());
    let (pairs, _) = hex.as_bytes().as_chunks::<2>();
    let bytes: Option<Vec<u8>> = pairs
        .iter()
        .map(|[hi, lo]| Some(digit(*hi)? << 4 | digit(*lo)?))
        .collect();
    bytes.map_or_else(|| Value::Text(text.to_owned()), |bytes| Value::bytes(&bytes))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn exact_numbers_stay_text() {
        assert_eq!(
            decode(Some("9007199254740993"), &Type::INT8),
            Value::BigInt("9007199254740993".into())
        );
        assert_eq!(
            decode(Some("1.10"), &Type::NUMERIC),
            Value::Decimal("1.10".into())
        );
        assert_eq!(decode(Some("7"), &Type::INT4), Value::Int(7));
    }

    #[test]
    fn dates_are_not_reinterpreted() {
        assert_eq!(
            decode(Some("2026-01-01"), &Type::DATE),
            Value::Temporal("2026-01-01".into())
        );
        let ts = "2026-11-07 12:30:00+06";
        assert_eq!(decode(Some(ts), &Type::TIMESTAMPTZ), Value::Temporal(ts.into()));
    }

    #[test]
    fn non_finite_floats_become_text() {
        assert_eq!(decode(Some("NaN"), &Type::FLOAT8), Value::Text("NaN".into()));
        assert_eq!(
            decode(Some("Infinity"), &Type::FLOAT8),
            Value::Text("Infinity".into())
        );
    }

    #[test]
    fn bytea_keeps_a_preview() {
        assert_eq!(decode(Some("\\x01ff"), &Type::BYTEA), Value::bytes(&[1, 255]));
    }

    #[test]
    fn null_is_null() {
        assert_eq!(decode(None, &Type::TEXT), Value::Null);
    }
}
