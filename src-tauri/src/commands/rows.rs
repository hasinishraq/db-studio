//! Row edits and new tables. SQL is built here, never in the frontend, and goes through the normal
//! review flow, so edits are confirmed and audited like typed SQL.

use super::check_text;
use super::query::grid_masks;
use crate::error::AppResult;
use crate::state::AppState;
use studio_ai::mask::Masks;
use studio_db::sql::{qualified, quote_ident};
use studio_types::{
    AppError, CellValue, ColumnInfo, Engine, NewColumn, PlannedSql, RowChange, SessionId, TableRef,
};
use tauri::State;

/// Renders a value as a string literal. Every engine coerces a quoted literal to the column's type.
pub fn literal(engine: Engine, value: &str) -> Result<String, AppError> {
    if value.contains('\0') {
        return Err(AppError::invalid("a value holds a NUL character"));
    }
    Ok(studio_db::sql::string_literal(engine, value))
}

pub(crate) fn rendered(engine: Engine, value: Option<&str>) -> Result<String, AppError> {
    value.map_or_else(|| Ok("NULL".to_owned()), |v| literal(engine, v))
}

pub(crate) fn check_columns(cells: &[CellValue], known: &[ColumnInfo]) -> Result<(), AppError> {
    for cell in cells {
        if !known.iter().any(|c| c.name == cell.column) {
            return Err(AppError::invalid(format!(
                "the table has no column {}",
                cell.column
            )));
        }
    }
    Ok(())
}

/// Reduces a key to the primary key when there is one, so a row is named exactly. Without a primary
/// key every column is used, and that can match more than one row.
fn identify(
    key: &[CellValue],
    columns: &[ColumnInfo],
    warnings: &mut Vec<String>,
) -> Result<Vec<CellValue>, AppError> {
    let primary: Vec<&ColumnInfo> = columns.iter().filter(|c| c.primary_key).collect();
    if primary.is_empty() {
        const WARNING: &str = "This table has no primary key, so every column identifies the row. \
                               If another row is identical, nothing is saved.";
        if !warnings.iter().any(|w| w == WARNING) {
            warnings.push(WARNING.to_owned());
        }
        return Ok(key.to_vec());
    }
    primary
        .iter()
        .map(|column| {
            key.iter()
                .find(|cell| cell.column == column.name)
                .cloned()
                .ok_or_else(|| AppError::invalid(format!("the key is missing column {}", column.name)))
        })
        .collect()
}

