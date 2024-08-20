/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;

use gotham_derive::StateData;
use mononoke_api::Mononoke;

/// Struct containing the SaplingRemoteAPI server's global shared state.
/// Intended to be exposed throughout the server by being inserted into
/// the `State` for each request via Gotham's `StateMiddleware`. As such,
/// this type is designed to be cheaply clonable, with all cloned sharing
/// the same underlying data.
#[derive(Clone, StateData)]
pub struct ServerContext<R: Send + Sync + 'static> {
    inner: Arc<Mutex<ServerContextInner<R>>>,
    will_exit: Arc<AtomicBool>,
}

impl<R: Send + Sync + 'static> ServerContext<R> {
    pub fn new(mononoke: Arc<Mononoke<R>>, will_exit: Arc<AtomicBool>) -> Self {
        let inner = ServerContextInner::new(mononoke);
        Self {
            inner: Arc::new(Mutex::new(inner)),
            will_exit,
        }
    }

    pub fn will_exit(&self) -> bool {
        self.will_exit.load(Ordering::Relaxed)
    }

    /// Get a reference to the Mononoke API. This is the main way that
    /// the SaplingRemoteAPI server should interact with the Mononoke backend.
    pub fn mononoke_api(&self) -> Arc<Mononoke<R>> {
        self.inner.lock().expect("lock poisoned").mononoke.clone()
    }
}

/// Underlying global state for a ServerContext. Any data that needs to
/// be broadly available throughout the server's request handlers should
/// be placed here.
struct ServerContextInner<R: Send + Sync + 'static> {
    mononoke: Arc<Mononoke<R>>,
}

impl<R: Send + Sync + 'static> ServerContextInner<R> {
    fn new(mononoke: Arc<Mononoke<R>>) -> Self {
        Self { mononoke }
    }
}
