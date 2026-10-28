//! Ingest append via shared `005` catalog put (application=spacestorage-ingest).
//! Durable wait uses existing catalog durable_put hooks (013 blocking pool / WAL fsync).

use spacestorage_ingest::{AppendAck, LogRecord, APPLICATION_INGEST};
use spacestorage_query::ensure_container;
use spacestorage_types::ContainerCatalog;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub type SharedCatalog = Arc<RwLock<ContainerCatalog>>;

/// Ensure target exists as document store (log_stream alias) and put one durable row.
pub fn append_log_record(
    catalog: &SharedCatalog,
    namespace: &str,
    container: &str,
    rec: LogRecord,
) -> AppendAck {
    if let Err(e) = ensure_container(catalog, namespace, container, "doc") {
        tracing::warn!(error=%e, application=%APPLICATION_INGEST, "ingest ensure failed");
        return AppendAck { durable: false };
    }
    let key = format!(
        "{}:{}",
        rec.kafka_offset
            .map(|o| o.to_string())
            .unwrap_or_else(|| Uuid::now_v7().to_string()),
        Uuid::now_v7()
    );
    let bytes = serde_json::to_vec(&rec.to_json()).unwrap_or_default();
    let mut cat = match catalog.write() {
        Ok(g) => g,
        Err(_) => return AppendAck { durable: false },
    };
    let id = match cat.describe(namespace, container) {
        Ok(c) => c.id,
        Err(e) => {
            tracing::warn!(error=%e, "ingest describe failed");
            return AppendAck { durable: false };
        }
    };
    match cat.put(id, &key, bytes) {
        Ok(()) => AppendAck { durable: true },
        Err(e) => {
            tracing::warn!(error=%e, application=%APPLICATION_INGEST, "ingest put failed");
            AppendAck { durable: false }
        }
    }
}

pub fn append_fn(
    catalog: SharedCatalog,
    namespace: String,
    container: String,
) -> spacestorage_ingest::kafka::AppendFn {
    Arc::new(move |rec: LogRecord| append_log_record(&catalog, &namespace, &container, rec))
}
