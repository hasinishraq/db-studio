//! Export and import, with a fake database and real files.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use studio_session::testing::{harness, profile};

fn rows_fixture(h: &studio_session::testing::Harness) {
    let columns = vec![
        ColumnMeta {
            name: "id".into(),
            type_name: "int".into(),
            nullable: None,
        },
        ColumnMeta {
            name: "note".into(),
            type_name: "text".into(),
            nullable: None,
        },
    ];
    let rows = vec![
        vec![Value::Int(1), Value::Text("it's, fine".into())],
        vec![Value::Int(2), Value::Null],
    ];
    *h.driver.rows.lock().unwrap() = Some((columns, rows));
}

async fn export_to(format: ExportFormat, name: &str) -> (tempfile::TempDir, PathBuf, u64) {
    let h = harness();
    rows_fixture(&h);
    let s = h.manager.connect(profile(), None, None).await.unwrap().session_id;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    let rows = export(
        &h.manager,
        s,
        "SELECT * FROM t",
        format,
        &path,
        "\"t\"".into(),
        RunId::new(),
        |_| {},
    )
    .await
    .unwrap();
    (dir, path, rows)
}

#[tokio::test]
async fn csv_export_quotes_properly() {
    let (_dir, path, rows) = export_to(ExportFormat::Csv, "a.csv").await;
    assert_eq!(rows, 2);
    let text = std::fs::read_to_string(path).unwrap();
    assert_eq!(text.replace("\r\n", "\n"), "id,note\n1,\"it's, fine\"\n2,\n");
}

#[tokio::test]
async fn json_lines_export_keeps_types() {
    let (_dir, path, _) = export_to(ExportFormat::JsonLines, "a.jsonl").await;
    let text = std::fs::read_to_string(path).unwrap();
    let first: Json = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(first["id"], 1);
    assert_eq!(first["note"], "it's, fine");
}

#[tokio::test]
async fn sql_export_escapes_values() {
    let (_dir, path, _) = export_to(ExportFormat::SqlInserts, "a.sql").await;
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains("VALUES (1, 'it''s, fine');"), "{text}");
    assert!(text.contains("VALUES (2, NULL);"), "{text}");
}

#[tokio::test]
async fn xlsx_export_writes_a_workbook() {
    let (_dir, path, rows) = export_to(ExportFormat::Xlsx, "a.xlsx").await;
    assert_eq!(rows, 2);
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(&bytes[..2], b"PK", "an xlsx is a zip file");
}

#[tokio::test]
async fn export_refuses_a_write() {
    let h = harness();
    let s = h.manager.connect(profile(), None, None).await.unwrap().session_id;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.csv");
    let error = export(
        &h.manager,
        s,
        "DELETE FROM t",
        ExportFormat::Csv,
        &path,
        "t".into(),
        RunId::new(),
        |_| {},
    )
    .await
    .expect_err("must refuse");
    assert_eq!(error.code, ErrorCode::ApprovalRequired);
    assert!(h.driver.log.lock().unwrap().executed.is_empty());
}

fn csv_file(dir: &tempfile::TempDir, rows: usize) -> PathBuf {
    let path = dir.path().join("in.csv");
    let mut text = String::from("id,name\n");
    for i in 0..rows {
        text.push_str(&format!("{i},name{i}\n"));
    }
    std::fs::write(&path, text).unwrap();
    path
}

fn mappings() -> Vec<ImportMapping> {
    vec![
        ImportMapping {
            csv_column: 0,
            column: "id".into(),
        },
        ImportMapping {
            csv_column: 1,
            column: "name".into(),
        },
    ]
}

fn table() -> TableRef {
    TableRef {
        schema: "public".into(),
        name: "t".into(),
    }
}

#[test]
fn preview_reads_headers_and_at_most_a_hundred_rows() {
    let dir = tempfile::tempdir().unwrap();
    let (headers, rows) = preview(&csv_file(&dir, 150)).unwrap();
    assert_eq!(headers, ["id", "name"]);
    assert_eq!(rows.len(), PREVIEW_ROWS);
}

