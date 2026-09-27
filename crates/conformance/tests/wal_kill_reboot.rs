//! G6/G7 residual: kill → reboot → re-read client content from WAL.

use spacestorage_conformance::boot_one_node;
use spacestorage_handler_redis::{RedisReply, SessionState, dispatch};
use std::sync::Arc;
use tempfile::tempdir;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn wal_kill_reboot_reread_redis_content() {
    let dir = tempdir().unwrap();
    let work = dir.path();

    {
        let lab = boot_one_node(work).await;
        // Wait until storage hooks are attached (restore_storage done at ready).
        assert!(
            lab.node.storage.read().await.is_some(),
            "storage engine must be open for WAL durability"
        );
        let mut s = SessionState::demo(Arc::clone(&lab.node.catalog));
        assert!(matches!(
            dispatch(&mut s, "AUTH", &["demo", "demo"]),
            RedisReply::Ok
        ));
        assert!(matches!(
            dispatch(&mut s, "SET", &["durable-key", "wal-payload-v1"]),
            RedisReply::Ok
        ));
        match dispatch(&mut s, "GET", &["durable-key"]) {
            RedisReply::Bulk(Some(v)) => assert_eq!(v, b"wal-payload-v1"),
            other => panic!("pre-kill GET: {other:?}"),
        }
        lab.shutdown().await;
    }

    // Reboot same data_dir — catalog+content rehydrated from definitions + WAL.
    let lab2 = boot_one_node(work).await;
    let mut s2 = SessionState::demo(Arc::clone(&lab2.node.catalog));
    assert!(matches!(
        dispatch(&mut s2, "AUTH", &["demo", "demo"]),
        RedisReply::Ok
    ));
    match dispatch(&mut s2, "GET", &["durable-key"]) {
        RedisReply::Bulk(Some(v)) => assert_eq!(
            v, b"wal-payload-v1",
            "G6/G7 residual: client content must survive kill→reboot via WAL"
        ),
        other => panic!("post-reboot GET: {other:?}"),
    }
    lab2.shutdown().await;
}
