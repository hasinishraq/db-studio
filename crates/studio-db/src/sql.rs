//! Identifier quoting and the filter builder.

use std::collections::HashMap;
use studio_types::{Engine, FilterOperator, FilterRule, SortSpec, TableRef};

/// Quotes an identifier, doubling the quote character.
pub fn quote_ident(engine: Engine, name: &str) -> String {
    let quote = match engine {
        Engine::MySql => '`',
        Engine::Postgres | Engine::Sqlite => '"',
    };
    let mut out = String::with_capacity(name.len() + 2);
    out.push(quote);
    for c in name.chars() {
        if c == quote {
            out.push(quote);
        }
        out.push(c);
    }
    out.push(quote);
    out
}

/// A schema-qualified, quoted table name.
pub fn qualified(engine: Engine, schema: &str, table: &str) -> String {
    format!("{}.{}", quote_ident(engine, schema), quote_ident(engine, table))
}

/// Shorthand for [`qualified`] with a [`TableRef`].
pub fn qualified_ref(engine: Engine, table: &TableRef) -> String {
    qualified(engine, &table.schema, &table.name)
}

/// A rendered WHERE clause. `sql` is empty, or starts with ` WHERE`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WhereClause {
    /// SQL text with placeholders.
    pub sql: String,
    /// Values for the placeholders, in order.
    pub params: Vec<String>,
}

/// A SQL string literal for one engine. The caller refuses NUL characters first.
pub fn string_literal(engine: Engine, value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for c in value.chars() {
        match c {
            '\'' => out.push_str("''"),
            // Only MySQL treats a backslash as an escape inside a plain string.
            '\\' if engine == Engine::MySql => out.push_str("\\\\"),
            other => out.push(other),
        }
    }
    out.push('\'');
    out
}

/// Turns filter rules into a WHERE clause. Values are bound, unless [`Self::inline`] asks otherwise.
pub struct FilterBuilder<'a> {
    engine: Engine,
    inline: bool,
    /// PostgreSQL only: column name to catalog type, so a text parameter can be cast to it.
    casts: Option<&'a HashMap<String, String>>,
}

/// The LIKE escape character. `!` behaves the same on every engine and under every `sql_mode`,
/// unlike a backslash.
const LIKE_ESCAPE: char = '!';

impl<'a> FilterBuilder<'a> {
    /// A builder for one dialect.
    pub fn new(engine: Engine) -> Self {
        Self {
            engine,
            inline: false,
            casts: None,
        }
    }

    /// Supplies PostgreSQL column types. Without one, a column compares as text.
    pub fn with_casts(mut self, casts: &'a HashMap<String, String>) -> Self {
        self.casts = Some(casts);
        self
    }

    /// Writes values as escaped literals, for a single statement streamed as plain text. Only for
    /// SQL that then goes through the safety checks as a read, such as an export.
    pub fn inline(mut self) -> Self {
        self.inline = true;
        self
    }

    /// Renders the rules, combined with AND.
    pub fn build(&self, rules: &[FilterRule]) -> WhereClause {
        let mut params = Vec::new();
        let parts: Vec<String> = rules
            .iter()
            .filter_map(|rule| self.render(rule, &mut params))
            .collect();
        if parts.is_empty() {
            return WhereClause::default();
        }
        WhereClause {
            sql: format!(" WHERE {}", parts.join(" AND ")),
            params,
        }
    }

    fn render(&self, rule: &FilterRule, params: &mut Vec<String>) -> Option<String> {
        let column = quote_ident(self.engine, &rule.column);
        let op = match rule.operator {
            FilterOperator::IsNull => return Some(format!("{column} IS NULL")),
            FilterOperator::IsNotNull => return Some(format!("{column} IS NOT NULL")),
            FilterOperator::Eq => "=",
            FilterOperator::NotEq => "<>",
            FilterOperator::Gt => ">",
            FilterOperator::Gte => ">=",
            FilterOperator::Lt => "<",
            FilterOperator::Lte => "<=",
            FilterOperator::Contains | FilterOperator::StartsWith | FilterOperator::EndsWith => {
                return self.render_like(rule, &column, params)
            }
        };
        let placeholder = self.bind(rule.value.clone()?, params);
        Some(
            match (self.engine, self.casts.and_then(|c| c.get(&rule.column))) {
                (Engine::Postgres, Some(pg_type)) => format!("{column} {op} {placeholder}::text::{pg_type}"),
                (Engine::Postgres, None) => format!("{column}::text {op} {placeholder}::text"),
                _ => format!("{column} {op} {placeholder}"),
            },
        )
    }

    fn render_like(&self, rule: &FilterRule, column: &str, params: &mut Vec<String>) -> Option<String> {
        let escaped = escape_like(rule.value.as_deref()?);
        let pattern = match rule.operator {
            FilterOperator::StartsWith => format!("{escaped}%"),
            FilterOperator::EndsWith => format!("%{escaped}"),
            _ => format!("%{escaped}%"),
        };
        let placeholder = self.bind(pattern, params);
        Some(match self.engine {
            Engine::Postgres => {
                format!("{column}::text ILIKE {placeholder}::text ESCAPE '{LIKE_ESCAPE}'")
            }
            Engine::MySql => format!("CAST({column} AS CHAR) LIKE {placeholder} ESCAPE '{LIKE_ESCAPE}'"),
            Engine::Sqlite => format!("CAST({column} AS TEXT) LIKE {placeholder} ESCAPE '{LIKE_ESCAPE}'"),
        })
    }