#[tokio::test]
async fn one_approval_covers_every_batch_in_one_transaction() {
    let h = harness();
    let s = h.manager.connect(profile(), None, None).await.unwrap().session_id;
    let dir = tempfile::tempdir().unwrap();
    let path = csv_file(&dir, 25);
    let plan = plan_import(Engine::Postgres, &path, &table(), &mappings()).unwrap();
    assert_eq!(plan.rows, 25);
    let token = h
        .approvals
        .issue(s, &plan.review_sql, studio_types::Origin::Import);
    let rows = import(&h.manager, s, path, table(), mappings(), 10, token, RunId::new())
        .await
        .unwrap();
    assert_eq!(rows, 25);
    let log = h.driver.log.lock().unwrap();
    let sql: Vec<&str> = log.executed.iter().map(|e| e.0.as_str()).collect();
    assert_eq!(sql.len(), 5, "BEGIN, three batches, COMMIT: {sql:?}");
    assert_eq!(sql[0], "BEGIN");
    assert_eq!(sql[4], "COMMIT");
    assert!(sql[1].starts_with("INSERT INTO \"public\".\"t\" (\"id\", \"name\") VALUES ('0', 'name0')"));
    let entries = h.audit.entries.lock().unwrap();
    assert_eq!(entries.len(), 1, "one audit entry for the whole import");
    assert_eq!(entries[0].rows_affected, Some(25));
    assert_eq!(entries[0].origin, studio_types::Origin::Import);
}

#[tokio::test]
async fn a_bad_batch_rolls_everything_back() {
    let h = harness();
    let s = h.manager.connect(profile(), None, None).await.unwrap().session_id;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("in.csv");
    std::fs::write(&path, "id,name\n1,ok\n2,ok\n3,boom\n").unwrap();
    let plan = plan_import(Engine::Postgres, &path, &table(), &mappings()).unwrap();
    let token = h
        .approvals
        .issue(s, &plan.review_sql, studio_types::Origin::Import);
    let error = import(&h.manager, s, path, table(), mappings(), 1, token, RunId::new()).await;
    assert_eq!(error.err().unwrap().code, ErrorCode::Database);
    let log = h.driver.log.lock().unwrap();
    assert_eq!(log.executed.last().unwrap().0, "ROLLBACK");
    assert!(!log.executed.iter().any(|e| e.0 == "COMMIT"));
    assert_eq!(h.audit.entries.lock().unwrap()[0].outcome, "error");
}

#[tokio::test]
async fn a_file_changed_after_review_fails_the_approval() {
    let h = harness();
    let s = h.manager.connect(profile(), None, None).await.unwrap().session_id;
    let dir = tempfile::tempdir().unwrap();
    let path = csv_file(&dir, 5);
    let plan = plan_import(Engine::Postgres, &path, &table(), &mappings()).unwrap();
    let token = h
        .approvals
        .issue(s, &plan.review_sql, studio_types::Origin::Import);
    std::fs::write(&path, "id,name\n99,evil\n").unwrap();
    let error = import(&h.manager, s, path, table(), mappings(), 10, token, RunId::new()).await;
    assert_eq!(error.err().unwrap().code, ErrorCode::ApprovalMismatch);
    assert!(h.driver.log.lock().unwrap().executed.is_empty());
}

#[tokio::test]
async fn import_needs_an_import_token() {
    let h = harness();
    let s = h.manager.connect(profile(), None, None).await.unwrap().session_id;
    let dir = tempfile::tempdir().unwrap();
    let path = csv_file(&dir, 5);
    let plan = plan_import(Engine::Postgres, &path, &table(), &mappings()).unwrap();
    let token = h
        .approvals
        .issue(s, &plan.review_sql, studio_types::Origin::Editor);
    let error = import(&h.manager, s, path, table(), mappings(), 10, token, RunId::new()).await;
    assert_eq!(error.err().unwrap().code, ErrorCode::ApprovalMismatch);
}

#[test]
fn the_review_text_is_a_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = csv_file(&dir, 5);
    let plan = plan_import(Engine::Postgres, &path, &table(), &mappings()).unwrap();
    let statements = studio_safety::classify_script(&plan.review_sql, Engine::Postgres);
    assert_eq!(statements.len(), 1);
    assert!(!statements[0].verdict.read_only);
}

/// Real engines at the sizes the Phase 15 gates name. Skipped when a server is not configured.
mod at_scale {
    use super::*;
    use crate::setup::drivers;
    use std::sync::Arc;
    use studio_safety::ApprovalBook;
    use studio_session::{NullAuditSink, SessionManager};
    use studio_types::{ConnectionProfile, Origin, TlsMode};

    const IMPORT_ROWS: usize = 100_000;

    fn parse(url: &str, engine: Engine) -> (ConnectionProfile, secrecy::SecretString) {
        let rest = url.split_once("://").unwrap().1;
        let (credentials, location) = rest.split_once('@').unwrap();
        let (user, password) = credentials.split_once(':').unwrap();
        let (address, database) = location.split_once('/').unwrap();
        let (host, port) = address.split_once(':').unwrap();
        let config = ConnectionProfile {
            engine,
            host: host.into(),
            port: port.parse().unwrap(),
            database: database.into(),
            username: user.into(),
            tls_mode: TlsMode::Disable,
            ..profile()
        };
        (config, secrecy::SecretString::from(password.to_owned()))
    }

