/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::format_err;
use anyhow::Result;
use async_trait::async_trait;
use futures::future::BoxFuture;
use futures::future::FutureExt;

use blobstore::Blobstore;
use blobstore::BlobstoreEnumerationData;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreKeyParam;
use blobstore::BlobstoreKeySource;
use blobstore::BlobstorePutOps;
use blobstore::BlobstoreUnlinkOps;
use blobstore::OverwriteStatus;
use blobstore::PutBehaviour;
use blobstore::DEFAULT_PUT_BEHAVIOUR;
use context::CoreContext;
use mononoke_types::BlobstoreBytes;

// Implements hardlink-style links
#[derive(Default, Debug)]
struct MemState {
    next_id: usize,
    data: HashMap<usize, BlobstoreBytes>,
    links: BTreeMap<String, usize>,
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
        Ok(inner.get(key).map(|bytes| bytes.clone().into()))
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

    async fn copy<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        old_key: &'a str,
        new_key: String,
    ) -> Result<()> {
        let state = self.state.clone();

        let mut inner = state.lock().expect("lock poison");
        inner.link(old_key, new_key)
    }
}

#[async_trait]
impl BlobstoreUnlinkOps for Memblob {
    async fn unlink<'a>(&'a self, _ctx: &'a CoreContext, key: &'a str) -> Result<()> {
        let state = self.state.clone();
        let mut inner = state.lock().expect("lock poison");
        if inner.unlink(key).is_some() {
            Ok(())
        } else {
            Err(format_err!("Unknown key {} to Memblob::unlink()", key))
        }
    }
}

#[async_trait]
impl BlobstoreKeySource for Memblob {
    async fn enumerate<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        range: &'a BlobstoreKeyParam,
    ) -> Result<BlobstoreEnumerationData> {
        match range {
            BlobstoreKeyParam::Start(range) => {
                let state = self.state.lock().expect("lock poison");
                Ok(BlobstoreEnumerationData {
                    keys: state.links.range(range).map(|(k, _)| k.clone()).collect(),
                    next_token: None,
                })
            }
            BlobstoreKeyParam::Continuation(_) => {
                Err(format_err!("Continuation not supported for memblob"))
            }
        }
    }
}

impl fmt::Debug for Memblob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Memblob")
            .field("state", &self.state.lock().expect("lock poisoned"))
            .finish()
    }
}
