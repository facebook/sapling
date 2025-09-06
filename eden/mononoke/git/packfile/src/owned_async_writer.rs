/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use tokio::io::AsyncWrite;
use tokio::sync::mpsc::Sender;

/// In contrast to AsyncWrite, the OwnedAsyncWrite trait deals with writing owned
/// collection of bytes asynchronously
#[allow(async_fn_in_trait)]
pub trait OwnedAsyncWrite {
    async fn write_all(&mut self, src: Vec<u8>) -> anyhow::Result<()>;

    async fn flush(&mut self) -> anyhow::Result<()>;
}

impl<T: AsyncWrite + Unpin> OwnedAsyncWrite for T {
    async fn write_all(&mut self, src: Vec<u8>) -> anyhow::Result<()> {
        tokio::io::AsyncWriteExt::write_all(self, src.as_slice())
            .await
            .map_err(|e| anyhow::anyhow!("Error while writing bytes through OwnedAsyncWrite::write_all to underlying AsyncWrite: {:?}", e))
    }

    async fn flush(&mut self) -> anyhow::Result<()> {
        tokio::io::AsyncWriteExt::flush(self).await.map_err(|e| anyhow::anyhow!("Error while flushing bytes through OwnedAsyncWrite::flush to underlying AsyncWrite: {:?}", e))
    }
}

/// Wrapper sender to allow implementing OwnedAsyncWrite
pub struct WrapperSender<T> {
    inner: Sender<T>,
}

impl<T> WrapperSender<T> {
    pub fn new(inner: Sender<T>) -> Self {
        Self { inner }
    }
}

impl OwnedAsyncWrite for WrapperSender<Vec<u8>> {
    async fn write_all(&mut self, src: Vec<u8>) -> anyhow::Result<()> {
        self.inner.send(src).await.map_err(|e| {
            anyhow::anyhow!(
                "Error while writing bytes to MPSC sender through OwnedAsyncWrite::write_all: {:?}",
                e
            )
        })
    }

    async fn flush(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}
