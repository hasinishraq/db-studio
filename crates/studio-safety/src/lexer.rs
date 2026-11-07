//! A small dialect-exact scanner for strings, quoted identifiers and comments.
//!
//! It must agree with the server about where literals and comments end. A disagreement could hide
//! a statement, so each rule below follows the engine's own lexing, not a generic one.

use studio_types::Dialect;

/// A region of SQL that is not code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Skip {
    /// First byte after the region.
    pub end: usize,
    /// Whether this is a comment rather than a literal.
    pub comment: bool,
    /// Whether this is an executable comment, which the server treats as code.
    pub executable: bool,
}

impl Skip {
    fn literal(end: usize) -> Self {
        Self {
            end,
            comment: false,
            executable: false,
        }
    }

    fn comment(end: usize) -> Self {
        Self {
            end,
            comment: true,
            executable: false,
        }
    }
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

/// Finds the end of a quoted region opened at `open`. A doubled quote closes and reopens, so it
/// needs no special case. With `backslash`, a backslash escapes the next byte.
fn end_of_quote(sql: &[u8], open: usize, quote: u8, backslash: bool) -> usize {
    let mut i = open + 1;
    while i < sql.len() {
        match sql[i] {
            b'\\' if backslash => i += 2,
            byte if byte == quote => return i + 1,
            _ => i += 1,
        }
    }
    sql.len()
}

fn end_of_line(sql: &[u8], from: usize) -> usize {
    sql[from..]
        .iter()
        .position(|b| *b == b'\n')
        .map_or(sql.len(), |p| from + p + 1)
}

/// Finds the end of a block comment. PostgreSQL nests them; MySQL and SQLite do not.
fn end_of_block_comment(sql: &[u8], open: usize, nested: bool) -> usize {
    let mut depth = 1;
    let mut i = open + 2;
    while i + 1 < sql.len() {
        if sql[i] == b'*' && sql[i + 1] == b'/' {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return i;
            }
        } else if nested && sql[i] == b'/' && sql[i + 1] == b'*' {
            depth += 1;
            i += 2;
        } else {
            i += 1;
        }
    }
    sql.len()
}

/// Recognises a PostgreSQL dollar quote at `open` and returns its end.
fn dollar_quote(sql: &[u8], open: usize) -> Option<usize> {
    if open > 0 && is_ident_byte(sql[open - 1]) {
        return None;
    }
    let tag_end = sql[open + 1..]
        .iter()
        .position(|b| !is_ident_byte(*b))
        .map(|p| open + 1 + p)?;
    if sql[tag_end] != b'$' || sql.get(open + 1).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let tag = &sql[open..=tag_end];
    let body = tag_end + 1;
    let close = sql[body..].windows(tag.len()).position(|w| w == tag);
    Some(close.map_or(sql.len(), |p| body + p + tag.len()))
}

/// Whether the `--` at `at` starts a comment. MySQL needs whitespace or the end after it, so
/// `SELECT 1--1` there is arithmetic, and anything after it is still code.
fn dash_comment_starts(sql: &[u8], at: usize, dialect: Dialect) -> bool {
    if dialect != Dialect::MySql {
        return true;
    }
    match sql.get(at + 2) {
        None => true,
        Some(next) => next.is_ascii_whitespace() || next.is_ascii_control(),
    }
}

/// Whether the `'` at `at` opens a PostgreSQL escape string (`E'...'`).
fn is_escape_string(sql: &[u8], at: usize) -> bool {
    at > 0 && matches!(sql[at - 1], b'e' | b'E') && (at == 1 || !is_ident_byte(sql[at - 2]))
}

/// If a string, quoted identifier or comment starts at `i`, returns where it ends.
pub(crate) fn skip_at(sql: &[u8], i: usize, dialect: Dialect) -> Option<Skip> {
    let next = sql.get(i + 1).copied();
    match sql[i] {
        b'\'' => {
            let backslash = match dialect {
                Dialect::MySql => true,
                Dialect::Postgres => is_escape_string(sql, i),
                Dialect::Sqlite => false,
            };
            Some(Skip::literal(end_of_quote(sql, i, b'\'', backslash)))
        }
        b'"' => Some(Skip::literal(end_of_quote(
            sql,
            i,
            b'"',
            dialect == Dialect::MySql,
        ))),
        b'`' if dialect != Dialect::Postgres => Some(Skip::literal(end_of_quote(sql, i, b'`', false))),
        b'[' if dialect == Dialect::Sqlite => Some(Skip::literal(end_of_quote(sql, i, b']', false))),
        b'-' if next == Some(b'-') && dash_comment_starts(sql, i, dialect) => {
            Some(Skip::comment(end_of_line(sql, i)))
        }
        b'#' if dialect == Dialect::MySql => Some(Skip::comment(end_of_line(sql, i))),
        b'/' if next == Some(b'*') => {
            let end = end_of_block_comment(sql, i, dialect == Dialect::Postgres);
            let body = &sql[i + 2..end.max(i + 2)];
            let executable = body.starts_with(b"!") || body.starts_with(b"M!");
            Some(Skip {
                end,
                comment: true,
                executable,
            })
        }
        b'$' if dialect == Dialect::Postgres => dollar_quote(sql, i).map(Skip::literal),
        _ => None,
    }
}

