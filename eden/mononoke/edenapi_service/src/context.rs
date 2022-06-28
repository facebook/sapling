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

/// Struct containing the EdenAPI server's global shared state.
/// Intended to be exposed throughout the server by being inserted into
/// the `State` for each request via Gotham's `StateMiddleware`. As such,
/// this type is designed to be cheaply clonable, with all cloned sharing
/// the same underlying data.
#[derive(Clone, StateData)]
pub struct ServerContext {
    inner: Arc<Mutex<ServerContextInner>>,
    will_exit: Arc<AtomicBool>,
}

impl ServerContext {
    pub fn new(mononoke: Mononoke, will_exit: Arc<AtomicBool>) -> Self {
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
    /// the EdenAPI server should interact with the Mononoke backend.
    pub fn mononoke_api(&self) -> Arc<Mononoke> {
        self.inner.lock().expect("lock poisoned").mononoke.clone()
    }
}

/// Underlying global state for a ServerContext. Any data that needs to
/// be broadly available throughout the server's request handlers should
/// be placed here.
struct ServerContextInner {
    mononoke: Arc<Mononoke>,
}

impl ServerContextInner {
    fn new(mononoke: Mononoke) -> Self {
        Self {
            mononoke: Arc::new(mononoke),
        }
    }
}
