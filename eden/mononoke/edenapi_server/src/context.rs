/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use mononoke_api::Mononoke;

use gotham_derive::StateData;

/// Struct containing the EdenAPI server's global shared state.
/// Intended to be exposed throughout the server by being inserted into
/// the `State` for each request via Gotham's `StateMiddleware`. As such,
/// this type is designed to be cheaply clonable, with all cloned sharing
/// the same underlying data.
#[derive(Clone, StateData)]
pub struct EdenApiServerContext {
    inner: Arc<Mutex<EdenApiServerContextInner>>,
    will_exit: Arc<AtomicBool>,
}

impl EdenApiServerContext {
    pub fn new(mononoke: Mononoke, will_exit: Arc<AtomicBool>) -> Self {
        let inner = EdenApiServerContextInner::new(mononoke);
        Self {
            inner: Arc::new(Mutex::new(inner)),
            will_exit,
        }
    }

    pub fn will_exit(&self) -> bool {
        self.will_exit.load(Ordering::Relaxed)
    }
}

/// Underlying global state for an EdenApiContext. Any data that needs to
/// be broadly available throughout the server's request handlers should
/// be placed here.
struct EdenApiServerContextInner {
    #[allow(unused)]
    mononoke: Mononoke,
}

impl EdenApiServerContextInner {
    fn new(mononoke: Mononoke) -> Self {
        Self { mononoke }
    }
}
