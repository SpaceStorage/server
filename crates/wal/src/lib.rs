//! Durable WAL + crash restore (013 MVP / constitution XII).

use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::fs;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub struct Wal {
    path: PathBuf,
}

impl Wal {
    pub async fn open(dir: &Path) -> Result<Self, Error> {
        fs::create_dir_all(dir).await?;
        let path = dir.join("wal.log");
        if !path.exists() {
            fs::File::create(&path).await?;
        }
        Ok(Self { path })
    }

    pub async fn append_durable(&self, record: &[u8]) -> Result<(), Error> {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;
        let len = (record.len() as u32).to_be_bytes();
        f.write_all(&len).await?;
        f.write_all(record).await?;
        f.sync_all().await?;
        Ok(())
    }

    pub async fn replay(&self) -> Result<Vec<Vec<u8>>, Error> {
        let data = fs::read(&self.path).await?;
        let mut out = Vec::new();
        let mut i = 0;
        while i + 4 <= data.len() {
            let len = u32::from_be_bytes(data[i..i + 4].try_into().unwrap()) as usize;
            i += 4;
            if i + len > data.len() {
                break;
            }
            out.push(data[i..i + len].to_vec());
            i += len;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn durable_append_and_replay() {
        let dir = tempdir().unwrap();
        let wal = Wal::open(dir.path()).await.unwrap();
        wal.append_durable(b"hello").await.unwrap();
        wal.append_durable(b"world").await.unwrap();
        let recs = wal.replay().await.unwrap();
        assert_eq!(recs, vec![b"hello".to_vec(), b"world".to_vec()]);
    }
}
