//! The tools offered to the model.

use crate::provider::ToolSpec;
use serde::Deserialize;
use serde_json::json;

/// Runs one SQL statement.
pub const RUN_SQL: &str = "run_sql";
/// Describes a table.
pub const DESCRIBE_TABLE: &str = "describe_table";
/// Explains a statement.
pub const EXPLAIN_QUERY: &str = "explain_query";
/// Lists tables.
pub const LIST_TABLES: &str = "list_tables";

/// Arguments of `run_sql` and `explain_query`. Unknown fields are ignored, so nothing the model adds,
/// such as `"approved": true`, has any effect.
#[derive(Debug, Deserialize)]
pub struct SqlArgs {
    /// The statement.
    pub sql: String,
    /// A chart request, read by [`crate::chart::parse`]. Only `run_sql` uses it.
    #[serde(default)]
    pub chart: Option<serde_json::Value>,
}

/// Arguments of `describe_table`.
#[derive(Debug, Deserialize)]
pub struct TableArgs {
    /// Schema; the first one if omitted.
    pub schema: Option<String>,
    /// Table.
    pub table: String,
}

/// Arguments of `list_tables`.
#[derive(Debug, Default, Deserialize)]
pub struct SchemaArgs {
    /// Schema; all of them if omitted.
    pub schema: Option<String>,
}

/// The tools, with their JSON schemas.
pub fn specs() -> Vec<ToolSpec> {
    let sql = json!({
        "type": "object",
        "properties": { "sql": { "type": "string", "description": "One SQL statement" } },
        "required": ["sql"]
    });
    vec![
        ToolSpec {
            name: RUN_SQL.to_owned(),
            description: "Run one SQL statement. Reads usually run at once; anything else waits for \
                          the user to approve it. Pass chart to show a read's result as a chart."
                .to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "sql": { "type": "string", "description": "One SQL statement" },
                    "chart": crate::chart::schema()
                },
                "required": ["sql"]
            }),
        },
        ToolSpec {
            name: DESCRIBE_TABLE.to_owned(),
            description: "Show a table's columns, types, keys and indexes. Never rows.".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": { "schema": { "type": "string" }, "table": { "type": "string" } },
                "required": ["table"]
            }),
        },
        ToolSpec {
            name: EXPLAIN_QUERY.to_owned(),
            description: "Show the plan of one statement without running it.".to_owned(),
            parameters: sql,
        },
        ToolSpec {
            name: LIST_TABLES.to_owned(),
            description: "List table names.".to_owned(),
            parameters: json!({ "type": "object", "properties": { "schema": { "type": "string" } } }),
        },
    ]
}

/// Finds fenced SQL blocks, for models without tool calling.
pub fn fenced_sql(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("```") {
        let after = &rest[open + 3..];
        let Some(line_end) = after.find('\n') else { break };
        let language = after[..line_end].trim().to_lowercase();
        let body = &after[line_end + 1..];
        let Some(close) = body.find("```") else { break };
        if language == "sql" {
            let sql = body[..close].trim();
            if !sql.is_empty() {
                found.push(sql.to_owned());
            }
        }
        rest = &body[close + 3..];
    }
    found
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn finds_sql_blocks_only() {
        let text = "Here:\n```sql\nSELECT 1;\n```\nand\n```python\nprint(1)\n```\n```SQL\nSELECT 2\n```";
        assert_eq!(fenced_sql(text), ["SELECT 1;", "SELECT 2"]);
        assert!(fenced_sql("no blocks").is_empty());
        assert!(fenced_sql("```sql\nunterminated").is_empty());
    }

    #[test]
    fn extra_fields_are_ignored() {
        let args: SqlArgs = serde_json::from_str(r#"{"sql":"x","approved":true}"#).expect("parses");
        assert_eq!(args.sql, "x");
    }
}
