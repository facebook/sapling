/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::Result;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::BlobstorePutOps;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use context::CoreContext;
use futures::Future;
use metaconfig_types::BlobstoreId;
use mononoke_types::BlobstoreBytes;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

// inferred from the current timeout, see https://fburl.com/code/rgj8497o
const GET_REQUEST_TIMEOUT: Duration = Duration::from_secs(100);
const PUT_REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Clone, Debug)]
pub struct MultiplexTimeout {
    pub read: Duration,
    pub write: Duration,
}

impl Default for MultiplexTimeout {
    fn default() -> Self {
        Self::new(None, None)
    }
}

impl MultiplexTimeout {
    /// This allows to set either both timeouts or only one of them
    pub fn new(read: Option<Duration>, write: Option<Duration>) -> Self {
        Self {
            read: read.unwrap_or(GET_REQUEST_TIMEOUT),
            write: write.unwrap_or(PUT_REQUEST_TIMEOUT),
        }
    }
}

#[derive(Clone)]
pub(crate) struct TimedStore {
    id: BlobstoreId,
    inner: Arc<dyn BlobstorePutOps>,
    /// Timeout enforced on the read/write futures, including those running in the background
    timeout: MultiplexTimeout,
}

impl fmt::Debug for TimedStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({:?}:{:?})", self.id, self.inner.to_string())
    }
}

impl std::fmt::Display for TimedStore {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

impl TimedStore {
    pub(crate) fn new(
        id: BlobstoreId,
        inner: Arc<dyn BlobstorePutOps>,
        timeout: MultiplexTimeout,
    ) -> Self {
        Self { id, inner, timeout }
    }

    pub(crate) fn id(&self) -> &BlobstoreId {
        &self.id
    }

    pub(crate) async fn put(
        &self,
        ctx: &CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: Option<PutBehaviour>,
    ) -> Result<OverwriteStatus, (BlobstoreId, Error)> {
        let put_fut = if let Some(put_behaviour) = put_behaviour {
            self.inner.put_explicit(ctx, key, value, put_behaviour)
        } else {
            self.inner.put_with_status(ctx, key, value)
        };

        with_timeout(put_fut, self.timeout.write)
            .await
            .map_err(|er| (self.id.clone(), er))
    }

    pub(crate) async fn get(
        &self,
        ctx: &CoreContext,
        key: &str,
    ) -> Result<Option<BlobstoreGetData>, (BlobstoreId, Error)> {
        with_timeout(self.inner.get(ctx, key), self.timeout.read)
            .await
            .map_err(|er| (self.id.clone(), er))
    }

    pub(crate) async fn is_present(
        &self,
        ctx: &CoreContext,
        key: &str,
    ) -> (BlobstoreId, Result<BlobstoreIsPresent>) {
        let result = with_timeout(self.inner.is_present(ctx, key), self.timeout.read).await;
        (self.id.clone(), result)
    }
}

pub(crate) fn with_timed_stores(
    blobstores: Vec<(BlobstoreId, Arc<dyn BlobstorePutOps>)>,
    to: MultiplexTimeout,
) -> Vec<TimedStore> {
    blobstores
        .into_iter()
        .map(|(id, bs)| TimedStore::new(id, bs, to.clone()))
        .collect()
}

async fn with_timeout<T>(fut: impl Future<Output = Result<T>>, to: Duration) -> Result<T> {
    let timeout_or_result = timeout(to, fut).await;
    timeout_or_result.unwrap_or_else(|_| Err(Error::msg("blobstore operation timeout")))
}
