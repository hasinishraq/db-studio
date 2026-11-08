#![allow(clippy::unwrap_used)]
use super::*;
use insta::assert_snapshot;
use studio_types::RowUpdate;

fn column(name: &str, data_type: &str, primary_key: bool) -> ColumnInfo {
    ColumnInfo {
        table: "t".into(),
        name: name.into(),
        data_type: data_type.into(),
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

fn columns() -> Vec<ColumnInfo> {
    vec![
        column("id", "integer", true),
        column("name", "character varying", false),
        column("score", "double precision", false),
        column("doc", "jsonb", false),
        column("photo", "bytea", false),
        column("password", "text", false),
    ]
}

fn mixed() -> ChangeSet {
    ChangeSet {
        updates: vec![RowUpdate {
            key: vec![cell("id", Some("1")), cell("name", Some("old"))],
            before: vec![cell("name", Some("old")), cell("score", Some("1.5"))],
            after: vec![cell("name", Some("new")), cell("score", None)],
        }],
        inserts: vec![vec![cell("name", Some("it's"))]],
        deletes: vec![vec![cell("id", Some("7"))], vec![cell("id", Some("8"))]],
    }
}

fn plan(engine: Engine, changes: &ChangeSet) -> Result<PlannedSql, AppError> {
    plan_changes(engine, &table(), &columns(), Some(&Masks::default()), changes)
}

#[test]
fn a_save_is_deletes_then_updates_then_inserts_one_row_each() {
    assert_snapshot!(plan(Engine::Postgres, &mixed()).unwrap().sql, @r#"
    DELETE FROM "s"."t" WHERE "id" = '7';
    DELETE FROM "s"."t" WHERE "id" = '8';
    UPDATE "s"."t" SET "name" = 'new', "score" = NULL WHERE "id" = '1' AND "name" = 'old';
    INSERT INTO "s"."t" ("name") VALUES ('it''s')
    "#);
    assert_snapshot!(plan(Engine::MySql, &mixed()).unwrap().sql, @"
    DELETE FROM `s`.`t` WHERE `id` = '7';
    DELETE FROM `s`.`t` WHERE `id` = '8';
    UPDATE `s`.`t` SET `name` = 'new', `score` = NULL WHERE `id` = '1' AND `name` = 'old';
    INSERT INTO `s`.`t` (`name`) VALUES ('it''s')
    ");
    assert_snapshot!(plan(Engine::Sqlite, &mixed()).unwrap().sql, @r#"
    DELETE FROM "s"."t" WHERE "id" = '7';
    DELETE FROM "s"."t" WHERE "id" = '8';
    UPDATE "s"."t" SET "name" = 'new', "score" = NULL WHERE "id" = '1' AND "name" = 'old';
    INSERT INTO "s"."t" ("name") VALUES ('it''s')
    "#);
}

#[test]
fn every_statement_is_a_row_change_to_the_classifier() {
    for engine in [Engine::Postgres, Engine::MySql, Engine::Sqlite] {
        let sql = plan(engine, &mixed()).unwrap().sql;
        let statements = studio_safety::classify_script(&sql, engine);
        assert_eq!(statements.len(), 4, "{engine:?}: {sql}");
        for s in &statements {
            assert!(
                matches!(
                    s.verdict.kind,
                    studio_types::StatementKind::Insert
                        | studio_types::StatementKind::Update
                        | studio_types::StatementKind::Delete
                ),
                "{engine:?}: {}",
                s.text
            );
        }
    }
}

#[test]
fn a_loaded_null_is_matched_with_is_null() {
    let changes = ChangeSet {
        updates: vec![RowUpdate {
            key: vec![cell("id", Some("1"))],
            before: vec![cell("name", None)],
            after: vec![cell("name", Some("x"))],
        }],
        ..Default::default()
    };
    assert_snapshot!(plan(Engine::Postgres, &changes).unwrap().sql, @r#"UPDATE "s"."t" SET "name" = 'x' WHERE "id" = '1' AND "name" IS NULL"#);
}

#[test]
fn floats_and_json_are_not_matched_by_their_loaded_value() {
    let changes = ChangeSet {
        updates: vec![RowUpdate {
            key: vec![cell("id", Some("1"))],
            before: vec![cell("score", Some("0.1")), cell("doc", Some("{}"))],
            after: vec![cell("score", Some("2")), cell("doc", Some("[]"))],
        }],
        ..Default::default()
    };
    assert_snapshot!(plan(Engine::Postgres, &changes).unwrap().sql, @r#"UPDATE "s"."t" SET "score" = '2', "doc" = '[]' WHERE "id" = '1'"#);
}

#[test]
fn editing_the_key_matches_on_the_loaded_key() {
    let changes = ChangeSet {
        updates: vec![RowUpdate {
            key: vec![cell("id", Some("1"))],
            before: vec![cell("id", Some("1"))],
            after: vec![cell("id", Some("2"))],
        }],
        ..Default::default()
    };
    assert_snapshot!(plan(Engine::MySql, &changes).unwrap().sql, @"UPDATE `s`.`t` SET `id` = '2' WHERE `id` = '1'");
}

#[test]
fn an_empty_new_row_takes_every_default() {
    let changes = ChangeSet {
        inserts: vec![vec![]],
        ..Default::default()
    };
    assert_eq!(
        plan(Engine::Postgres, &changes).unwrap().sql,
        r#"INSERT INTO "s"."t" DEFAULT VALUES"#
    );
    assert_eq!(
        plan(Engine::Sqlite, &changes).unwrap().sql,
        r#"INSERT INTO "s"."t" DEFAULT VALUES"#
    );
    assert_eq!(
        plan(Engine::MySql, &changes).unwrap().sql,
        "INSERT INTO `s`.`t` () VALUES ()"
    );
    for engine in [Engine::Postgres, Engine::MySql, Engine::Sqlite] {
        let sql = plan(engine, &changes).unwrap().sql;
        let statements = studio_safety::classify_script(&sql, engine);
        assert_eq!(statements.len(), 1);
        assert_eq!(
            statements[0].verdict.kind,
            studio_types::StatementKind::Insert,
            "{engine:?}"
        );
    }
}

#[test]
fn hostile_values_stay_inside_their_literal() {
    for engine in [Engine::Postgres, Engine::MySql, Engine::Sqlite] {
        let changes = ChangeSet {
            inserts: vec![vec![cell("name", Some("'); DROP TABLE t; --"))]],
            updates: vec![RowUpdate {
                key: vec![cell("id", Some(r"1\'; DROP TABLE t; --"))],
                before: vec![cell("name", Some("\n; DROP TABLE t"))],
                after: vec![cell("name", Some("a'b"))],
            }],
            ..Default::default()
        };
        let sql = plan(engine, &changes).unwrap().sql;
        assert_eq!(
            studio_safety::classify_script(&sql, engine).len(),
            2,
            "{engine:?}: {sql}"
        );
    }
}

#[test]
fn bad_change_sets_are_refused() {
    let update = |key: Vec<CellValue>, before: Vec<CellValue>, after: Vec<CellValue>| ChangeSet {
        updates: vec![RowUpdate { key, before, after }],
        ..Default::default()
    };
    let refused = [
        ("nothing", ChangeSet::default()),
        (
            "missing key",
            update(vec![], vec![cell("name", None)], vec![cell("name", Some("x"))]),
        ),
        (
            "no new values",
            update(vec![cell("id", Some("1"))], vec![], vec![]),
        ),
        (
            "missing before",
            update(vec![cell("id", Some("1"))], vec![], vec![cell("name", Some("x"))]),
        ),
        (
            "unknown column",
            update(
                vec![cell("id", Some("1"))],
                vec![cell("nope", None)],
                vec![cell("nope", Some("x"))],
            ),
        ),
        (
            "binary",
            update(
                vec![cell("id", Some("1"))],
                vec![cell("photo", None)],
                vec![cell("photo", Some("00"))],
            ),
        ),
        (
            "masked",
            update(
                vec![cell("id", Some("1"))],
                vec![cell("password", None)],
                vec![cell("password", Some("x"))],
            ),
        ),
        (
            "twice",
            update(
                vec![cell("id", Some("1"))],
                vec![cell("name", None)],
                vec![cell("name", Some("x")), cell("name", Some("y"))],
            ),
        ),
        (
            "binary insert",
            ChangeSet {
                inserts: vec![vec![cell("photo", Some("00"))]],
                ..Default::default()
            },
        ),
        (
            "NUL",
            ChangeSet {
                inserts: vec![vec![cell("name", Some("a\0b"))]],
                ..Default::default()
            },
        ),
        (
            "delete without key",
            ChangeSet {
                deletes: vec![vec![cell("name", Some("a"))]],
                ..Default::default()
            },
        ),
    ];
    for (label, changes) in refused {
        assert!(
            plan(Engine::Postgres, &changes).is_err(),
            "{label} should be refused"
        );
    }
}

#[test]
fn a_masked_key_is_refused() {
    let columns = vec![column("token", "text", true), column("name", "text", false)];
    let changes = ChangeSet {
        deletes: vec![vec![cell("token", Some("***"))]],
        ..Default::default()
    };
    assert!(plan_changes(
        Engine::Postgres,
        &table(),
        &columns,
        Some(&Masks::default()),
        &changes
    )
    .is_err());
}

#[test]
fn a_table_without_a_primary_key_is_refused() {
    let columns = vec![column("a", "text", false)];
    let changes = ChangeSet {
        deletes: vec![vec![cell("a", Some("1"))]],
        ..Default::default()
    };
    assert!(plan_changes(Engine::Postgres, &table(), &columns, None, &changes).is_err());
}

#[test]
fn too_many_changes_are_refused() {
    let changes = ChangeSet {
        deletes: vec![vec![cell("id", Some("1"))]; MAX_GRID_CHANGES + 1],
        ..Default::default()
    };
    assert!(plan(Engine::Postgres, &changes).is_err());
    let changes = ChangeSet {
        deletes: vec![vec![cell("id", Some("1"))]; MAX_GRID_CHANGES],
        ..Default::default()
    };
    assert!(plan(Engine::Postgres, &changes).is_ok());
}

#[test]
fn edit_info_locks_masked_and_binary_columns() {
    let info = edit_info(&columns(), Some(&Masks::default()));
    assert_eq!(info.primary_key, vec!["id".to_owned()]);
    assert_eq!(
        info.locked,
        vec![
            LockedColumn {
                name: "photo".into(),
                reason: LockReason::Binary
            },
            LockedColumn {
                name: "password".into(),
                reason: LockReason::Masked
            },
        ]
    );
    let unmasked = edit_info(&columns(), None);
    assert_eq!(unmasked.locked.len(), 1);
}

#[test]
fn type_rules() {
    for binary in [
        "bytea",
        "BLOB",
        "longblob",
        "varbinary(16)",
        "binary(4)",
        "tinyblob",
    ] {
        assert!(is_binary(binary), "{binary}");
    }
    for not_binary in ["text", "bit", "integer"] {
        assert!(!is_binary(not_binary), "{not_binary}");
    }
    for exact in [
        "integer",
        "int(11) unsigned",
        "character varying",
        "VARCHAR(20)",
        "numeric(10,2)",
        "timestamp with time zone",
        "time without time zone",
        "datetime",
        "boolean",
        "uuid",
    ] {
        assert!(compares_exactly(exact), "{exact}");
    }
    for loose in [
        "double precision",
        "real",
        "float",
        "json",
        "jsonb",
        "",
        "USER-DEFINED",
        "ARRAY",
        "interval",
    ] {
        assert!(!compares_exactly(loose), "{loose}");
    }
}
