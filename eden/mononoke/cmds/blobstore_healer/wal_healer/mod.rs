/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore_sync_queue::BlobstoreWal;
use chrono::Duration as ChronoDuration;
use context::CoreContext;
use futures_03_ext::BufferedParams;
use metaconfig_types::BlobstoreId;
use metaconfig_types::MultiplexId;

use crate::healer::HealResult;
use crate::healer::Healer;

pub struct WalHealer {
    #[allow(dead_code)]
    /// The amount of entries healers processes in one go.
    batch_size: usize,
    #[allow(dead_code)]
    /// Dynamic batch size, that helps to recover in case of failures.
    current_fetch_size: AtomicUsize,
    #[allow(dead_code)]
    /// The buffered params are specified in order to balance our concurrent and parallel
    /// blobs' healing according to their sizes.
    buffered_params: BufferedParams,
    #[allow(dead_code)]
    /// Write-ahead log that is used by multiplexed storage to synchronize the data.
    wal: Arc<dyn BlobstoreWal>,
    #[allow(dead_code)]
    /// The blob storages healer operates on.
    blobstores: Arc<HashMap<BlobstoreId, Arc<dyn Blobstore>>>,
    #[allow(dead_code)]
    /// Multiplex configuration id.
    multiplex_id: MultiplexId,
    #[allow(dead_code)]
    /// Optional pattern for fetching specific keys in SQL LIKE format.
    blobstore_key_like: Option<String>,
    #[allow(dead_code)]
    /// Drain the queue without healing. Use with caution.
    drain_only: bool,
}

impl WalHealer {
    pub fn new(
        batch_size: usize,
        buffered_params: BufferedParams,
        wal: Arc<dyn BlobstoreWal>,
        blobstores: Arc<HashMap<BlobstoreId, Arc<dyn Blobstore>>>,
        multiplex_id: MultiplexId,
        blobstore_key_like: Option<String>,
        drain_only: bool,
    ) -> Self {
        Self {
            batch_size,
            current_fetch_size: AtomicUsize::new(batch_size),
            buffered_params,
            wal,
            blobstores,
            multiplex_id,
            blobstore_key_like,
            drain_only,
        }
    }
}

#[async_trait]
impl Healer for WalHealer {
    async fn heal(&self, _ctx: &CoreContext, _minimum_age: ChronoDuration) -> Result<HealResult> {
        unimplemented!();
    }
}