pub(crate) fn where_row(engine: Engine, key: &[CellValue]) -> Result<String, AppError> {
    if key.is_empty() {
        return Err(AppError::invalid("the row has no identifying values"));
    }
    let parts = key
        .iter()
        .map(|cell| {
            let name = quote_ident(engine, &cell.column);
            Ok(match &cell.value {
                Some(value) => format!("{name} = {}", literal(engine, value)?),
                None => format!("{name} IS NULL"),
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    Ok(parts.join(" AND "))
}

/// Refuses cells in columns whose values the grid only shows masked.
pub(crate) fn check_unmasked(cells: &[CellValue], masks: Option<&Masks>) -> Result<(), AppError> {
    if let Some(cell) = cells.iter().find(|c| masks.is_some_and(|m| m.hides(&c.column))) {
        return Err(AppError::invalid(format!(
            "{} is masked, so it cannot be edited or used to find a row",
            cell.column
        )));
    }
    Ok(())
}

/// Plans one change against a table's columns. Each deleted row gets its own statement, so the
/// grid's door can check that every statement changes exactly one row.
pub fn plan_change(
    engine: Engine,
    table: &TableRef,
    columns: &[ColumnInfo],
    masks: Option<&Masks>,
    change: &RowChange,
) -> Result<PlannedSql, AppError> {
    let target = qualified(engine, &table.schema, &table.name);
    let mut warnings = Vec::new();
    let sql = match change {
        RowChange::Insert { values } => {
            check_columns(values, columns)?;
            check_unmasked(values, masks)?;
            if values.is_empty() {
                return Err(AppError::invalid("there is nothing to insert"));
            }
            let names: Vec<String> = values.iter().map(|c| quote_ident(engine, &c.column)).collect();
            let cells = values
                .iter()
                .map(|c| rendered(engine, c.value.as_deref()))
                .collect::<Result<Vec<_>, _>>()?;
            format!(
                "INSERT INTO {target} ({}) VALUES ({})",
                names.join(", "),
                cells.join(", ")
            )
        }
        RowChange::Update { key, values } => {
            check_columns(key, columns)?;
            check_columns(values, columns)?;
            check_unmasked(values, masks)?;
            if values.is_empty() {
                return Err(AppError::invalid("there is nothing to update"));
            }
            let sets = values
                .iter()
                .map(|c| {
                    let value = rendered(engine, c.value.as_deref())?;
                    Ok(format!("{} = {value}", quote_ident(engine, &c.column)))
                })
                .collect::<Result<Vec<_>, AppError>>()?;
            let key = identify(key, columns, &mut warnings)?;
            check_unmasked(&key, masks)?;
            format!(
                "UPDATE {target} SET {} WHERE {}",
                sets.join(", "),
                where_row(engine, &key)?
            )
        }
        RowChange::Delete { keys } => {
            if keys.is_empty() {
                return Err(AppError::invalid("no rows were chosen"));
            }
            let mut rows = Vec::with_capacity(keys.len());
            for key in keys {
                check_columns(key, columns)?;
                let key = identify(key, columns, &mut warnings)?;
                check_unmasked(&key, masks)?;
                rows.push(format!("DELETE FROM {target} WHERE {}", where_row(engine, &key)?));
            }
            rows.join(";\n")
        }
    };
    Ok(PlannedSql { sql, warnings })
}

/// Whether a type name is plain enough to place in DDL. It cannot be quoted, so it must be checked.
fn valid_type(data_type: &str) -> bool {
    !data_type.trim().is_empty()
        && data_type.len() <= 100
        && data_type
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || " _(),".contains(c))
}

/// Plans a CREATE TABLE.
pub fn plan_table(engine: Engine, table: &TableRef, columns: &[NewColumn]) -> Result<PlannedSql, AppError> {
    if columns.is_empty() {
        return Err(AppError::invalid("a table needs at least one column"));
    }
    let mut lines = Vec::with_capacity(columns.len() + 1);
    for column in columns {
        check_text("the column name", &column.name, 255)?;
        if !valid_type(&column.data_type) {
            return Err(AppError::invalid(format!(
                "the type of {} is not valid",
                column.name
            )));
        }
        let mut line = format!(
            "  {} {}",
            quote_ident(engine, &column.name),
            column.data_type.trim()
        );
        if !column.nullable {
            line.push_str(" NOT NULL");
        }
        if let Some(default) = &column.default_value {
            line.push_str(&format!(" DEFAULT {}", literal(engine, default)?));
        }
        lines.push(line);
    }
    let keys: Vec<String> = columns
        .iter()
        .filter(|c| c.primary_key)
        .map(|c| quote_ident(engine, &c.name))
        .collect();
    if !keys.is_empty() {
        lines.push(format!("  PRIMARY KEY ({})", keys.join(", ")));
    }
    let sql = format!(
        "CREATE TABLE {} (\n{}\n)",
        qualified(engine, &table.schema, &table.name),
        lines.join(",\n")
    );
    Ok(PlannedSql {
        sql,
        warnings: Vec::new(),
    })
}

/// Turns a grid edit into reviewed SQL.
#[tauri::command]
#[specta::specta]
pub async fn plan_row_change(
    state: State<'_, AppState>,
    session_id: SessionId,
    table: TableRef,
    change: RowChange,
) -> Result<PlannedSql, AppError> {
    let open = state.sessions.get(session_id).app()?;
    let structure = state.sessions.table_structure(session_id, &table).await.app()?;
    let masks = grid_masks(&state.store, open.profile.id).await?;
    plan_change(open.engine(), &table, &structure.columns, masks.as_ref(), &change)
}

/// Generates CREATE TABLE for the confirm flow.
#[tauri::command]
#[specta::specta]
pub async fn plan_create_table(
    state: State<'_, AppState>,
    session_id: SessionId,
    table: TableRef,
    columns: Vec<NewColumn>,
) -> Result<PlannedSql, AppError> {
    let open = state.sessions.get(session_id).app()?;
    check_text("the table name", &table.name, 255)?;
    plan_table(open.engine(), &table, &columns)
}

/// The longest database name each server accepts.
fn longest_name(engine: Engine) -> usize {
    match engine {
        Engine::MySql => 64,
        _ => 63,
    }
}

/// Writes CREATE DATABASE. The name is quoted, so any text is only ever a name.
pub fn plan_database(engine: Engine, name: &str, charset: Option<&str>) -> Result<PlannedSql, AppError> {
    if engine == Engine::Sqlite {
        return Err(AppError::invalid(
            "a SQLite file holds one database; open another file instead",
        ));
    }
    check_text("the database name", name, longest_name(engine))?;
    if name.chars().any(char::is_control) {
        return Err(AppError::invalid("the database name holds a control character"));
    }
    let mut sql = format!("CREATE DATABASE {}", quote_ident(engine, name));
    match (engine, charset) {
        (Engine::MySql, Some(set)) => {
            // A character set is a bare word, so only letters, digits and underscores may pass.
            if set.is_empty() || set.len() > 32 || !set.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return Err(AppError::invalid("that is not a character set name"));
            }
            sql.push_str(&format!(" CHARACTER SET {set}"));
        }
        (_, Some(_)) => return Err(AppError::invalid("only MySQL takes a character set here")),
        (_, None) => {}
    }
    let warnings = match engine {
        Engine::Postgres => vec!["PostgreSQL cannot create a database inside a transaction.".to_owned()],
        _ => Vec::new(),
    };
    Ok(PlannedSql { sql, warnings })
}

/// Generates CREATE DATABASE for the confirm flow.
#[tauri::command]
#[specta::specta]
pub async fn plan_create_database(
    state: State<'_, AppState>,
    session_id: SessionId,
    name: String,
    charset: Option<String>,
) -> Result<PlannedSql, AppError> {
    let open = state.sessions.get(session_id).app()?;
    plan_database(open.engine(), &name, charset.as_deref())
}

#[cfg(test)]
mod database_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn names_are_quoted_for_each_engine() {
        assert_eq!(
            plan_database(Engine::Postgres, "shop", None).unwrap().sql,
            r#"CREATE DATABASE "shop""#
        );
        let mysql = plan_database(Engine::MySql, "shop", Some("utf8mb4")).unwrap();
        assert_eq!(mysql.sql, "CREATE DATABASE `shop` CHARACTER SET utf8mb4");
    }

    #[test]
    fn a_hostile_name_stays_one_statement() {
        for (engine, name) in [
            (Engine::Postgres, r#"x"; DROP DATABASE studio; --"#),
            (Engine::MySql, "x`; DROP DATABASE studio; --"),
        ] {
            let sql = plan_database(engine, name, None).unwrap().sql;
            assert_eq!(studio_safety::classify_script(&sql, engine).len(), 1, "{sql}");
            assert!(!studio_safety::verdicts(&sql, engine)[0].read_only);
        }
    }

    #[test]
    fn bad_input_is_refused() {
        assert!(plan_database(Engine::Sqlite, "x", None).is_err());
        assert!(plan_database(Engine::Postgres, "  ", None).is_err());
        assert!(plan_database(Engine::Postgres, &"x".repeat(64), None).is_err());
        assert!(plan_database(Engine::MySql, &"x".repeat(64), None).is_ok());
        assert!(plan_database(Engine::MySql, "a\nb", None).is_err());
        assert!(plan_database(Engine::MySql, r"a\b", None).is_ok());
        assert!(plan_database(Engine::MySql, "a\u{0}b", None).is_err());
        assert!(plan_database(Engine::MySql, "x", Some("utf8; DROP")).is_err());
        assert!(plan_database(Engine::Postgres, "x", Some("utf8")).is_err());
    }
}

#[cfg(test)]
mod rows_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use insta::assert_snapshot;

    fn column(name: &str, primary_key: bool) -> ColumnInfo {
        ColumnInfo {
            table: "t".into(),
            name: name.into(),
            data_type: "text".into(),
            nullable: true,
            default_value: None,
            primary_key,
            ordinal: 1,
        }
    }

    fn cell(column: &str, value: Option<&str>) -> CellValue {
        CellValue {
            column: column.into(),
            value: value.map(str::to_owned),
        }
    }

    fn table() -> TableRef {
        TableRef {
            schema: "s".into(),
            name: "t".into(),
        }
    }

    fn keyed() -> Vec<ColumnInfo> {
        vec![column("a", true), column("b", true), column("c", false)]
    }

    fn unkeyed() -> Vec<ColumnInfo> {
        vec![column("a", false), column("b", false)]
    }

    fn sql(engine: Engine, columns: &[ColumnInfo], change: RowChange) -> String {
        plan_change(engine, &table(), columns, None, &change).unwrap().sql
    }

    fn update() -> RowChange {
        RowChange::Update {
            key: vec![cell("a", Some("1")), cell("b", Some("2")), cell("c", Some("old"))],
            values: vec![cell("c", Some("new"))],
        }
    }

    #[test]
    fn composite_keys_use_only_the_primary_key() {
        assert_snapshot!(sql(Engine::Postgres, &keyed(), update()), @r#"UPDATE "s"."t" SET "c" = 'new' WHERE "a" = '1' AND "b" = '2'"#);
        assert_snapshot!(sql(Engine::MySql, &keyed(), update()), @"UPDATE `s`.`t` SET `c` = 'new' WHERE `a` = '1' AND `b` = '2'");
        assert_snapshot!(sql(Engine::Sqlite, &keyed(), update()), @r#"UPDATE "s"."t" SET "c" = 'new' WHERE "a" = '1' AND "b" = '2'"#);
    }

    #[test]
    fn null_keys_use_is_null() {
        let change = RowChange::Delete {
            keys: vec![vec![cell("a", None), cell("b", Some("x"))]],
        };
        assert_snapshot!(sql(Engine::Postgres, &unkeyed(), change), @r#"DELETE FROM "s"."t" WHERE "a" IS NULL AND "b" = 'x'"#);
    }

    #[test]
    fn quotes_in_values_are_escaped() {
        let change = RowChange::Insert {
            values: vec![cell("a", Some("it's")), cell("b", None)],
        };
        assert_snapshot!(sql(Engine::Postgres, &unkeyed(), change.clone()), @r#"INSERT INTO "s"."t" ("a", "b") VALUES ('it''s', NULL)"#);
        assert_snapshot!(sql(Engine::MySql, &unkeyed(), change), @"INSERT INTO `s`.`t` (`a`, `b`) VALUES ('it''s', NULL)");
    }

    #[test]
    fn mysql_backslashes_cannot_escape_the_literal() {
        assert_eq!(literal(Engine::MySql, r"a\").unwrap(), r"'a\\'");
        assert_eq!(literal(Engine::Postgres, r"a\").unwrap(), r"'a\'");
        assert_eq!(literal(Engine::Sqlite, r"a\").unwrap(), r"'a\'");
        assert_eq!(
            literal(Engine::MySql, r"\'; DROP TABLE t; -- ").unwrap(),
            r"'\\''; DROP TABLE t; -- '"
        );
    }

    #[test]
    fn a_literal_stays_one_statement_under_the_classifier() {
        for engine in [Engine::Postgres, Engine::MySql, Engine::Sqlite] {
            for hostile in [
                "'; DROP TABLE t; --",
                r"\'; DROP TABLE t; --",
                "\n; DROP TABLE t",
                "a'b''c",
            ] {
                let change = RowChange::Insert {
                    values: vec![cell("a", Some(hostile))],
                };
                let planned = sql(engine, &unkeyed(), change);
                let statements = studio_safety::classify_script(&planned, engine);
                assert_eq!(statements.len(), 1, "{engine:?} split {planned}");
            }
        }
    }

    #[test]
    fn identifiers_needing_quotes_are_quoted() {
        let columns = vec![column("we\"ird", true), column("we`ird", false)];
        let change = RowChange::Update {
            key: vec![cell("we\"ird", Some("1"))],
            values: vec![cell("we`ird", Some("2"))],
        };
        assert_snapshot!(sql(Engine::Postgres, &columns, change.clone()), @r#"UPDATE "s"."t" SET "we`ird" = '2' WHERE "we""ird" = '1'"#);
        assert_snapshot!(sql(Engine::MySql, &columns, change), @"UPDATE `s`.`t` SET `we``ird` = '2' WHERE `we\"ird` = '1'");
    }

    #[test]
    fn no_key_falls_back_to_all_columns_with_a_warning() {
        let change = RowChange::Update {
            key: vec![cell("a", Some("1")), cell("b", None)],
            values: vec![cell("a", Some("9"))],
        };
        let planned = plan_change(Engine::Sqlite, &table(), &unkeyed(), None, &change).unwrap();
        assert_snapshot!(planned.sql, @r#"UPDATE "s"."t" SET "a" = '9' WHERE "a" = '1' AND "b" IS NULL"#);
        assert_eq!(planned.warnings.len(), 1);
    }

    #[test]
    fn each_deleted_row_is_its_own_statement() {
        let change = RowChange::Delete {
            keys: vec![
                vec![cell("a", Some("1")), cell("b", Some("2"))],
                vec![cell("a", Some("3")), cell("b", Some("4"))],
            ],
        };
        let planned = sql(Engine::MySql, &keyed(), change);
        assert_eq!(
            planned,
            "DELETE FROM `s`.`t` WHERE `a` = '1' AND `b` = '2';\nDELETE FROM `s`.`t` WHERE `a` = '3' AND `b` = '4'"
        );
        assert_eq!(studio_safety::classify_script(&planned, Engine::MySql).len(), 2);
    }

    #[test]
    fn masked_columns_are_neither_written_nor_used_as_keys() {
        let masks = Masks::default();
        let columns = vec![
            column("id", true),
            column("password", false),
            column("name", false),
        ];
        let write = RowChange::Update {
            key: vec![cell("id", Some("1"))],
            values: vec![cell("password", Some("x"))],
        };
        assert!(plan_change(Engine::Postgres, &table(), &columns, Some(&masks), &write).is_err());
        assert!(plan_change(Engine::Postgres, &table(), &columns, None, &write).is_ok());
        let unkeyed = vec![column("password", false), column("name", false)];
        let delete = RowChange::Delete {
            keys: vec![vec![cell("password", Some("***")), cell("name", Some("a"))]],
        };
        assert!(plan_change(Engine::Postgres, &table(), &unkeyed, Some(&masks), &delete).is_err());
    }

    #[test]
    fn a_partial_primary_key_is_refused() {
        let change = RowChange::Delete {
            keys: vec![vec![cell("a", Some("1"))]],
        };
        assert!(plan_change(Engine::Postgres, &table(), &keyed(), None, &change).is_err());
    }

    #[test]
    fn unknown_columns_and_empty_changes_are_refused() {
        let unknown = RowChange::Insert {
            values: vec![cell("nope", Some("1"))],
        };
        assert!(plan_change(Engine::Postgres, &table(), &keyed(), None, &unknown).is_err());
        let empty = RowChange::Insert { values: vec![] };
        assert!(plan_change(Engine::Postgres, &table(), &keyed(), None, &empty).is_err());
        let none = RowChange::Delete { keys: vec![] };
        assert!(plan_change(Engine::Postgres, &table(), &keyed(), None, &none).is_err());
    }

    fn new_column(name: &str, data_type: &str, primary_key: bool) -> NewColumn {
        NewColumn {
            name: name.into(),
            data_type: data_type.into(),
            nullable: !primary_key,
            primary_key,
            default_value: None,
        }
    }

    #[test]
    fn create_table_is_generated_per_dialect() {
        let columns = vec![
            new_column("id", "INTEGER", true),
            NewColumn {
                default_value: Some("n/a".into()),
                ..new_column("note", "VARCHAR(20)", false)
            },
        ];
        assert_snapshot!(plan_table(Engine::Postgres, &table(), &columns).unwrap().sql, @r#"
        CREATE TABLE "s"."t" (
          "id" INTEGER NOT NULL,
          "note" VARCHAR(20) DEFAULT 'n/a',
          PRIMARY KEY ("id")
        )
        "#);
        assert_snapshot!(plan_table(Engine::MySql, &table(), &columns).unwrap().sql, @"
        CREATE TABLE `s`.`t` (
          `id` INTEGER NOT NULL,
          `note` VARCHAR(20) DEFAULT 'n/a',
          PRIMARY KEY (`id`)
        )
        ");
    }

    #[test]
    fn a_type_cannot_smuggle_sql() {
        let bad = new_column("a", "INT); DROP TABLE t; --", false);
        assert!(plan_table(Engine::Postgres, &table(), &[bad]).is_err());
        assert!(plan_table(Engine::Postgres, &table(), &[]).is_err());
    }
}
