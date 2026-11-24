//! Namespace-scoped multipart registry (019 US1).

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Part metadata kept in the registry. The body lives in durable chunk pieces.
#[derive(Debug, Clone)]
pub struct PartRecord {
    pub number: u32,
    pub etag: String,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct MultipartUpload {
    pub bucket: String,
    pub key: String,
    pub namespace: String,
    pub parts: Vec<PartRecord>,
    pub last_activity: Instant,
}

#[derive(Debug)]
pub struct MultipartRegistry {
    uploads: HashMap<String, MultipartUpload>,
    idle: Duration,
    min_part: u64,
    max_parts: u32,
}

impl Default for MultipartRegistry {
    fn default() -> Self {
        Self::new(Duration::from_secs(24 * 3600), 5 * 1024 * 1024, 10_000)
    }
}

impl MultipartRegistry {
    pub fn new(idle: Duration, min_part: u64, max_parts: u32) -> Self {
        Self {
            uploads: HashMap::new(),
            idle,
            min_part,
            max_parts,
        }
    }

    pub fn create(&mut self, upload_id: String, bucket: String, key: String, namespace: String) {
        self.uploads.insert(
            upload_id,
            MultipartUpload {
                bucket,
                key,
                namespace,
                parts: Vec::new(),
                last_activity: Instant::now(),
            },
        );
    }

    pub fn get_mut(&mut self, upload_id: &str) -> Option<&mut MultipartUpload> {
        self.get_for(upload_id, "")
    }

    /// Namespace-pinned lookup. A foreign namespace does not observe the upload (T052).
    pub fn get_for<'a>(
        &'a mut self,
        upload_id: &str,
        namespace: &str,
    ) -> Option<&'a mut MultipartUpload> {
        let upload = self.uploads.get_mut(upload_id)?;
        if !namespace.is_empty() && upload.namespace != namespace {
            return None;
        }
        upload.last_activity = Instant::now();
        Some(upload)
    }

    pub fn remove(&mut self, upload_id: &str) -> Option<MultipartUpload> {
        self.remove_for(upload_id, "")
    }

    pub fn remove_for(&mut self, upload_id: &str, namespace: &str) -> Option<MultipartUpload> {
        match self.uploads.get(upload_id) {
            Some(u) if namespace.is_empty() || u.namespace == namespace => {
                self.uploads.remove(upload_id)
            }
            _ => None,
        }
    }

    pub fn min_part_bytes(&self) -> u64 {
        self.min_part
    }

    pub fn max_parts(&self) -> u32 {
        self.max_parts
    }

    /// Drop in-memory upload state so the next lookup reloads the catalog copy.
    pub fn forget_memory(&mut self) {
        self.uploads.clear();
    }

    pub fn set_idle(&mut self, idle: Duration) {
        self.idle = idle;
    }

    pub fn idle(&self) -> Duration {
        self.idle
    }

    /// Test hook: move `last_activity` backward. Activity inside `idle` must survive `expire_idle`.
    pub fn backdate_for_test(&mut self, upload_id: &str, age: Duration) {
        if let Some(upload) = self.uploads.get_mut(upload_id) {
            upload.last_activity = Instant::now().checked_sub(age).unwrap_or_else(Instant::now);
        }
    }

    pub fn expire_idle(&mut self) {
        let idle = self.idle;
        self.uploads.retain(|_, u| u.last_activity.elapsed() < idle);
    }

    pub fn list_for_bucket(&self, bucket: &str, namespace: &str) -> Vec<(String, String, String)> {
        self.uploads
            .iter()
            .filter(|(_, u)| {
                u.bucket == bucket && (namespace.is_empty() || u.namespace == namespace)
            })
            .map(|(id, u)| (u.key.clone(), id.clone(), String::new()))
            .collect()
    }

    pub fn contains(&self, upload_id: &str) -> bool {
        self.uploads.contains_key(upload_id)
    }

    pub fn has_parts(&self, upload_id: &str) -> bool {
        self.uploads
            .get(upload_id)
            .is_some_and(|u| !u.parts.is_empty())
    }

    pub fn same_namespace(&self, upload_id: &str, namespace: &str) -> bool {
        self.uploads
            .get(upload_id)
            .is_some_and(|u| u.namespace == namespace)
    }

    /// Durable meta record (part bytes live under separate object keys).
    pub fn encode_meta(upload_id: &str, upload: &MultipartUpload) -> Vec<u8> {
        let mut lines = format!(
            "{}\n{}\n{}\n{}\n",
            upload_id, upload.namespace, upload.bucket, upload.key
        );
        for p in &upload.parts {
            lines.push_str(&format!("{}\t{}\t{}\n", p.number, p.etag, p.size));
        }
        lines.into_bytes()
    }

    /// Header fields from a durable meta record (`upload_id`, `namespace`, `bucket`, `key`).
    pub fn decode_meta_fields(bytes: &[u8]) -> Option<(String, String, String, String)> {
        let text = std::str::from_utf8(bytes).ok()?;
        let mut lines = text.lines();
        let upload_id = lines.next()?.to_string();
        let namespace = lines.next()?.to_string();
        let bucket = lines.next()?.to_string();
        let key = lines.next()?.to_string();
        Some((upload_id, namespace, bucket, key))
    }

    pub fn decode_meta(bytes: &[u8]) -> Option<(String, MultipartUpload)> {
        let text = std::str::from_utf8(bytes).ok()?;
        let mut lines = text.lines();
        let upload_id = lines.next()?.to_string();
        let namespace = lines.next()?.to_string();
        let bucket = lines.next()?.to_string();
        let key = lines.next()?.to_string();
        let mut parts = Vec::new();
        for line in lines {
            if line.is_empty() {
                continue;
            }
            let mut cols = line.split('\t');
            let number = cols.next()?.parse().ok()?;
            let etag = cols.next()?.to_string();
            let size: u64 = cols.next()?.parse().ok()?;
            parts.push(PartRecord { number, etag, size });
        }
        Some((
            upload_id,
            MultipartUpload {
                bucket,
                key,
                namespace,
                parts,
                last_activity: Instant::now(),
            },
        ))
    }

    pub fn insert_loaded(&mut self, upload_id: String, upload: MultipartUpload) {
        self.uploads.insert(upload_id, upload);
    }

    pub fn uploads_snapshot(&self, upload_id: &str) -> Option<MultipartUpload> {
        self.uploads.get(upload_id).cloned()
    }

    pub fn upload_id_for(&self, bucket: &str, key: &str) -> Option<String> {
        self.uploads
            .iter()
            .find(|(_, u)| u.bucket == bucket && u.key == key)
            .map(|(id, _)| id.clone())
    }
}

