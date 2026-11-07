//! Schema scripts, proven by rebuilding a schema from one on real engines.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use driver_postgres::PostgresDriver;
use driver_sqlite::SqliteDriver;
use secrecy::SecretString;
use studio_db::{ConnectConfig, Driver, ExecOptions};
use studio_types::{ForeignKeyInfo, TableMetadata, TlsMode};
use tokio::sync::mpsc;

async fn run(session: &dyn Session, sql: &str) {
    let (sink, mut inbox) = mpsc::channel(8);
    let drain = tokio::spawn(async move { while inbox.recv().await.is_some() {} });
    session
        .execute(sql, ExecOptions::default(), sink)
        .await
        .unwrap_or_else(|e| panic!("{e}: {sql}"));
    drain.await.unwrap();
}

async fn run_script(session: &dyn Session, script: &str, engine: Engine) {
    for statement in studio_safety::classify_script(script, engine) {
        run(session, &statement.text).await;
    }
}

fn structure(name: &str, references: &[&str]) -> TableStructure {
    TableStructure {
        table: TableRef {
            schema: "s".into(),
            name: name.into(),
        },
        kind: ObjectKind::Table,
        columns: vec![],
        indexes: vec![],
        foreign_keys: references
            .iter()
            .map(|target| ForeignKeyInfo {
                name: format!("{name}_{target}"),
                columns: vec!["x".into()],
                referenced_schema: "s".into(),
                referenced_table: (*target).into(),
                referenced_columns: vec!["id".into()],
                definition: None,
            })
            .collect(),
        constraints: vec![],
        metadata: TableMetadata::default(),
        ddl: Some(format!("CREATE TABLE {name} (x INT);")),
    }
}

fn position(script: &str, name: &str) -> usize {
    script.find(&format!("CREATE TABLE {name} ")).unwrap()
}

#[test]
fn a_referenced_table_comes_first() {
    let tables = [
        structure("a", &["b"]),
        structure("b", &["c"]),
        structure("c", &[]),
    ];
    let script = schema_script(Engine::MySql, "s", &tables);
    assert!(position(&script, "c") < position(&script, "b"));
    assert!(position(&script, "b") < position(&script, "a"));
}

#[test]
fn cycles_and_self_references_lose_no_table() {
    let tables = [
        structure("a", &["b"]),
        structure("b", &["a"]),
        structure("self_ref", &["self_ref"]),
        structure("outside", &["not_in_this_schema"]),
    ];
    let script = schema_script(Engine::MySql, "s", &tables);
    for name in ["a", "b", "self_ref", "outside"] {
        assert_eq!(
            script.matches(&format!("CREATE TABLE {name} ")).count(),
            1,
            "{name}"
        );
    }
}

#[test]
fn every_statement_ends_once() {
    let script = schema_script(Engine::Sqlite, "main", &[structure("a", &[])]);
    assert!(script.contains("CREATE TABLE a (x INT);\n"));
    assert!(!script.contains(";;"));
}

#[test]
fn reading_a_file_handles_a_byte_order_mark_and_refuses_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.sql");

    std::fs::write(&path, "\u{feff}CREATE TABLE a (id INT);").unwrap();
    let file = read_script(&path).unwrap();
    assert_eq!(file.sql, "CREATE TABLE a (id INT);");
    assert_eq!(file.name, "a.sql");

    // What a PowerShell redirect produces.
    std::fs::write(&path, [0xFF, 0xFE, b'S', 0]).unwrap();
    assert!(read_script(&path).is_err());

    std::fs::write(&path, vec![b' '; MAX_SQL_BYTES + 1]).unwrap();
    assert!(read_script(&path).is_err());
}

const SQLITE_SCHEMA: &str = "
    CREATE TABLE child (id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES parent (id), note TEXT);
    CREATE TABLE parent (id INTEGER PRIMARY KEY, code TEXT UNIQUE);
    CREATE INDEX child_note ON child (note);
    CREATE VIEW child_view AS SELECT id FROM child;";

