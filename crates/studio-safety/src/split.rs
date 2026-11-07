//! Splits a script into statements with byte spans.
//!
//! Semicolons are found by the dialect-exact lexer, then every piece must parse to exactly one
//! statement. A lexer and parser disagreement therefore fails closed instead of hiding text.

use crate::dialect::parser_dialect;
use crate::lexer::{has_code, skip_at};
use sqlparser::ast::Statement;
use sqlparser::parser::Parser;
use studio_types::{Dialect, Span};

/// How far a block body such as `CREATE TRIGGER ... BEGIN ...; ...; END` may be merged.
const MAX_MERGE: usize = 256;

/// One statement of a script.
#[derive(Clone, Debug)]
pub struct Piece {
    /// Where it is, in bytes.
    pub span: Span,
    /// The exact text, without the trailing semicolon.
    pub text: String,
    /// The parsed statement, or `None` when it did not parse.
    pub statement: Option<Statement>,
}

fn raw_ranges(sql: &str, dialect: Dialect) -> Vec<(usize, usize)> {
    let bytes = sql.as_bytes();
    let (mut ranges, mut start, mut i) = (Vec::new(), 0, 0);
    while i < bytes.len() {
        if let Some(skip) = skip_at(bytes, i, dialect) {
            i = skip.end.max(i + 1);
        } else if bytes[i] == b';' {
            ranges.push((start, i));
            i += 1;
            start = i;
        } else {
            i += 1;
        }
    }
    ranges.push((start, bytes.len()));
    ranges.retain(|(from, to)| has_code(&sql[*from..*to], dialect));
    ranges
}

fn trimmed(sql: &str, from: usize, to: usize) -> (usize, usize) {
    let piece = &sql[from..to];
    let start = from + (piece.len() - piece.trim_start().len());
    (start, start + piece.trim().len())
}

/// The longest comma-free run of tokens we are willing to parse. A chain such as `1 + 1 + ...`
/// parses iteratively into a tree as deep as the chain, and walking or dropping that tree recurses.
/// Commas separate siblings, so big `IN` lists and multi-row `VALUES` do not count against this.
const MAX_CHAIN_TOKENS: usize = 4_000;

/// Whether a statement could build a tree too deep to handle. Such text fails closed.
pub(crate) fn too_complex(text: &str, dialect: Dialect) -> bool {
    let bytes = text.as_bytes();
    let (mut chain, mut in_word, mut i) = (0usize, false, 0);
    while i < bytes.len() {
        if let Some(skip) = skip_at(bytes, i, dialect) {
            chain += usize::from(!skip.comment);
            in_word = false;
            i = skip.end.max(i + 1);
            continue;
        }
        let byte = bytes[i];
        i += 1;
        if byte == b',' {
            chain = 0;
            in_word = false;
        } else if byte.is_ascii_whitespace() {
            in_word = false;
        } else if byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80 {
            chain += usize::from(!in_word);
            in_word = true;
        } else {
            chain += 1;
            in_word = false;
        }
        if chain > MAX_CHAIN_TOKENS {
            return true;
        }
    }
    false
}

/// Whether text can be the start of a statement with semicolons in its body.
fn may_have_block_body(text: &str) -> bool {
    let upper = text.to_ascii_uppercase();
    upper.contains("BEGIN") || upper.contains("CASE")
}

/// Parses text that must be exactly one statement.
fn parse_one(text: &str, dialect: Dialect) -> Option<Statement> {
    if too_complex(text, dialect) {
        return None;
    }
    let mut statements = Parser::parse_sql(parser_dialect(dialect).as_ref(), text).ok()?;
    match statements.len() {
        1 => statements.pop(),
        _ => None,
    }
}

fn span(from: usize, to: usize) -> Span {
    Span {
        start: u32::try_from(from).unwrap_or(u32::MAX),
        end: u32::try_from(to).unwrap_or(u32::MAX),
    }
}

