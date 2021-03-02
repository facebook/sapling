/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use anyhow::{format_err, Result};
use async_trait::async_trait;
use futures::future::{BoxFuture, FutureExt};

use blobstore::{
    Blobstore, BlobstoreGetData, BlobstorePutOps, BlobstoreWithLink, OverwriteStatus, PutBehaviour,
    DEFAULT_PUT_BEHAVIOUR,
};
use context::CoreContext;
use mononoke_types::BlobstoreBytes;

// Implements hardlink-style links
#[derive(Default, Debug)]
struct MemState {
    next_id: usize,
    data: HashMap<usize, BlobstoreBytes>,
    links: HashMap<String, usize>,
}

impl MemState {
    fn put(
        &mut self,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> OverwriteStatus {
        match put_behaviour {
            PutBehaviour::Overwrite => {
                let id = self.next_id;
                self.data.insert(id, value);
                self.links.insert(key, id);
                self.next_id += 1;
                OverwriteStatus::NotChecked
            }
            PutBehaviour::IfAbsent | PutBehaviour::OverwriteAndLog => {
                if self.links.contains_key(&key) {
                    if put_behaviour.should_overwrite() {
                        self.put(key, value, PutBehaviour::Overwrite);
                        OverwriteStatus::Overwrote
                    } else {
                        OverwriteStatus::Prevented
                    }
                } else {
                    self.put(key, value, PutBehaviour::Overwrite);
                    OverwriteStatus::New
                }
            }
        }
    }

    fn link(&mut self, existing_key: &str, link_key: String) -> Result<()> {
        if let Some(existing_id) = self.links.get(existing_key) {
            let existing_id = *existing_id;
            self.links.insert(link_key, existing_id);
            return Ok(());
        }
        Err(format_err!("Unknown existing_key {}", existing_key))
    }

    fn get(&self, key: &str) -> Option<&BlobstoreBytes> {
        if let Some(id) = self.links.get(key) {
            self.data.get(id)
        } else {
            None
        }
    }

    fn unlink(&mut self, key: &str) -> Option<()> {
        self.links.remove(key).map(|_| ())
    }
}

/// In-memory "blob store"
///
/// Pure in-memory implementation for testing.
#[derive(Clone)]
pub struct Memblob {
    state: Arc<Mutex<MemState>>,
    put_behaviour: PutBehaviour,
}

impl std::fmt::Display for Memblob {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Memblob")
    }
}

impl Memblob {
    pub fn new(put_behaviour: PutBehaviour) -> Self {
        Self {
            state: Arc::new(Mutex::new(MemState::default())),
            put_behaviour,
        }
    }

    pub fn unlink(&self, key: String) -> BoxFuture<'static, Result<Option<()>>> {
        let state = self.state.clone();

        async move {
            let mut inner = state.lock().expect("lock poison");
            Ok(inner.unlink(&key))
        }
        .boxed()
    }
}

impl Default for Memblob {
    fn default() -> Self {
        Self::new(DEFAULT_PUT_BEHAVIOUR)
    }
}

#[async_trait]
impl BlobstorePutOps for Memblob {
    async fn put_explicit<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus> {
        let state = self.state.clone();

        let mut inner = state.lock().expect("lock poison");
        Ok(inner.put(key, value, put_behaviour))
    }

    async fn put_with_status<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<OverwriteStatus> {
        self.put_explicit(ctx, key, value, self.put_behaviour).await
    }
}

#[async_trait]
impl Blobstore for Memblob {
    async fn get<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let state = self.state.clone();

        let inner = state.lock().expect("lock poison");
        Ok(inner.get(&key).map(|bytes| bytes.clone().into()))
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
impl BlobstoreWithLink for Memblob {
    async fn link<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        existing_key: &'a str,
        link_key: String,
    ) -> Result<()> {
        let state = self.state.clone();

        let mut inner = state.lock().expect("lock poison");
        inner.link(existing_key, link_key)
    }
}

impl fmt::Debug for Memblob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Memblob")
            .field("state", &self.state.lock().expect("lock poisoned"))
            .finish()
    }
}
