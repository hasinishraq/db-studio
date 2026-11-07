//! Reads the tables out of a script without running it, for the schema preview.

use crate::classify::isolated;
use crate::split::split_script;
use sqlparser::ast::{
    AlterTableOperation, ColumnDef, ColumnOption, CreateTable, Expr, ForeignKeyConstraint, IndexColumn,
    ObjectName, Statement, TableConstraint,
};
use studio_types::{Dialect, OutlineColumn, OutlineKey, OutlineTable, SchemaOutline};

fn last_name(name: &ObjectName) -> String {
    name.0
        .last()
        .and_then(|part| part.as_ident())
        .map_or_else(|| name.to_string(), |ident| ident.value.clone())
}

fn index_column(column: &IndexColumn) -> String {
    match &column.column.expr {
        Expr::Identifier(ident) => ident.value.clone(),
        other => other.to_string(),
    }
}

fn key(constraint: &ForeignKeyConstraint, local: Vec<String>) -> OutlineKey {
    OutlineKey {
        columns: local,
        referenced_table: last_name(&constraint.foreign_table),
        referenced_columns: constraint
            .referred_columns
            .iter()
            .map(|c| c.value.clone())
            .collect(),
    }
}

fn apply_constraint(table: &mut OutlineTable, constraint: &TableConstraint) {
    match constraint {
        TableConstraint::PrimaryKey(primary) => {
            let names: Vec<String> = primary.columns.iter().map(index_column).collect();
            for column in &mut table.columns {
                column.primary_key |= names.contains(&column.name);
            }
        }
        TableConstraint::ForeignKey(foreign) => {
            let local = foreign.columns.iter().map(|c| c.value.clone()).collect();
            table.foreign_keys.push(key(foreign, local));
        }
        _ => {}
    }
}

fn column(table: &mut OutlineTable, def: &ColumnDef) {
    let mut column = OutlineColumn {
        name: def.name.value.clone(),
        data_type: def.data_type.to_string(),
        primary_key: false,
    };
    for option in &def.options {
        match &option.option {
            ColumnOption::PrimaryKey(_) => column.primary_key = true,
            ColumnOption::ForeignKey(foreign) => {
                table
                    .foreign_keys
                    .push(key(foreign, vec![def.name.value.clone()]));
            }
            _ => {}
        }
    }
    table.columns.push(column);
}

fn table(create: &CreateTable) -> OutlineTable {
    let mut table = OutlineTable {
        name: last_name(&create.name),
        columns: Vec::new(),
        foreign_keys: Vec::new(),
    };
    for def in &create.columns {
        column(&mut table, def);
    }
    for constraint in &create.constraints {
        apply_constraint(&mut table, constraint);
    }
    table
}

fn build(sql: &str, dialect: Dialect) -> SchemaOutline {
    let mut outline = SchemaOutline::default();
    for piece in split_script(sql, dialect) {
        match piece.statement.as_ref() {
            Some(Statement::CreateTable(create)) => outline.tables.push(table(create)),
            // Dump tools add keys after the tables, so both ends exist by then.
            Some(Statement::AlterTable(alter)) => {
                let name = last_name(&alter.name);
                if let Some(found) = outline.tables.iter_mut().find(|t| t.name == name) {
                    for operation in &alter.operations {
                        if let AlterTableOperation::AddConstraint { constraint, .. } = operation {
                            apply_constraint(found, constraint);
                        }
                    }
                }
            }
            Some(_) => {}
            None => outline.unread += 1,
        }
    }
    outline
}

/// The tables and keys a script would create. Nothing here runs or judges the SQL: the review that
/// comes before applying a file is the classifier's job.
pub fn schema_outline(sql: &str, dialect: Dialect) -> SchemaOutline {
    isolated(|| build(sql, dialect)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_inline_keys() {
        let sql = "CREATE TABLE a (id INT PRIMARY KEY, name TEXT);
                   CREATE TABLE b (id INT PRIMARY KEY, a_id INT REFERENCES a (id));";
        let outline = schema_outline(sql, Dialect::Postgres);
        assert_eq!(outline.tables.len(), 2);
        assert!(outline.tables[0].columns[0].primary_key);
        assert!(!outline.tables[0].columns[1].primary_key);
        assert_eq!(outline.tables[1].foreign_keys[0].columns, ["a_id"]);
        assert_eq!(outline.tables[1].foreign_keys[0].referenced_table, "a");
        assert_eq!(outline.tables[1].foreign_keys[0].referenced_columns, ["id"]);
    }

    #[test]
    fn reads_keys_added_later_the_way_pg_dump_writes_them() {
        let sql = r#"SET statement_timeout = 0;
            CREATE TABLE public.orders (id integer NOT NULL, user_id integer);
            CREATE TABLE public.users (id integer NOT NULL);
            ALTER TABLE ONLY public.users ADD CONSTRAINT users_pkey PRIMARY KEY (id);
            ALTER TABLE ONLY public.orders
                ADD CONSTRAINT orders_user FOREIGN KEY (user_id) REFERENCES public.users(id);"#;
        let outline = schema_outline(sql, Dialect::Postgres);
        assert_eq!(outline.unread, 0);
        let names: Vec<&str> = outline.tables.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["orders", "users"]);
        assert!(outline.tables[1].columns[0].primary_key);
        assert_eq!(outline.tables[0].foreign_keys[0].referenced_table, "users");
    }

    #[test]
    fn reads_mysql_quoting_and_table_keys() {
        let sql = "CREATE TABLE `shop`.`items` (
              `id` int NOT NULL AUTO_INCREMENT,
              `owner` int DEFAULT NULL,
              PRIMARY KEY (`id`),
              KEY `owner` (`owner`),
              CONSTRAINT `fk` FOREIGN KEY (`owner`) REFERENCES `people` (`id`)
            ) ENGINE=InnoDB;";
        let outline = schema_outline(sql, Dialect::MySql);
        assert_eq!(outline.tables[0].name, "items");
        assert!(outline.tables[0].columns[0].primary_key);
        assert_eq!(outline.tables[0].foreign_keys[0].referenced_table, "people");
    }

    #[test]
    fn counts_what_it_cannot_read_and_keeps_going() {
        let sql = "THIS IS NOT SQL; CREATE TABLE a (id INT);";
        let outline = schema_outline(sql, Dialect::Sqlite);
        assert_eq!(outline.unread, 1);
        assert_eq!(outline.tables.len(), 1);
    }

    #[test]
    fn a_key_for_a_table_it_never_saw_is_ignored() {
        let sql = "ALTER TABLE ghost ADD CONSTRAINT k FOREIGN KEY (a) REFERENCES b (id);";
        assert!(schema_outline(sql, Dialect::Postgres).tables.is_empty());
    }

    #[test]
    fn deep_nesting_cannot_crash_it() {
        let sql = format!("SELECT {}1{}", "(".repeat(100_000), ")".repeat(100_000));
        assert!(schema_outline(&sql, Dialect::Postgres).tables.is_empty());
    }
}
