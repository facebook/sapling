/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

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
pub struct EdenApiContext {
    inner: Arc<Mutex<EdenApiContextInner>>,
    will_exit: Arc<AtomicBool>,
}

impl EdenApiContext {
    pub fn new(mononoke: Mononoke, will_exit: Arc<AtomicBool>) -> Self {
        let inner = EdenApiContextInner::new(mononoke);
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
struct EdenApiContextInner {
    #[allow(unused)]
    mononoke: Mononoke,
}

impl EdenApiContextInner {
    fn new(mononoke: Mononoke) -> Self {
        Self { mononoke }
    }
}
