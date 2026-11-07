//! The AST classifier.

use crate::dialect::parser_dialect;
use crate::functions::denied_name;
use crate::lexer::has_executable_comment;
use crate::risk;
use crate::split::{split_script, too_complex};
use sqlparser::ast::{Expr, ObjectName, Query, Select, SetExpr, Statement, TableFactor, Visit, Visitor};
use sqlparser::parser::Parser;
use std::ops::ControlFlow;
use studio_types::{Dialect, Span, StatementKind, StatementVerdict};

/// How risky a statement is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Risk {
    /// Ordinary.
    Normal,
    /// Needs the stronger confirmation.
    High {
        /// Plain-language reasons.
        reasons: Vec<String>,
    },
}

/// The decision for one statement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// Category.
    pub kind: StatementKind,
    /// May run without a human on the user path.
    pub read_only: bool,
    /// May run with no human for the AI. Implies `read_only`.
    pub ai_auto: bool,
    /// Risk.
    pub risk: Risk,
    /// Why `ai_auto` is false.
    pub reason: Option<String>,
}

/// One classified statement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassifiedStatement {
    /// Where it is in the script.
    pub span: Span,
    /// The exact text.
    pub text: String,
    /// The decision.
    pub verdict: Verdict,
}

/// SQLite pragmas that only read. Any other pragma is a possible write.
const READ_PRAGMAS: &[&str] = &[
    "table_info",
    "table_xinfo",
    "table_list",
    "index_list",
    "index_info",
    "index_xinfo",
    "foreign_key_list",
    "database_list",
    "collation_list",
    "compile_options",
    "function_list",
    "module_list",
    "pragma_list",
];

fn write(kind: StatementKind, reason: &str, statement: &Statement) -> Verdict {
    let reasons = risk::assess(statement);
    let risk = if reasons.is_empty() {
        Risk::Normal
    } else {
        Risk::High { reasons }
    };
    Verdict {
        kind,
        read_only: false,
        ai_auto: false,
        risk,
        reason: Some(reason.to_owned()),
    }
}

fn read(kind: StatementKind) -> Verdict {
    Verdict {
        kind,
        read_only: true,
        ai_auto: true,
        risk: Risk::Normal,
        reason: None,
    }
}

fn unparsed() -> Verdict {
    Verdict {
        kind: StatementKind::Other,
        read_only: false,
        ai_auto: false,
        risk: Risk::High {
            reasons: vec!["This statement could not be parsed, so its effect is unknown.".to_owned()],
        },
        reason: Some("could not parse".to_owned()),
    }
}

/// What a walk over a query found.
struct Scan {
    dialect: Dialect,
    /// First data-changing statement nested in the query.
    write: Option<StatementKind>,
    select_into: bool,
    locks: bool,
    denied: Option<String>,
}

impl Scan {
    fn check_name(&mut self, name: &ObjectName) {
        if self.denied.is_none() {
            self.denied = denied_name(name, self.dialect);
        }
    }
}

impl Visitor for Scan {
    type Break = ();