/// Splits `sql`. Comment-only pieces are dropped; pieces that do not parse are kept and marked.
pub fn split_script(sql: &str, dialect: Dialect) -> Vec<Piece> {
    let ranges = raw_ranges(sql, dialect);
    let mut pieces = Vec::with_capacity(ranges.len());
    let mut i = 0;
    while i < ranges.len() {
        let (start, end) = trimmed(sql, ranges[i].0, ranges[i].1);
        let mut found = parse_one(&sql[start..end], dialect).map(|s| (s, i, end));
        if found.is_none() && may_have_block_body(&sql[start..end]) {
            // A semicolon inside a block body splits too early: extend until it parses as one.
            for (j, range) in ranges.iter().enumerate().take(i + MAX_MERGE).skip(i + 1) {
                let (_, merged_end) = trimmed(sql, range.0, range.1);
                if let Some(statement) = parse_one(&sql[start..merged_end], dialect) {
                    found = Some((statement, j, merged_end));
                    break;
                }
            }
        }
        match found {
            Some((statement, last, end)) => {
                pieces.push(Piece {
                    span: span(start, end),
                    text: sql[start..end].to_owned(),
                    statement: Some(statement),
                });
                i = last + 1;
            }
            None => {
                pieces.push(Piece {
                    span: span(start, end),
                    text: sql[start..end].to_owned(),
                    statement: None,
                });
                i += 1;
            }
        }
    }
    pieces
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn texts(sql: &str, dialect: Dialect) -> Vec<String> {
        split_script(sql, dialect).into_iter().map(|p| p.text).collect()
    }

    #[test]
    fn splits_with_exact_spans() {
        let sql = "SELECT 1;  SELECT 2 ;\n";
        let pieces = split_script(sql, Dialect::Postgres);
        assert_eq!(pieces.len(), 2);
        assert_eq!(
            &sql[pieces[1].span.start as usize..pieces[1].span.end as usize],
            "SELECT 2"
        );
    }

    #[test]
    fn semicolons_in_literals_and_comments_do_not_split() {
        assert_eq!(texts("SELECT ';'", Dialect::Postgres).len(), 1);
        assert_eq!(texts("SELECT 1 /* ; */", Dialect::Postgres).len(), 1);
        assert_eq!(texts("SELECT $$;$$", Dialect::Postgres).len(), 1);
        assert_eq!(texts("SELECT `a;b` FROM t", Dialect::MySql).len(), 1);
        assert_eq!(texts("SELECT [a;b] FROM t", Dialect::Sqlite).len(), 1);
    }

    #[test]
    fn comment_only_pieces_are_dropped() {
        assert_eq!(texts("SELECT 1; -- done\n;;", Dialect::Postgres), ["SELECT 1"]);
        assert!(texts("-- nothing", Dialect::Postgres).is_empty());
        assert!(texts("", Dialect::Postgres).is_empty());
    }

    #[test]
    fn mysql_dash_without_space_does_not_hide_the_next_statement() {
        let pieces = texts("SELECT 1--1; DROP TABLE x", Dialect::MySql);
        assert_eq!(pieces.len(), 2, "{pieces:?}");
        assert_eq!(pieces[1], "DROP TABLE x");
    }

    #[test]
    fn mysql_backslash_quote_does_not_end_the_string() {
        assert_eq!(texts(r"SELECT 'a\'; DROP TABLE x; -- '", Dialect::MySql).len(), 1);
    }

    #[test]
    fn postgres_backslash_quote_ends_the_string() {
        let pieces = texts(r"SELECT 'a\'; DROP TABLE x; -- '", Dialect::Postgres);
        assert_eq!(pieces.len(), 2, "{pieces:?}");
        assert_eq!(pieces[1], "DROP TABLE x");
    }

    #[test]
    fn unparseable_pieces_are_kept_and_marked() {
        let pieces = split_script("SELEC 1; SELECT 2", Dialect::Postgres);
        assert_eq!(pieces.len(), 2);
        assert!(pieces[0].statement.is_none());
        assert!(pieces[1].statement.is_some());
    }

    #[test]
    fn a_trigger_body_stays_one_statement() {
        let sql = "CREATE TRIGGER t AFTER INSERT ON a BEGIN UPDATE b SET x = 1; DELETE FROM c; END; SELECT 1";
        let pieces = split_script(sql, Dialect::Sqlite);
        assert_eq!(pieces.len(), 2, "{pieces:?}");
        assert!(pieces[0].text.ends_with("END"));
    }
}