/// Whether the script holds a `/*! ... */` or `/*M! ... */` comment outside strings. Servers
/// execute their contents, so they are treated as code that cannot be inspected.
pub fn has_executable_comment(sql: &str, dialect: Dialect) -> bool {
    let bytes = sql.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match skip_at(bytes, i, dialect) {
            Some(skip) if skip.executable => return true,
            Some(skip) => i = skip.end.max(i + 1),
            None => i += 1,
        }
    }
    false
}

/// Whether the text holds anything other than whitespace and plain comments.
pub(crate) fn has_code(sql: &str, dialect: Dialect) -> bool {
    let bytes = sql.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if let Some(skip) = skip_at(bytes, i, dialect) {
            if !skip.comment || skip.executable {
                return true;
            }
            i = skip.end.max(i + 1);
        } else if bytes[i].is_ascii_whitespace() {
            i += 1;
        } else {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn finds_executable_comments_outside_strings() {
        assert!(has_executable_comment("SELECT /*! SLEEP(1) */ 1", Dialect::MySql));
        assert!(has_executable_comment("SELECT /*M! 1 */ 1", Dialect::MySql));
        assert!(has_executable_comment("SELECT /*!50000 1 */", Dialect::MySql));
        assert!(!has_executable_comment("SELECT '/*! x */'", Dialect::MySql));
        assert!(!has_executable_comment("SELECT 1 -- /*! x */", Dialect::MySql));
        assert!(!has_executable_comment("SELECT /* /*! x */", Dialect::MySql));
        assert!(!has_executable_comment("SELECT `/*! x */`", Dialect::MySql));
    }

    #[test]
    fn mysql_backslash_keeps_the_string_open() {
        assert!(has_executable_comment(r"SELECT 'a'/*! x */", Dialect::MySql));
        assert!(!has_executable_comment(r"SELECT 'a\'/*! x */'", Dialect::MySql));
    }

    #[test]
    fn postgres_backslash_only_escapes_in_e_strings() {
        assert!(has_executable_comment(r"SELECT 'a\'/*! x */", Dialect::Postgres));
        assert!(!has_executable_comment(
            r"SELECT E'a\'/*! x */'",
            Dialect::Postgres
        ));
        assert!(has_executable_comment(
            r"SELECT line'a\'/*! x */",
            Dialect::Postgres
        ));
    }

    #[test]
    fn dollar_quotes_hide_their_body() {
        assert!(!has_executable_comment(
            "SELECT $$ /*! x */ $$",
            Dialect::Postgres
        ));
        assert!(!has_executable_comment(
            "SELECT $a$ $$ /*! x */ $a$",
            Dialect::Postgres
        ));
        assert!(has_executable_comment("SELECT $1 /*! x */", Dialect::Postgres));
    }

    #[test]
    fn mysql_dash_needs_whitespace() {
        assert!(has_executable_comment("SELECT 1--1 /*! x */", Dialect::MySql));
        assert!(!has_executable_comment("SELECT 1-- 1 /*! x */", Dialect::MySql));
        assert!(!has_executable_comment("SELECT 1--1 /*! x */", Dialect::Postgres));
    }

    #[test]
    fn postgres_comments_nest_and_mysql_comments_do_not() {
        let sql = "/* /* */ /*! x */ */";
        assert!(has_executable_comment(sql, Dialect::MySql));
        assert!(!has_executable_comment(sql, Dialect::Postgres));
    }

    #[test]
    fn code_detection() {
        assert!(!has_code(
            "  -- only a comment\n /* and this */ ",
            Dialect::Postgres
        ));
        assert!(has_code("/* c */ SELECT 1", Dialect::Postgres));
        assert!(has_code("/*! x */", Dialect::MySql));
        assert!(!has_code("", Dialect::Sqlite));
    }
}
