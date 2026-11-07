//! Functions with side effects, per dialect. They may be read by a human but never run unattended.

use sqlparser::ast::{ObjectName, ObjectNamePart};
use studio_types::Dialect;

const POSTGRES_EXACT: &[&str] = &[
    "nextval",
    "setval",
    "set_config",
    "pg_notify",
    "pg_terminate_backend",
    "pg_cancel_backend",
    "pg_reload_conf",
    "pg_rotate_logfile",
    "pg_switch_wal",
    "pg_create_restore_point",
    "pg_read_file",
    "pg_read_binary_file",
    "pg_ls_dir",
    "pg_stat_file",
    "pg_file_write",
    "pg_promote",
    "pg_wal_replay_pause",
    "pg_wal_replay_resume",
    "pg_export_snapshot",
    "pg_import_system_collations",
    "pg_stat_reset",
    "pg_stat_reset_shared",
    // These run a query given as a string, so a write can hide in their argument.
    "query_to_xml",
    "query_to_xmlschema",
    "query_to_xml_and_xmlschema",
    "table_to_xml",
    "table_to_xmlschema",
    "table_to_xml_and_xmlschema",
    "schema_to_xml",
    "schema_to_xmlschema",
    "schema_to_xml_and_xmlschema",
    "database_to_xml",
    "database_to_xmlschema",
    "database_to_xml_and_xmlschema",
    "cursor_to_xml",
    "cursor_to_xmlschema",
    "ts_stat",
    "ts_rewrite",
];

const POSTGRES_PREFIXES: &[&str] = &[
    "pg_sleep",
    "dblink",
    "lo_",
    "pg_advisory",
    "pg_try_advisory",
    "pg_logical_",
    "pg_replication_",
    "pg_create_",
    "pg_drop_",
    "pg_ls_",
];

const MYSQL_EXACT: &[&str] = &[
    "sleep",
    "benchmark",
    "get_lock",
    "release_lock",
    "release_all_locks",
    "is_free_lock",
    "is_used_lock",
    "load_file",
    "master_pos_wait",
    "source_pos_wait",
    "wait_for_executed_gtid_set",
    "wait_until_sql_thread_after_gtids",
    "sys_exec",
    "sys_eval",
    "sys_get",
    "sys_set",
    "master_gtid_wait",
];

const SQLITE_EXACT: &[&str] = &[
    "load_extension",
    "writefile",
    "readfile",
    "edit",
    "fts3_tokenizer",
    "sqlite_rename_table",
];

/// Whether an already lowercased, unqualified function name is on the dialect's deny-list.
pub fn is_denied(name: &str, dialect: Dialect) -> bool {
    match dialect {
        Dialect::Postgres => {
            POSTGRES_EXACT.contains(&name) || POSTGRES_PREFIXES.iter().any(|p| name.starts_with(p))
        }
        Dialect::MySql => MYSQL_EXACT.contains(&name),
        Dialect::Sqlite => SQLITE_EXACT.contains(&name),
    }
}

/// The last part of a possibly schema-qualified name, lowercased.
fn base_name(name: &ObjectName) -> Option<String> {
    name.0.last().map(|part| match part {
        ObjectNamePart::Identifier(ident) => ident.value.to_lowercase(),
        ObjectNamePart::Function(function) => function.name.value.to_lowercase(),
    })
}

/// Returns the name when `name` is denied, with or without a schema prefix.
pub(crate) fn denied_name(name: &ObjectName, dialect: Dialect) -> Option<String> {
    base_name(name).filter(|base| is_denied(base, dialect))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn exact_and_prefix_matches() {
        assert!(is_denied("pg_sleep", Dialect::Postgres));
        assert!(is_denied("pg_sleep_for", Dialect::Postgres));
        assert!(is_denied("lo_import", Dialect::Postgres));
        assert!(is_denied("query_to_xml", Dialect::Postgres));
        assert!(!is_denied("count", Dialect::Postgres));
        assert!(!is_denied("lower", Dialect::Postgres));
        assert!(is_denied("sleep", Dialect::MySql));
        assert!(!is_denied("sleep", Dialect::Postgres));
        assert!(is_denied("load_extension", Dialect::Sqlite));
    }
}
