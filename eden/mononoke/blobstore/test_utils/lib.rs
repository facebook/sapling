/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{bail, Result};
use async_trait::async_trait;
use blobstore::{
    Blobstore, BlobstoreGetData, BlobstoreMetadata, BlobstorePutOps, OverwriteStatus, PutBehaviour,
};
use context::CoreContext;
use futures::channel::oneshot;
use lock_ext::LockExt;
use mononoke_types::BlobstoreBytes;
use std::{
    collections::{HashMap, VecDeque},
    fmt,
    future::Future,
    sync::{Arc, Mutex},
    time::SystemTime,
};

pub struct Tickable<T> {
    pub storage: Arc<Mutex<HashMap<String, T>>>,
    // queue of pending operations
    queue: Arc<Mutex<VecDeque<oneshot::Sender<Option<String>>>>>,
}

impl<T: fmt::Debug> fmt::Debug for Tickable<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tickable")
            .field("storage", &self.storage)
            .field("pending", &self.queue.with(|q| q.len()))
            .finish()
    }
}

impl<T> fmt::Display for Tickable<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Tickable")
    }
}

impl<T> Tickable<T> {
    pub fn new() -> Self {
        Self {
            storage: Default::default(),
            queue: Default::default(),
        }
    }

    // Broadcast either success or error to a set of outstanding futures, advancing the
    // overall state by one tick.
    pub fn tick(&self, error: Option<&str>) {
        let mut queue = self.queue.lock().unwrap();
        for send in queue.drain(..) {
            send.send(error.map(String::from)).unwrap();
        }
    }

    // Register this task on the tick queue and wait for it to progress.
    pub fn on_tick(&self) -> impl Future<Output = Result<()>> {
        let (send, recv) = oneshot::channel();
        let mut queue = self.queue.lock().unwrap();
        queue.push_back(send);
        async move {
            let error = recv.await?;
            match error {
                None => Ok(()),
                Some(error) => bail!(error),
            }
        }
    }
}

impl Tickable<(BlobstoreBytes, u64)> {
    pub fn get_bytes(&self, key: &str) -> Option<BlobstoreBytes> {
        self.storage
            .with(|s| s.get(key).map(|(v, _ctime)| v).cloned())
    }

    pub fn add_bytes(&self, key: String, value: BlobstoreBytes) {
        let ctime = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.storage.with(|s| {
            s.insert(key, (value, ctime));
        })
    }
}

#[async_trait]
impl Blobstore for Tickable<(BlobstoreBytes, u64)> {
    async fn get<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let storage = self.storage.clone();
        let on_tick = self.on_tick();

        on_tick.await?;
        Ok(storage.with(|s| {
            s.get(key).cloned().map(|(v, ctime)| {
                BlobstoreGetData::new(BlobstoreMetadata::new(Some(ctime as i64), None), v)
            })
        }))
    }

    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        BlobstorePutOps::put_with_status(self, ctx, key, value).await?;
        Ok(())
    }
}

#[async_trait]
impl BlobstorePutOps for Tickable<(BlobstoreBytes, u64)> {
    async fn put_explicit<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        self.on_tick().await?;
        if put_behaviour == PutBehaviour::IfAbsent {
            if self.storage.with(|s| s.contains_key(&key)) {
                return Ok(OverwriteStatus::Prevented);
            }
        }
        self.add_bytes(key, value);
        Ok(OverwriteStatus::NotChecked)
    }

    async fn put_with_status<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        self.put_explicit(ctx, key, value, PutBehaviour::Overwrite)
            .await
    }
}