    struct Target {
        manager: SessionManager,
        approvals: Arc<ApprovalBook>,
        session: SessionId,
        schema: String,
        engine: Engine,
    }

    async fn targets(dir: &tempfile::TempDir) -> Vec<Target> {
        let mut configs = Vec::new();
        for (variable, engine) in [
            ("TEST_PG_URL", Engine::Postgres),
            ("TEST_MYSQL_URL", Engine::MySql),
            ("TEST_MARIADB_URL", Engine::MySql),
        ] {
            match std::env::var(variable) {
                Ok(url) => {
                    let (config, password) = parse(&url, engine);
                    configs.push((config, Some(password)));
                }
                Err(_) => println!("skipped: {variable} not set"),
            }
        }
        let database = dir.path().join("scale.db").to_string_lossy().into_owned();
        configs.push((
            ConnectionProfile {
                engine: Engine::Sqlite,
                host: String::new(),
                port: 0,
                database,
                ..profile()
            },
            None,
        ));
        let mut out = Vec::new();
        for (config, password) in configs {
            let approvals = Arc::new(ApprovalBook::new());
            let manager = SessionManager::new(drivers(), approvals.clone(), Arc::new(NullAuditSink));
            let engine = config.engine;
            let schema = match engine {
                Engine::Postgres => "public".to_owned(),
                Engine::MySql => config.database.clone(),
                Engine::Sqlite => "main".to_owned(),
            };
            let session = manager.connect(config, password, None).await.unwrap().session_id;
            out.push(Target {
                manager,
                approvals,
                session,
                schema,
                engine,
            });
        }
        out
    }

    impl Target {
        async fn admin(&self, sql: &str) {
            let token = self.approvals.issue(self.session, sql, Origin::Editor);
            let (sink, mut inbox) = mpsc::channel(8);
            let drain = tokio::spawn(async move { while inbox.recv().await.is_some() {} });
            self.manager
                .run_user(
                    self.session,
                    sql,
                    Some(token),
                    RunId::new(),
                    RunOptions::default(),
                    sink,
                )
                .await
                .unwrap();
            drain.await.unwrap();
        }

        async fn rows(&self, table: &TableRef) -> Option<u64> {
            self.manager.table_count(self.session, table, &[]).await.unwrap()
        }

        fn table(&self, name: &str) -> TableRef {
            TableRef {
                schema: self.schema.clone(),
                name: name.into(),
            }
        }