    fn bind(&self, value: String, params: &mut Vec<String>) -> String {
        if self.inline {
            return string_literal(self.engine, &value);
        }
        params.push(value);
        self.placeholder(params.len())
    }

    fn placeholder(&self, index: usize) -> String {
        match self.engine {
            Engine::Postgres => format!("${index}"),
            Engine::MySql | Engine::Sqlite => "?".to_owned(),
        }
    }
}

fn escape_like(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if c == LIKE_ESCAPE || c == '%' || c == '_' {
            out.push(LIKE_ESCAPE);
        }
        out.push(c);
    }
    out
}

/// Renders ` ORDER BY ...`, or nothing.
pub fn order_by(engine: Engine, sort: &[SortSpec]) -> String {
    if sort.is_empty() {
        return String::new();
    }
    let keys: Vec<String> = sort
        .iter()
        .map(|s| {
            let direction = if s.descending { "DESC" } else { "ASC" };
            format!("{} {direction}", quote_ident(engine, &s.column))
        })
        .collect();
    format!(" ORDER BY {}", keys.join(", "))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use insta::assert_snapshot;

    fn rule(column: &str, operator: FilterOperator, value: Option<&str>) -> FilterRule {
        FilterRule {
            column: column.into(),
            operator,
            value: value.map(str::to_owned),
        }
    }

    #[test]
    fn quote_ident_doubles_the_quote() {
        assert_eq!(quote_ident(Engine::Postgres, r#"a"b"#), r#""a""b""#);
        assert_eq!(quote_ident(Engine::MySql, "a`b"), "`a``b`");
        assert_eq!(quote_ident(Engine::Sqlite, "plain"), "\"plain\"");
        assert_eq!(quote_ident(Engine::MySql, "a\"b"), "`a\"b`");
    }

    #[test]
    fn qualified_names() {
        assert_eq!(qualified(Engine::Postgres, "public", "t"), "\"public\".\"t\"");
        assert_eq!(qualified(Engine::MySql, "d`b", "t"), "`d``b`.`t`");
    }

    #[test]
    fn injection_in_a_filter_value_stays_a_parameter() {
        let attack = "x'; DROP TABLE users; --";
        for engine in [Engine::Postgres, Engine::MySql, Engine::Sqlite] {
            let clause = FilterBuilder::new(engine).build(&[rule("a", FilterOperator::Eq, Some(attack))]);
            assert!(!clause.sql.contains("DROP"), "{}", clause.sql);
            assert_eq!(clause.params, vec![attack.to_owned()]);
        }
    }

    #[test]
    fn injection_in_a_column_name_stays_quoted() {
        let clause = FilterBuilder::new(Engine::Postgres).build(&[rule(
            r#"a" = 1; DROP TABLE t; --"#,
            FilterOperator::IsNull,
            None,
        )]);
        assert_snapshot!(clause.sql, @r#" WHERE "a"" = 1; DROP TABLE t; --" IS NULL"#);
    }

    #[test]
    fn like_wildcards_in_values_are_literal() {
        let clause =
            FilterBuilder::new(Engine::Sqlite).build(&[rule("a", FilterOperator::Contains, Some("50%_!"))]);
        assert_eq!(clause.params, vec!["%50!%!_!!%".to_owned()]);
    }

    #[test]
    fn a_missing_value_drops_the_rule() {
        let clause = FilterBuilder::new(Engine::MySql).build(&[rule("a", FilterOperator::Eq, None)]);
        assert_eq!(clause, WhereClause::default());
    }

    #[test]
    fn postgres_where_clause() {
        let casts = HashMap::from([("age".to_owned(), "integer".to_owned())]);
        let clause = FilterBuilder::new(Engine::Postgres).with_casts(&casts).build(&[
            rule("age", FilterOperator::Gte, Some("18")),
            rule("name", FilterOperator::StartsWith, Some("A")),
            rule("note", FilterOperator::Eq, Some("x")),
            rule("gone", FilterOperator::IsNotNull, None),
        ]);
        assert_snapshot!(clause.sql, @r#" WHERE "age" >= $1::text::integer AND "name"::text ILIKE $2::text ESCAPE '!' AND "note"::text = $3::text AND "gone" IS NOT NULL"#);
        assert_eq!(clause.params, vec!["18", "A%", "x"]);
    }

    #[test]
    fn mysql_where_clause() {
        let clause = FilterBuilder::new(Engine::MySql).build(&[
            rule("age", FilterOperator::Lt, Some("18")),
            rule("name", FilterOperator::EndsWith, Some("z")),
        ]);
        assert_snapshot!(clause.sql, @" WHERE `age` < ? AND CAST(`name` AS CHAR) LIKE ? ESCAPE '!'");
    }

    #[test]
    fn sqlite_where_clause() {
        let clause = FilterBuilder::new(Engine::Sqlite).build(&[
            rule("a", FilterOperator::NotEq, Some("1")),
            rule("b", FilterOperator::Contains, Some("x")),
        ]);
        assert_snapshot!(clause.sql, @r#" WHERE "a" <> ? AND CAST("b" AS TEXT) LIKE ? ESCAPE '!'"#);
    }

    #[test]
    fn order_by_quotes_columns() {
        let sort = [
            SortSpec {
                column: "a".into(),
                descending: true,
            },
            SortSpec {
                column: "b`c".into(),
                descending: false,
            },
        ];
        assert_eq!(order_by(Engine::MySql, &sort), " ORDER BY `a` DESC, `b``c` ASC");
        assert_eq!(order_by(Engine::MySql, &[]), "");
    }
}
