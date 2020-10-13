/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use anyhow::{format_err, Error};
use futures::future::{self, lazy, BoxFuture, FutureExt, TryFutureExt};

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

    fn link(&mut self, existing_key: String, link_key: String) -> Result<(), Error> {
        if let Some(existing_id) = self.links.get(&existing_key) {
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
pub struct EagerMemblob {
    state: Arc<Mutex<MemState>>,
    put_behaviour: PutBehaviour,
}

/// As EagerMemblob, but methods are lazy - they wait until polled to do anything.
#[derive(Clone)]
pub struct LazyMemblob {
    state: Arc<Mutex<MemState>>,
    put_behaviour: PutBehaviour,
}

impl EagerMemblob {
    pub fn new(put_behaviour: PutBehaviour) -> Self {
        Self {
            state: Arc::new(Mutex::new(MemState::default())),
            put_behaviour,
        }
    }

    pub fn unlink(&self, key: String) -> BoxFuture<'static, Result<Option<()>, Error>> {
        let mut inner = self.state.lock().expect("lock poison");
        future::ok(inner.unlink(&key)).boxed()
    }
}

impl Default for EagerMemblob {
    fn default() -> Self {
        Self::new(DEFAULT_PUT_BEHAVIOUR)
    }
}

impl LazyMemblob {
    pub fn new(put_behaviour: PutBehaviour) -> Self {
        Self {
            state: Arc::new(Mutex::new(MemState::default())),
            put_behaviour,
        }
    }

    pub fn unlink(&self, key: String) -> BoxFuture<'static, Result<Option<()>, Error>> {
        let state = self.state.clone();

        lazy(move |_| {
            let mut inner = state.lock().expect("lock poison");
            Ok(inner.unlink(&key))
        })
        .boxed()
    }
}

impl Default for LazyMemblob {
    fn default() -> Self {
        Self::new(DEFAULT_PUT_BEHAVIOUR)
    }
}

impl BlobstorePutOps for EagerMemblob {
    fn put_explicit(
        &self,
        _ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> BoxFuture<'static, Result<OverwriteStatus, Error>> {
        let mut inner = self.state.lock().expect("lock poison");

        future::ok(inner.put(key, value, put_behaviour)).boxed()
    }

    fn put_with_status(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<OverwriteStatus, Error>> {
        self.put_explicit(ctx, key, value, self.put_behaviour)
    }
}

impl Blobstore for EagerMemblob {
    fn get(
        &self,
        _ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
        let inner = self.state.lock().expect("lock poison");
        future::ok(inner.get(&key).map(|blob_ref| blob_ref.clone().into())).boxed()
    }

    fn put(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<(), Error>> {
        BlobstorePutOps::put_with_status(self, ctx, key, value)
            .map_ok(|_| ())
            .boxed()
    }
}

impl BlobstoreWithLink for EagerMemblob {
    fn link(
        &self,
        _ctx: CoreContext,
        existing_key: String,
        link_key: String,
    ) -> BoxFuture<'static, Result<(), Error>> {
        let mut inner = self.state.lock().expect("lock poison");
        future::ready(inner.link(existing_key, link_key)).boxed()
    }
}

impl BlobstorePutOps for LazyMemblob {
    fn put_explicit(
        &self,
        _ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> BoxFuture<'static, Result<OverwriteStatus, Error>> {
        let state = self.state.clone();

        lazy(move |_| {
            let mut inner = state.lock().expect("lock poison");
            Ok(inner.put(key, value, put_behaviour))
        })
        .boxed()
    }

    fn put_with_status(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<OverwriteStatus, Error>> {
        self.put_explicit(ctx, key, value, self.put_behaviour)
    }
}

impl Blobstore for LazyMemblob {
    fn get(
        &self,
        _ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
        let state = self.state.clone();

        lazy(move |_| {
            let inner = state.lock().expect("lock poison");
            Ok(inner.get(&key).map(|bytes| bytes.clone().into()))
        })
        .boxed()
    }

    fn put(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<(), Error>> {
        BlobstorePutOps::put_with_status(self, ctx, key, value)
            .map_ok(|_| ())
            .boxed()
    }
}

impl BlobstoreWithLink for LazyMemblob {
    fn link(
        &self,
        _ctx: CoreContext,
        existing_key: String,
        link_key: String,
    ) -> BoxFuture<'static, Result<(), Error>> {
        let state = self.state.clone();

        lazy(move |_| {
            let mut inner = state.lock().expect("lock poison");
            inner.link(existing_key, link_key)
        })
        .boxed()
    }
}

impl fmt::Debug for EagerMemblob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EagerMemblob")
            .field("state", &self.state.lock().expect("lock poisoned"))
            .finish()
    }
}

impl fmt::Debug for LazyMemblob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LazyMemblob")
            .field("state", &self.state.lock().expect("lock poisoned"))
            .finish()
    }
}
