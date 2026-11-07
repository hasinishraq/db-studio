//! Plain-language reasons a statement is high risk.

use sqlparser::ast::{Statement, Truncate};

/// Reasons `statement` is high risk; empty when it is not.
pub fn assess(statement: &Statement) -> Vec<String> {
    let mut reasons = Vec::new();
    match statement {
        Statement::Truncate(Truncate { .. }) => {
            reasons.push("TRUNCATE removes every row and cannot be filtered.".to_owned());
        }
        Statement::Delete(delete) if delete.selection.is_none() && delete.limit.is_none() => {
            reasons.push("DELETE without WHERE removes every row in the table.".to_owned());
        }
        Statement::Update(update) if update.selection.is_none() => {
            reasons.push("UPDATE without WHERE changes every row in the table.".to_owned());
        }
        Statement::AlterTable(alter) => {
            if alter
                .operations
                .iter()
                .any(|op| format!("{op:?}").starts_with("Drop"))
            {
                reasons.push("ALTER ... DROP permanently removes part of the table.".to_owned());
            }
        }
        Statement::Grant(_) | Statement::Revoke(_) | Statement::Deny(_) => {
            reasons.push("This changes who can access the database.".to_owned());
        }
        other => {
            let text = other.to_string();
            let first = text.split_whitespace().next().unwrap_or_default();
            if first.eq_ignore_ascii_case("DROP") {
                reasons.push("DROP permanently removes the object and its data.".to_owned());
            }
        }
    }
    reasons
}