    fn pre_visit_statement(&mut self, statement: &Statement) -> ControlFlow<()> {
        if !matches!(statement, Statement::Query(_)) && self.write.is_none() {
            self.write = Some(match statement {
                Statement::Insert(_) => StatementKind::Insert,
                Statement::Update(_) => StatementKind::Update,
                Statement::Delete(_) => StatementKind::Delete,
                Statement::Merge(_) => StatementKind::Merge,
                _ => StatementKind::Other,
            });
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<()> {
        self.locks |= !query.locks.is_empty();
        let nested = match query.body.as_ref() {
            SetExpr::Insert(_) => Some(StatementKind::Insert),
            SetExpr::Update(_) => Some(StatementKind::Update),
            SetExpr::Delete(_) => Some(StatementKind::Delete),
            SetExpr::Merge(_) => Some(StatementKind::Merge),
            _ => None,
        };
        self.write = self.write.or(nested);
        ControlFlow::Continue(())
    }

    fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<()> {
        self.select_into |= select.into.is_some();
        ControlFlow::Continue(())
    }

    fn pre_visit_relation(&mut self, relation: &ObjectName) -> ControlFlow<()> {
        self.check_name(relation);
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, factor: &TableFactor) -> ControlFlow<()> {
        if let TableFactor::Function { name, .. } = factor {
            self.check_name(name);
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<()> {
        if let Expr::Function(function) = expr {
            self.check_name(&function.name);
        }
        ControlFlow::Continue(())
    }
}

fn classify_query(statement: &Statement, dialect: Dialect) -> Verdict {
    let mut scan = Scan {
        dialect,
        write: None,
        select_into: false,
        locks: false,
        denied: None,
    };
    // The walk never breaks; the result carries nothing.
    let _ = statement.visit(&mut scan);

    if let Some(kind) = scan.write {
        return write(kind, "This query changes data.", statement);
    }
    if scan.select_into {
        return write(
            StatementKind::Ddl,
            "SELECT INTO creates a table or writes a file.",
            statement,
        );
    }
    let mut verdict = read(StatementKind::Select);
    if scan.locks {
        verdict.ai_auto = false;
        verdict.reason = Some("This query takes row locks.".to_owned());
    } else if let Some(name) = scan.denied {
        verdict.ai_auto = false;
        verdict.reason = Some(format!("{name} has side effects."));
    }
    verdict
}

/// `EXPLAIN (ANALYZE) ...` and `EXPLAIN (ANALYSE) ...` execute the statement too.
fn explain_executes(analyze: bool, options: Option<&[sqlparser::ast::UtilityOption]>) -> bool {
    analyze
        || options.is_some_and(|list| {
            list.iter().any(|option| {
                let name = option.name.value.to_lowercase();
                name == "analyze" || name == "analyse"
            })
        })
}

fn classify_pragma(statement: &Statement, name: &ObjectName) -> Verdict {
    let base = name
        .to_string()
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_lowercase();
    if READ_PRAGMAS.contains(&base.as_str()) {
        read(StatementKind::Show)
    } else {
        write(
            StatementKind::Other,
            "This pragma may change the database.",
            statement,
        )
    }
}

/// SQLite's everyday `PRAGMA table_info(t)` takes a bare identifier, which the parser rejects. This
/// accepts exactly an allow-listed name and one bare identifier, with nothing before or after.
fn is_read_pragma_call(text: &str, dialect: Dialect) -> bool {
    if dialect != Dialect::Sqlite {
        return false;
    }
    let lower = text.trim().to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("pragma") else {
        return false;
    };
    if !rest.starts_with(char::is_whitespace) {
        return false;
    }
    let Some((name, argument)) = rest.trim_start().split_once('(') else {
        return false;
    };
    let Some(argument) = argument.strip_suffix(')') else {
        return false;
    };
    READ_PRAGMAS.contains(&name.trim_end())
        && !argument.is_empty()
        && argument.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn classify_other(statement: &Statement) -> Verdict {
    let text = statement.to_string();
    let mut words = text.split_whitespace().map(str::to_uppercase);
    let first = words.next().unwrap_or_default();
    let second = words.next().unwrap_or_default();
    let (kind, reason) = match first.as_str() {
        "SET" if second == "ROLE" || second == "SESSION" => {
            (StatementKind::Dcl, "This changes the session's privileges.")
        }
        "CREATE" | "ALTER" | "DROP" | "TRUNCATE" | "RENAME" | "COMMENT" => {
            (StatementKind::Ddl, "This changes the database structure.")
        }
        _ => (StatementKind::Other, "This statement is not a known read."),
    };
    write(kind, reason, statement)
}

/// Classifies one parsed statement. Anything not positively recognised is a write.
pub(crate) fn verdict_for(statement: &Statement, dialect: Dialect) -> Verdict {
    match statement {
        Statement::Query(_) => classify_query(statement, dialect),
        Statement::Explain {
            analyze,
            options,
            statement: inner,
            ..
        } => {
            if explain_executes(*analyze, options.as_deref()) {
                verdict_for(inner, dialect)
            } else {
                read(StatementKind::Explain)
            }
        }
        Statement::ExplainTable { .. }
        | Statement::ShowFunctions { .. }
        | Statement::ShowVariable { .. }
        | Statement::ShowStatus { .. }
        | Statement::ShowVariables { .. }
        | Statement::ShowCreate { .. }
        | Statement::ShowColumns { .. }
        | Statement::ShowCatalogs { .. }
        | Statement::ShowDatabases { .. }
        | Statement::ShowProcessList { .. }
        | Statement::ShowSchemas { .. }
        | Statement::ShowCharset(_)
        | Statement::ShowObjects(_)
        | Statement::ShowTables { .. }
        | Statement::ShowViews { .. }
        | Statement::ShowCollation { .. } => read(StatementKind::Show),
        Statement::Pragma { name, .. } => classify_pragma(statement, name),
        Statement::Insert(_) => write(StatementKind::Insert, "INSERT adds rows.", statement),
        Statement::Update(_) => write(StatementKind::Update, "UPDATE changes rows.", statement),
        Statement::Delete(_) => write(StatementKind::Delete, "DELETE removes rows.", statement),
        Statement::Merge(_) => write(StatementKind::Merge, "MERGE changes rows.", statement),
        Statement::Grant(_) | Statement::Revoke(_) | Statement::Deny(_) => {
            write(StatementKind::Dcl, "This changes privileges.", statement)
        }
        Statement::StartTransaction { .. }
        | Statement::Commit { .. }
        | Statement::Rollback { .. }
        | Statement::Savepoint { .. }
        | Statement::ReleaseSavepoint { .. } => write(
            StatementKind::Tcl,
            "The AI may not control transactions.",
            statement,
        ),
        other => classify_other(other),
    }
}

/// Stack for parsing and walking. Reserved, not committed, so the size costs almost nothing.
const CLASSIFIER_STACK: usize = 64 * 1024 * 1024;

/// Runs `work` on its own large stack, so that a deep tree or a parser panic cannot take the process
/// down. Every AST is created and dropped inside `work`. `None` means it failed: callers fail closed.
fn isolated<T: Send>(work: impl FnOnce() -> T + Send) -> Option<T> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("sql-classifier".to_owned())
            .stack_size(CLASSIFIER_STACK)
            .spawn_scoped(scope, work)
            .ok()?
            .join()
            .ok()
    })
}

/// Splits and classifies a script. Unparseable statements are writes.
pub fn classify_script(sql: &str, dialect: Dialect) -> Vec<ClassifiedStatement> {
    isolated(|| classify_pieces(sql, dialect)).unwrap_or_else(|| {
        vec![ClassifiedStatement {
            span: Span {
                start: 0,
                end: u32::try_from(sql.len()).unwrap_or(u32::MAX),
            },
            text: sql.to_owned(),
            verdict: unparsed(),
        }]
    })
}

fn classify_pieces(sql: &str, dialect: Dialect) -> Vec<ClassifiedStatement> {
    split_script(sql, dialect)
        .into_iter()
        .map(|piece| {
            let mut verdict = match piece.statement.as_ref() {
                Some(statement) => verdict_for(statement, dialect),
                None if is_read_pragma_call(&piece.text, dialect) => read(StatementKind::Show),
                None => unparsed(),
            };
            if has_executable_comment(&piece.text, dialect) {
                verdict.read_only = false;
                verdict.ai_auto = false;
                verdict.reason = Some("Executable comments are not allowed.".to_owned());
            }
            ClassifiedStatement {
                span: piece.span,
                text: piece.text,
                verdict,
            }
        })
        .collect()
}

/// The verdicts shown to the UI.
pub fn verdicts(sql: &str, dialect: Dialect) -> Vec<StatementVerdict> {
    classify_script(sql, dialect)
        .into_iter()
        .map(|c| StatementVerdict {
            span: c.span,
            kind: c.verdict.kind,
            read_only: c.verdict.read_only,
            risk_reasons: match c.verdict.risk {
                Risk::Normal => Vec::new(),
                Risk::High { reasons } => reasons,
            },
            reason: c.verdict.reason,
        })
        .collect()
}

/// The strict AI gate: exactly one statement, and that statement is `ai_auto`.
pub fn ai_auto_allowed(sql: &str, dialect: Dialect) -> Result<(), String> {
    isolated(|| ai_gate(sql, dialect)).unwrap_or_else(|| Err("Could not parse this statement.".to_owned()))
}

fn ai_gate(sql: &str, dialect: Dialect) -> Result<(), String> {
    if has_executable_comment(sql, dialect) {
        return Err("Executable comments are not allowed.".to_owned());
    }
    if too_complex(sql, dialect) {
        return Err("Could not parse this statement.".to_owned());
    }
    let text = sql.trim().trim_end_matches(';');
    if !is_read_pragma_call(text, dialect) {
        let parsed = Parser::parse_sql(parser_dialect(dialect).as_ref(), sql)
            .map_err(|_| "Could not parse this statement.".to_owned())?;
        if parsed.len() != 1 {
            return Err("Exactly one statement is allowed.".to_owned());
        }
    }
    let classified = classify_pieces(sql, dialect);
    let [statement] = classified.as_slice() else {
        return Err("Exactly one statement is allowed.".to_owned());
    };
    if statement.verdict.ai_auto {
        Ok(())
    } else {
        Err(statement
            .verdict
            .reason
            .clone()
            .unwrap_or_else(|| "This is not a read.".to_owned()))
    }
}
