//! Paging, sorting, filtering and counting.

use super::{run, Scratch};
use crate::Session;
use std::time::Duration;
use studio_types::{FilterOperator, FilterRule, PageRequest, SortSpec, Value};

fn text(value: &Value) -> String {
    match value {
        Value::Text(v) => v.clone(),
        other => panic!("expected text, got {other:?}"),
    }
}

pub(crate) async fn check(session: &dyn Session, scratch: &Scratch) {
    let table = scratch.qualified();
    run(
        session,
        &format!(
            "INSERT INTO {table} (id, name, score) VALUES \
             (1, 'alpha', 10), (2, 'beta', 20), (3, 'gamma', 30), (4, '50%_off', 40), (5, NULL, 50)"
        ),
    )
    .await;
    let target = scratch.table_ref();
    let sort = |column: &str, descending| {
        vec![SortSpec {
            column: column.into(),
            descending,
        }]
    };

    let page = PageRequest {
        limit: 2,
        offset: 1,
        sort: sort("score", true),
        filters: vec![],
    };
    let out = session.table_page(&target, &page).await.expect("page");
    assert_eq!(out.columns.len(), 3);
    assert_eq!(out.rows.len(), 2);
    assert_eq!(text(&out.rows[0][1]), "50%_off", "sorted descending, offset 1");

    let filters = vec![FilterRule {
        column: "score".into(),
        operator: FilterOperator::Gte,
        value: Some("30".into()),
    }];
    let page = PageRequest {
        limit: 10,
        offset: 0,
        sort: sort("id", false),
        filters: filters.clone(),
    };
    assert_eq!(
        session
            .table_page(&target, &page)
            .await
            .expect("numeric filter")
            .rows
            .len(),
        3
    );
    let count = session
        .table_count(&target, &filters, Duration::from_secs(5))
        .await
        .expect("count");
    assert_eq!(count, Some(3));

    let filters = vec![FilterRule {
        column: "name".into(),
        operator: FilterOperator::Contains,
        value: Some("%_".into()),
    }];
    let page = PageRequest {
        limit: 10,
        offset: 0,
        sort: vec![],
        filters,
    };
    let out = session
        .table_page(&target, &page)
        .await
        .expect("literal wildcard");
    assert_eq!(out.rows.len(), 1, "% and _ in a filter value must be literal");

    let filters = vec![FilterRule {
        column: "name".into(),
        operator: FilterOperator::IsNull,
        value: None,
    }];
    let count = session
        .table_count(&target, &filters, Duration::from_secs(5))
        .await
        .expect("null count");
    assert_eq!(count, Some(1));

    let attack = "x' OR '1'='1";
    let filters = vec![FilterRule {
        column: "name".into(),
        operator: FilterOperator::Eq,
        value: Some(attack.into()),
    }];
    let page = PageRequest {
        limit: 10,
        offset: 0,
        sort: vec![],
        filters,
    };
    assert!(session
        .table_page(&target, &page)
        .await
        .expect("injection")
        .rows
        .is_empty());

    let all = session
        .table_count(&target, &[], Duration::from_secs(5))
        .await
        .expect("total");
    assert_eq!(all, Some(5));
    run(session, &format!("DELETE FROM {table}")).await;
}
