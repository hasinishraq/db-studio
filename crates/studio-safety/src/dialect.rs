//! Maps engines to `sqlparser` dialects.

use sqlparser::dialect::{Dialect as ParserDialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect};
use studio_types::Dialect;

/// The parser dialect for an engine.
pub fn parser_dialect(dialect: Dialect) -> Box<dyn ParserDialect> {
    match dialect {
        Dialect::Postgres => Box::new(PostgreSqlDialect {}),
        Dialect::MySql => Box::new(MySqlDialect {}),
        Dialect::Sqlite => Box::new(SQLiteDialect {}),
    }
}
