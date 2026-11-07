//! Properties that must hold for any input.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use proptest::prelude::*;
use studio_safety::{ai_auto_allowed, classify_script, verdicts};
use studio_types::Dialect;

const DIALECTS: [Dialect; 3] = [Dialect::Postgres, Dialect::MySql, Dialect::Sqlite];

/// Writes that must never be `ai_auto`, used to build hostile scripts.
const WRITES: &[&str] = &[
    "DROP TABLE x",
    "DELETE FROM t",
    "UPDATE t SET a = 1",
    "INSERT INTO t VALUES (1)",
];

/// Text biased towards the characters that drive the lexer.
fn sqlish() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            Just("'".to_owned()),
            Just("\"".to_owned()),
            Just("`".to_owned()),
            Just("\\".to_owned()),
            Just("--".to_owned()),
            Just("#".to_owned()),
            Just("/*".to_owned()),
            Just("*/".to_owned()),
            Just("$$".to_owned()),
            Just(";".to_owned()),
            Just("\n".to_owned()),
            Just(" ".to_owned()),
            Just("SELECT 1".to_owned()),
            Just("é".to_owned()),
            "[a-z]{1,6}",
        ],
        0..24,
    )
    .prop_map(|parts| parts.concat())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn arbitrary_strings_never_panic(sql in any::<String>()) {
        for dialect in DIALECTS {
            let _ = classify_script(&sql, dialect);
            let _ = verdicts(&sql, dialect);
            let _ = ai_auto_allowed(&sql, dialect);
        }
    }

    #[test]
    fn sql_like_strings_never_panic(sql in sqlish()) {
        for dialect in DIALECTS {
            let _ = classify_script(&sql, dialect);
            let _ = ai_auto_allowed(&sql, dialect);
        }
    }

    #[test]
    fn spans_are_valid_and_in_order(sql in sqlish()) {
        for dialect in DIALECTS {
            let mut last = 0;
            for statement in classify_script(&sql, dialect) {
                let (start, end) = (statement.span.start as usize, statement.span.end as usize);
                prop_assert!(start >= last && end >= start && end <= sql.len());
                prop_assert_eq!(&sql[start..end], statement.text.as_str());
                last = end;
            }
        }
    }

    #[test]
    fn an_executable_comment_is_never_ai_auto(prefix in "[a-z0-9 ,]{0,20}", body in "[a-z0-9 ,]{0,20}") {
        for dialect in DIALECTS {
            for marker in ["/*!", "/*!50000", "/*M!"] {
                let sql = format!("SELECT {prefix} {marker} {body} */ 1");
                prop_assert!(ai_auto_allowed(&sql, dialect).is_err(), "{}", sql);
                prop_assert!(classify_script(&sql, dialect).iter().all(|c| !c.verdict.ai_auto), "{}", sql);
            }
        }
    }

    #[test]
    fn appending_a_write_never_passes_the_gate(sql in sqlish(), index in 0..WRITES.len()) {
        for dialect in DIALECTS {
            // Closing any open construct first, so the write really is a separate statement.
            let script = format!("SELECT 1; {}; {sql}", WRITES[index]);
            prop_assert!(ai_auto_allowed(&script, dialect).is_err(), "{}", script);
            prop_assert!(classify_script(&script, dialect).iter().any(|c| !c.verdict.read_only), "{}", script);
        }
    }

    #[test]
    fn ai_auto_implies_read_only(sql in sqlish()) {
        for dialect in DIALECTS {
            for statement in classify_script(&sql, dialect) {
                prop_assert!(!statement.verdict.ai_auto || statement.verdict.read_only);
            }
        }
    }
}
