//! Metadata lists the scratch objects.

use super::Scratch;
use crate::Session;
use studio_types::ObjectKind;

pub(crate) async fn check(session: &dyn Session, scratch: &Scratch) {
    let info = session.server_info().await.expect("server_info");
    assert!(!info.version.is_empty());

    let schemas = session.list_schemas().await.expect("list_schemas");
    assert!(
        schemas.iter().any(|s| s.name == scratch.schema),
        "schema missing from {schemas:?}"
    );

    let objects = session
        .list_objects(&scratch.schema_ref())
        .await
        .expect("list_objects");
    let table = objects
        .iter()
        .find(|o| o.name == scratch.table)
        .expect("table listed");
    assert_eq!(table.kind, ObjectKind::Table);

    let columns = session.columns(&scratch.schema_ref()).await.expect("columns");
    let mine: Vec<_> = columns.iter().filter(|c| c.table == scratch.table).collect();
    assert_eq!(mine.len(), 3);
    assert_eq!(mine[0].name, "id");
    assert!(mine[0].primary_key);
    assert!(!mine[1].primary_key);

    let structure = session
        .table_structure(&scratch.table_ref())
        .await
        .expect("table_structure");
    assert_eq!(structure.columns.len(), 3);
    assert_eq!(structure.kind, ObjectKind::Table);
    assert!(structure.ddl.is_some(), "structure must include DDL");
}
