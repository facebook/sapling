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
use futures::future::{self, lazy, BoxFuture, FutureExt};

use blobstore::{Blobstore, BlobstoreGetData, BlobstoreWithLink};
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
    fn put(&mut self, key: String, value: BlobstoreBytes) {
        let id = self.next_id;
        self.data.insert(id, value);
        self.links.insert(key, id);
        self.next_id += 1;
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
}

/// As EagerMemblob, but methods are lazy - they wait until polled to do anything.
#[derive(Clone)]
pub struct LazyMemblob {
    state: Arc<Mutex<MemState>>,
}

impl EagerMemblob {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(MemState::default())),
        }
    }

    pub fn unlink(&self, key: String) -> BoxFuture<'static, Result<Option<()>, Error>> {
        let mut inner = self.state.lock().expect("lock poison");
        future::ok(inner.unlink(&key)).boxed()
    }
}

impl LazyMemblob {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(MemState::default())),
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

impl Blobstore for EagerMemblob {
    fn put(
        &self,
        _ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<(), Error>> {
        let mut inner = self.state.lock().expect("lock poison");

        inner.put(key, value);
        future::ok(()).boxed()
    }

    fn get(
        &self,
        _ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>> {
        let inner = self.state.lock().expect("lock poison");

        future::ok(inner.get(&key).map(|blob_ref| blob_ref.clone().into())).boxed()
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

impl Blobstore for LazyMemblob {
    fn put(
        &self,
        _ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<(), Error>> {
        let state = self.state.clone();

        lazy(move |_| {
            let mut inner = state.lock().expect("lock poison");

            inner.put(key, value);
            Ok(())
        })
        .boxed()
    }

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