#[tokio::test]
async fn sqlite_rebuilds_from_its_own_script() {
    let dir = tempfile::tempdir().unwrap();
    let open = |name: &str| ConnectConfig::new("", 0, dir.path().join(name).to_string_lossy().into_owned());
    let source = SqliteDriver.connect(&open("a.db")).await.unwrap();
    run_script(source.as_ref(), SQLITE_SCHEMA, Engine::Sqlite).await;

    let structures = read_structures(source.as_ref(), "main").await.unwrap();
    let script = schema_script(Engine::Sqlite, "main", &structures);

    let copy = SqliteDriver.connect(&open("b.db")).await.unwrap();
    run_script(copy.as_ref(), &script, Engine::Sqlite).await;
    let rebuilt = read_structures(copy.as_ref(), "main").await.unwrap();

    let names = |list: &[TableStructure]| list.iter().map(|s| s.table.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&rebuilt), names(&structures));
    let child = rebuilt.iter().find(|s| s.table.name == "child").unwrap();
    assert_eq!(child.foreign_keys[0].referenced_table, "parent");
    assert!(
        child.indexes.iter().any(|i| i.name == "child_note"),
        "{:?}",
        child.indexes
    );
}

const PG_SCHEMA: &str = r#"
    DROP SCHEMA IF EXISTS studio_roundtrip CASCADE;
    CREATE SCHEMA studio_roundtrip;
    CREATE TABLE studio_roundtrip.a_child (
        id SERIAL PRIMARY KEY,
        parent_id INTEGER NOT NULL,
        note TEXT
    );
    CREATE TABLE studio_roundtrip.b_parent (id SERIAL PRIMARY KEY, code TEXT UNIQUE);
    ALTER TABLE studio_roundtrip.a_child
        ADD CONSTRAINT a_child_parent FOREIGN KEY (parent_id) REFERENCES studio_roundtrip.b_parent (id)
        ON DELETE CASCADE;
    CREATE INDEX a_child_note ON studio_roundtrip.a_child (note);
    CREATE VIEW studio_roundtrip.c_view AS SELECT id FROM studio_roundtrip.a_child;"#;

#[tokio::test]
async fn postgres_rebuilds_from_its_own_script() {
    let Ok(url) = std::env::var("TEST_PG_URL") else {
        println!("skipped: TEST_PG_URL not set");
        return;
    };
    let rest = url.split_once("://").unwrap().1;
    let (credentials, location) = rest.split_once('@').unwrap();
    let (user, password) = credentials.split_once(':').unwrap();
    let (address, database) = location.split_once('/').unwrap();
    let (host, port) = address.split_once(':').unwrap();
    let mut cfg = ConnectConfig::new(host, port.parse().unwrap(), database);
    cfg.username = user.to_owned();
    cfg.password = Some(SecretString::from(password.to_owned()));
    cfg.tls.mode = TlsMode::Disable;

    let session = PostgresDriver.connect(&cfg).await.unwrap();
    run_script(session.as_ref(), PG_SCHEMA, Engine::Postgres).await;
    let before = read_structures(session.as_ref(), "studio_roundtrip")
        .await
        .unwrap();
    let script = schema_script(Engine::Postgres, "studio_roundtrip", &before);

    run(session.as_ref(), "DROP SCHEMA studio_roundtrip CASCADE").await;
    run_script(session.as_ref(), &script, Engine::Postgres).await;

    let after = read_structures(session.as_ref(), "studio_roundtrip")
        .await
        .unwrap();
    let names = |list: &[TableStructure]| list.iter().map(|s| s.table.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&after), names(&before));
    let child = after.iter().find(|s| s.table.name == "a_child").unwrap();
    assert_eq!(child.foreign_keys[0].referenced_table, "b_parent");
    let definition = child.foreign_keys[0].definition.as_deref().unwrap();
    assert!(definition.contains("ON DELETE CASCADE"), "{definition}");
    assert!(child.indexes.iter().any(|i| i.name == "a_child_note"));
    let parent = after.iter().find(|s| s.table.name == "b_parent").unwrap();
    assert!(
        parent.constraints.iter().any(|c| c.kind == "UNIQUE"),
        "{:?}",
        parent.constraints
    );
    run(
        session.as_ref(),
        "INSERT INTO studio_roundtrip.b_parent (code) VALUES ('x')",
    )
    .await;
    // The filtered export's SQL is valid on the real server, with casts taken from the structure.
    let table = parent.table.clone();
    let id_filter = studio_types::FilterRule {
        column: "id".into(),
        operator: studio_types::FilterOperator::Gt,
        value: Some("0".into()),
    };
    let casts = parent
        .columns
        .iter()
        .map(|c| (c.name.clone(), c.data_type.clone()))
        .collect();
    let filtered =
        super::super::transfer::table_select(Engine::Postgres, &table, &[id_filter], &casts).unwrap();
    run(session.as_ref(), &filtered).await;
    run(session.as_ref(), "DROP SCHEMA studio_roundtrip CASCADE").await;
    session.close().await;
}