        async fn import_file(&self, path: PathBuf, table: &TableRef) -> Result<u64, AppError> {
            let plan = plan_import(self.engine, &path, table, &mappings())?;
            let token = self
                .approvals
                .issue(self.session, &plan.review_sql, Origin::Import);
            import(
                &self.manager,
                self.session,
                path,
                table.clone(),
                mappings(),
                1000,
                token,
                RunId::new(),
            )
            .await
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_100k_import_commits_and_one_bad_row_rolls_everything_back() {
        let dir = tempfile::tempdir().unwrap();
        for target in targets(&dir).await {
            let name = format!("scale_{:x}", SessionId::new().0.as_u128() & 0xffff_ffff);
            let table = target.table(&name);
            let quoted = qualified(target.engine, &table.schema, &table.name);
            target
                .admin(&format!(
                    "CREATE TABLE {quoted} (id INTEGER PRIMARY KEY, name VARCHAR(40) NOT NULL)"
                ))
                .await;

            let good = csv_file(&dir, IMPORT_ROWS);
            let imported = target.import_file(good, &table).await.expect("import");
            assert_eq!(imported, IMPORT_ROWS as u64, "{:?}", target.engine);
            assert_eq!(
                target.rows(&table).await,
                Some(IMPORT_ROWS as u64),
                "{:?}",
                target.engine
            );

            // A duplicate key near the end must undo the thousands of rows before it.
            let path = dir.path().join("bad.csv");
            let mut text = String::from("id,name\n");
            for i in 0..5_000 {
                text.push_str(&format!("{},fresh\n", 1_000_000 + i));
            }
            text.push_str("1,duplicate\n");
            std::fs::write(&path, text).unwrap();
            assert!(
                target.import_file(path, &table).await.is_err(),
                "{:?}",
                target.engine
            );
            assert_eq!(
                target.rows(&table).await,
                Some(IMPORT_ROWS as u64),
                "{:?} kept part of a failed import",
                target.engine
            );

            target.admin(&format!("DROP TABLE {quoted}")).await;
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_million_rows_export_to_every_format_that_can_hold_them() {
        let Ok(url) = std::env::var("TEST_PG_URL") else {
            println!("skipped: TEST_PG_URL not set");
            return;
        };
        let (config, password) = parse(&url, Engine::Postgres);
        let manager = SessionManager::new(drivers(), Arc::new(ApprovalBook::new()), Arc::new(NullAuditSink));
        let session = manager
            .connect(config, Some(password), None)
            .await
            .unwrap()
            .session_id;
        let dir = tempfile::tempdir().unwrap();
        let sql = "SELECT n, md5(n::text) AS hash FROM generate_series(1, 1000000) n";
        for (format, name) in [
            (ExportFormat::Csv, "m.csv"),
            (ExportFormat::JsonLines, "m.jsonl"),
            (ExportFormat::SqlInserts, "m.sql"),
        ] {
            let path = dir.path().join(name);
            let written = export(
                &manager,
                session,
                sql,
                format,
                &path,
                "t".into(),
                RunId::new(),
                |_| {},
            )
            .await
            .unwrap();
            assert_eq!(written, 1_000_000);
            let lines = std::fs::read_to_string(&path).unwrap().lines().count();
            let header = usize::from(format == ExportFormat::Csv);
            assert_eq!(lines, 1_000_000 + header, "{name}");
        }
        let small = "SELECT n FROM generate_series(1, 200000) n";
        let path = dir.path().join("m.xlsx");
        let written = export(
            &manager,
            session,
            small,
            ExportFormat::Xlsx,
            &path,
            "t".into(),
            RunId::new(),
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(written, 200_000);
    }
}

fn picked_columns() -> Vec<ColumnMeta> {
    vec![
        ColumnMeta {
            name: "id".into(),
            type_name: "int".into(),
            nullable: None,
        },
        ColumnMeta {
            name: "note".into(),
            type_name: "text".into(),
            nullable: None,
        },
    ]
}

#[test]
fn picked_rows_export_only_those_rows() {
    let dir = tempfile::tempdir().unwrap();
    let rows = vec![vec![Value::Int(7), Value::Text("kept".into())]];
    for (format, name) in [
        (ExportFormat::Csv, "a.csv"),
        (ExportFormat::JsonLines, "a.jsonl"),
        (ExportFormat::SqlInserts, "a.sql"),
    ] {
        let path = dir.path().join(name);
        let target = "\"t\"".to_owned();
        let written = write_rows(format, &path, Engine::Postgres, target, picked_columns(), &rows).unwrap();
        assert_eq!(written, 1);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("kept"), "{text}");
        assert_eq!(text.matches("kept").count(), 1);
    }
    let path = dir.path().join("a.xlsx");
    write_rows(
        ExportFormat::Xlsx,
        &path,
        Engine::Postgres,
        "t".into(),
        picked_columns(),
        &rows,
    )
    .unwrap();
    assert!(std::fs::metadata(&path).unwrap().len() > 0);
}

#[test]
fn picked_rows_must_fit_their_columns() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.csv");
    let short = vec![vec![Value::Int(1)]];
    let write = |rows: &[Vec<Value>]| {
        write_rows(
            ExportFormat::Csv,
            &path,
            Engine::Sqlite,
            "t".into(),
            picked_columns(),
            rows,
        )
    };
    assert!(write(&short).is_err());
    assert!(write(&[]).is_err());
}

#[test]
fn sql_inserts_refuse_binary_the_grid_cut_short() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.sql");
    let long = Value::bytes(&[7u8; 100]);
    let rows = vec![vec![Value::Int(1), long]];
    let result = write_rows(
        ExportFormat::SqlInserts,
        &path,
        Engine::Sqlite,
        "t".into(),
        picked_columns(),
        &rows,
    );
    assert!(result.is_err());
    assert!(!path.exists());
}

#[test]
fn a_filtered_table_export_is_one_safe_read() {
    let table = TableRef {
        schema: "public".into(),
        name: "t".into(),
    };
    let rule = |value: &str| FilterRule {
        column: "age".into(),
        operator: studio_types::FilterOperator::Gt,
        value: Some(value.into()),
    };
    let casts = HashMap::from([("age".to_owned(), "integer".to_owned())]);
    let sql = table_select(Engine::Postgres, &table, &[rule("30")], &casts).unwrap();
    assert_eq!(
        sql,
        r#"SELECT * FROM "public"."t" WHERE "age" > '30'::text::integer"#
    );

    // A hostile value stays inside its quotes, and the whole thing is still one read.
    let sql = table_select(Engine::Postgres, &table, &[rule("1'; DROP TABLE t; --")], &casts).unwrap();
    let classified = studio_safety::classify_script(&sql, Engine::Postgres);
    assert_eq!(classified.len(), 1);
    assert!(studio_safety::verdicts(&sql, Engine::Postgres)[0].read_only);

    assert!(table_select(Engine::Postgres, &table, &[rule("a\0b")], &casts).is_err());
    let plain = table_select(Engine::MySql, &table, &[], &HashMap::new()).unwrap();
    assert_eq!(plain, "SELECT * FROM `public`.`t`");
}
