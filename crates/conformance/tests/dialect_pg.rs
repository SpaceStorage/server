//! G3/G4 PostgreSQL first-binary dialect.

use spacestorage_compat::DialectProfile;
use spacestorage_handler_postgresql::{FEATURE_NOT_SUPPORTED, execute_sql};
use spacestorage_types::ContainerCatalog;
use std::sync::{Arc, RwLock};

#[test]
fn g3_simple_extended_crud_autocommit() {
    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let p = DialectProfile::FirstBinary;
    execute_sql(p, &cat, "demo", "CREATE TABLE t (id text, v text)").unwrap();
    execute_sql(p, &cat, "demo", "INSERT INTO t VALUES ('1', 'a')").unwrap();
    let sel = execute_sql(p, &cat, "demo", "SELECT * FROM t").unwrap();
    assert_eq!(sel.rows.len(), 1);
    execute_sql(p, &cat, "demo", "UPDATE t SET v = 'b' WHERE id = '1'").unwrap();
    let sel = execute_sql(p, &cat, "demo", "SELECT * FROM t WHERE id = '1'").unwrap();
    assert_eq!(sel.rows[0][1].as_deref(), Some("b"));
    execute_sql(p, &cat, "demo", "DELETE FROM t WHERE id = '1'").unwrap();
    execute_sql(p, &cat, "demo", "DROP TABLE t").unwrap();
    // Extended path is Parse/Bind/Execute in the handler crate wire loop;
    // catalog SQL above covers the auto-commit IR for G3.
}

#[test]
fn g4_copy_begin_commit_rollback_are_0a000() {
    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    let p = DialectProfile::FirstBinary;
    for sql in ["BEGIN", "COMMIT", "ROLLBACK", "COPY t FROM STDIN"] {
        let e = execute_sql(p, &cat, "demo", sql).unwrap_err();
        assert_eq!(e.sqlstate, FEATURE_NOT_SUPPORTED, "{sql}");
    }
}
