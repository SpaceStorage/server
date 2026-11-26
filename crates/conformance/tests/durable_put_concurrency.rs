//! Regression: concurrent durable puts must not deadlock the multi-thread runtime.
//!
//! The S3 `SessionState::exec` path bridges sync dispatch to the blocking pool
//! (`block_in_place` + `Handle::block_on(spawn_blocking(run))`). `run` holds the
//! catalog `std::sync::RwLock` write guard while the durable put hook waits for
//! WAL fsync.
//!
//! Pre-fix, the hook re-entered the runtime with a nested `Handle::block_on`:
//!
//! ```ignore
//! Arc::new(move |id, _mode, key, val| {
//!     tokio::task::block_in_place(|| {
//!         tokio::runtime::Handle::current().block_on(async move {
//!             eng.put_kv_durable(id, &key, &val).await.map_err(|e| e.to_string())
//!         })
//!     })
//! })
//! ```
//!
//! `CachedParkThread::block_on` (used by `Handle::block_on`) does not drive the
//! runtime timer, and the WAL group commit waits on `tokio::time::sleep`. With
//! few workers every core ended up parked (the guard still held) so readers in
//! `bucket_exists` blocked forever and the process wedged — this test used to
//! hang until killed (`timeout` exited `124`).
//!
//! The production hook ([`StorageEngine::durable_put_hook`]) now appends +
//! fsyncs synchronously on a blocking region: no nested `block_on` and no
//! runtime timer dependency. Each test arms an OS-thread watchdog so a
//! regression fails the run instead of hanging the suite.

use spacestorage_handler_s3::{S3Reply, SessionState, dispatch};
use spacestorage_storage::{StorageEngine, SyncMode, WalMetrics};
use spacestorage_types::ContainerCatalog;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

async fn real_engine(dir: &std::path::Path) -> Arc<StorageEngine> {
    let metrics = Arc::new(WalMetrics::default());
    let engine = Arc::new(StorageEngine::new(Arc::clone(&metrics)));
    engine
        .open_drives(
            Some(dir),
            &[],
            SyncMode::Fdatasync,
            Duration::from_millis(2),
            1024 * 1024,
            "n1",
        )
        .await
        .unwrap();
    engine
}

/// Catalog wired with the production sync durable hook (no create hook).
fn catalog_with_production_hook(engine: &Arc<StorageEngine>) -> Arc<RwLock<ContainerCatalog>> {
    let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
    cat.write()
        .expect("catalog")
        .set_durable_hooks(Some(engine.durable_put_hook()), None);
    cat
}

/// Kill the process if `done` is still false after `secs`, so a deadlock fails
/// the run (`exit(124)`) instead of hanging CI forever. Runs on its own OS
/// thread, independent of the possibly-wedged Tokio runtime.
fn arm_watchdog(done: Arc<AtomicBool>, secs: u64) {
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if done.load(Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        eprintln!(
            "durable_put_concurrency: watchdog fired — durable puts deadlocked the runtime"
        );
        std::process::exit(124);
    });
}

/// 16 concurrent PUTs on a 2-worker runtime, all serialized through the catalog
/// write lock while the durable hook fsyncs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_durable_put_does_not_deadlock() {
    let dir = tempfile::tempdir().unwrap();
    let engine = real_engine(dir.path()).await;
    let cat = catalog_with_production_hook(&engine);

    {
        let mut s = SessionState::demo(Arc::clone(&cat));
        assert!(matches!(
            dispatch(&mut s, "CREATEBUCKET", &["bbb"]),
            S3Reply::Ok
        ));
    }

    let done = Arc::new(AtomicBool::new(false));
    arm_watchdog(Arc::clone(&done), 60);

    let mut handles = Vec::new();
    for i in 0..16u32 {
        let cat = Arc::clone(&cat);
        handles.push(tokio::spawn(async move {
            let mut s = SessionState::demo(cat);
            let key = format!("k{i}");
            dispatch(&mut s, "PUTOBJECT", &["bbb", &key, "v"])
        }));
    }
    for h in handles {
        let r = h.await.expect("put task panicked");
        assert!(matches!(r, S3Reply::OkHeaders(_)), "{r:?}");
    }
    done.store(true, Ordering::SeqCst);
}

/// The hook can also be invoked straight from a runtime worker (the syslog /
/// ingest path calls `catalog.put` inline). `run_blocking` must move the worker
/// aside instead of parking it on fsync.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn durable_hook_from_runtime_worker_does_not_deadlock() {
    let dir = tempfile::tempdir().unwrap();
    let engine = real_engine(dir.path()).await;
    let cat = catalog_with_production_hook(&engine);

    let id = {
        let mut s = SessionState::demo(Arc::clone(&cat));
        assert!(matches!(
            dispatch(&mut s, "CREATEBUCKET", &["bbb"]),
            S3Reply::Ok
        ));
        cat.read().expect("catalog").describe("demo", "bbb").unwrap().id
    };

    let done = Arc::new(AtomicBool::new(false));
    arm_watchdog(Arc::clone(&done), 60);

    // This test body runs on a runtime worker.
    cat.write()
        .expect("catalog")
        .put(id, "worker-key", b"v".to_vec())
        .unwrap();
    assert_eq!(
        cat.read().expect("catalog").get(id, "worker-key").unwrap(),
        Some(b"v".as_slice())
    );
    done.store(true, Ordering::SeqCst);
}
