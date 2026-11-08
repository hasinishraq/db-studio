//! Charts the model may ask for with `run_sql`. A request names columns only; the values are
//! always the rows the database returned, so the model cannot draw numbers it made up. Nothing
//! here needs a row, so asking for a chart never shares data with the model.

use serde_json::Value as Json;
use studio_types::{ChartKind, ChartSpec, ColumnMeta};

/// Most value columns in one chart.
pub const MAX_SERIES: usize = 5;
/// Longest title kept.
pub const MAX_TITLE: usize = 120;
const MAX_NAME: usize = 128;

/// The names the model uses, for the tool schema and for messages.
pub const KINDS: [&str; 8] = [
    "bar",
    "horizontal_bar",
    "stacked_bar",
    "line",
    "area",
    "pie",
    "donut",
    "scatter",
];

/// The JSON schema of the `chart` argument.
pub fn schema() -> Json {
    serde_json::json!({
        "type": "object",
        "description": "Optional. Draw the result as a chart instead of a table. Name columns of \
                        this query's result only; the values come from the rows it returns.",
        "properties": {
            "type": { "type": "string", "enum": KINDS },
            "x": { "type": "string", "description": "Column holding the labels (categories or dates)" },
            "y": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Numeric columns to plot, one series each (pie and donut take one)"
            },
            "title": { "type": "string" }
        },
        "required": ["type", "x", "y"]
    })
}

fn kind(name: &str) -> Option<ChartKind> {
    Some(match name {
        "bar" => ChartKind::Bar,
        "horizontal_bar" => ChartKind::HorizontalBar,
        "stacked_bar" => ChartKind::StackedBar,
        "line" => ChartKind::Line,
        "area" => ChartKind::Area,
        "pie" => ChartKind::Pie,
        "donut" => ChartKind::Donut,
        "scatter" => ChartKind::Scatter,
        _ => return None,
    })
}

fn name(value: &Json, field: &str) -> Result<String, String> {
    match value.as_str() {
        Some(text) if !text.trim().is_empty() && text.len() <= MAX_NAME => Ok(text.to_owned()),
        _ => Err(format!("{field} must be a column name")),
    }
}

/// Reads the `chart` argument. Unknown fields are ignored.
pub fn parse(value: &Json) -> Result<ChartSpec, String> {
    let object = value.as_object().ok_or("chart must be an object")?;
    let kind_name = object.get("type").and_then(Json::as_str).unwrap_or_default();
    let kind = kind(kind_name).ok_or_else(|| format!("type must be one of {}", KINDS.join(", ")))?;
    let x = name(object.get("x").unwrap_or(&Json::Null), "x")?;
    let y = match object.get("y") {
        Some(Json::String(_)) => vec![name(&object["y"], "y")?],
        Some(Json::Array(items)) => items
            .iter()
            .map(|item| name(item, "each y"))
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err("y must list the value columns".to_owned()),
    };
    if y.is_empty() || y.len() > MAX_SERIES {
        return Err(format!("y must name 1 to {MAX_SERIES} columns"));
    }
    if matches!(kind, ChartKind::Pie | ChartKind::Donut) && y.len() != 1 {
        return Err("a pie or donut takes exactly one y column".to_owned());
    }
    let title = object
        .get("title")
        .and_then(Json::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| t.chars().take(MAX_TITLE).collect());
    Ok(ChartSpec { kind, x, y, title })
}

/// Checks that every named column is in the result.
pub fn check(spec: &ChartSpec, columns: &[ColumnMeta]) -> Result<(), String> {
    let known = |n: &str| columns.iter().any(|c| c.name == n);
    let missing: Vec<&str> = std::iter::once(spec.x.as_str())
        .chain(spec.y.iter().map(String::as_str))
        .filter(|n| !known(n))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        let have: Vec<&str> = columns.iter().map(|c| c.name.as_str()).collect();
        Err(format!(
            "the result has no column {}; it has {}",
            missing.join(", "),
            have.join(", ")
        ))
    }
}

/// The note added to the tool result, so the model knows what the user sees.
pub fn note(spec: &ChartSpec) -> String {
    let kind = KINDS
        .iter()
        .find(|k| kind(k) == Some(spec.kind))
        .copied()
        .unwrap_or("chart")
        .replace('_', " ");
    format!(
        "The user sees this result as a {kind} chart of {} by {}. Summarise it briefly instead of listing every value.",
        spec.y.join(", "),
        spec.x
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use serde_json::json;

    fn column(name: &str) -> ColumnMeta {
        ColumnMeta {
            name: name.into(),
            type_name: "text".into(),
            nullable: None,
        }
    }

    #[test]
    fn reads_a_full_request_and_ignores_extra_fields() {
        let spec = parse(&json!({
            "type": "pie", "x": "region", "y": ["total"], "title": "Sales", "values": [1, 2, 3]
        }))
        .unwrap();
        assert_eq!(
            spec,
            ChartSpec {
                kind: ChartKind::Pie,
                x: "region".into(),
                y: vec!["total".into()],
                title: Some("Sales".into())
            }
        );
        // A single y may be a plain string.
        assert_eq!(
            parse(&json!({"type": "bar", "x": "a", "y": "b"})).unwrap().y,
            vec!["b".to_owned()]
        );
    }

    #[test]
    fn every_kind_is_known() {
        for name in KINDS {
            assert!(
                parse(&json!({"type": name, "x": "a", "y": ["b"]})).is_ok(),
                "{name}"
            );
        }
    }

    #[test]
    fn bad_requests_are_refused_with_a_reason() {
        for bad in [
            json!("pie"),
            json!({"type": "radar", "x": "a", "y": ["b"]}),
            json!({"type": "bar", "y": ["b"]}),
            json!({"type": "bar", "x": "a"}),
            json!({"type": "bar", "x": "a", "y": []}),
            json!({"type": "bar", "x": "a", "y": [1]}),
            json!({"type": "bar", "x": " ", "y": ["b"]}),
            json!({"type": "bar", "x": "a", "y": ["b", "c", "d", "e", "f", "g"]}),
            json!({"type": "pie", "x": "a", "y": ["b", "c"]}),
            json!({"type": "bar", "x": "a".repeat(200), "y": ["b"]}),
        ] {
            assert!(parse(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn long_titles_are_cut() {
        let spec = parse(&json!({"type": "bar", "x": "a", "y": ["b"], "title": "t".repeat(500)})).unwrap();
        assert_eq!(spec.title.unwrap().chars().count(), MAX_TITLE);
    }

    #[test]
    fn columns_must_be_in_the_result() {
        let spec = parse(&json!({"type": "line", "x": "day", "y": ["total", "count"]})).unwrap();
        assert!(check(&spec, &[column("day"), column("total"), column("count")]).is_ok());
        let error = check(&spec, &[column("day"), column("total")]).unwrap_err();
        assert!(error.contains("count") && error.contains("day, total"), "{error}");
    }

    #[test]
    fn the_note_names_the_chart() {
        let spec = parse(&json!({"type": "horizontal_bar", "x": "city", "y": ["n"]})).unwrap();
        assert_eq!(
            note(&spec),
            "The user sees this result as a horizontal bar chart of n by city. Summarise it briefly instead of listing every value."
        );
    }
}
