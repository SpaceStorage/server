//! Per-stream bounded buffers; full → stream_backpressured (no worker park).

use thiserror::Error;
use tokio::sync::mpsc;

#[derive(Debug, Error, PartialEq, Eq)]
#[error("stream_backpressured")]
pub struct StreamBackpressured;

pub struct StreamBuffer<T> {
    tx: mpsc::Sender<T>,
    rx: mpsc::Receiver<T>,
}

impl<T> StreamBuffer<T> {
    pub fn new(capacity: usize) -> Self {
        let (tx, rx) = mpsc::channel(capacity.max(1));
        Self { tx, rx }
    }

    pub fn try_send(&self, item: T) -> Result<(), StreamBackpressured> {
        self.tx.try_send(item).map_err(|_| StreamBackpressured)
    }

    pub fn sender(&self) -> mpsc::Sender<T> {
        self.tx.clone()
    }

    pub fn into_receiver(self) -> mpsc::Receiver<T> {
        self.rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_buffer_errors_without_blocking() {
        let buf = StreamBuffer::new(1);
        buf.try_send(1u8).unwrap();
        assert_eq!(buf.try_send(2u8).unwrap_err(), StreamBackpressured);
    }
}