#[cfg(test)]
mod idle_tests {
    use super::*;

    #[test]
    fn slow_client_inside_idle_is_not_aborted() {
        let mut reg = MultipartRegistry::new(Duration::from_secs(3600), 1, 10);
        reg.create("u".into(), "b".into(), "k".into(), "ns".into());
        reg.backdate_for_test("u", Duration::from_secs(60));
        reg.expire_idle();
        assert!(reg.contains("u"), "within idle must stay");
        reg.backdate_for_test("u", Duration::from_secs(3601));
        reg.expire_idle();
        assert!(!reg.contains("u"), "past idle must drop");
    }

    #[test]
    fn meta_records_size_without_embedding_the_body() {
        let upload = MultipartUpload {
            bucket: "b".into(),
            key: "k".into(),
            namespace: "ns".into(),
            parts: vec![PartRecord {
                number: 1,
                etag: "\"abc\"".into(),
                size: 50_000_000,
            }],
            last_activity: Instant::now(),
        };
        let encoded = MultipartRegistry::encode_meta("mpu-1", &upload);
        assert!(
            encoded.len() < 256,
            "registry meta must not embed the part body, len {}",
            encoded.len()
        );
        let (id, loaded) = MultipartRegistry::decode_meta(&encoded).unwrap();
        assert_eq!(id, "mpu-1");
        assert_eq!(loaded.parts.len(), 1);
        assert_eq!(loaded.parts[0].size, 50_000_000);
        assert_eq!(loaded.parts[0].etag, "\"abc\"");
        assert!(std::mem::size_of_val(&loaded.parts[0]) < 128);
    }
}
