/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use once_cell::sync::OnceCell;

use crate::watchmanfs;

pub struct DeferredWatchmanClient {
    watchman_client: OnceCell<Arc<watchman_client::Client>>,
}

// Defer connection attempt to watchman until necessary.
impl DeferredWatchmanClient {
    pub fn new() -> Self {
        Self {
            watchman_client: Default::default(),
        }
    }

    pub fn get(&self) -> Result<Arc<watchman_client::Client>> {
        self.watchman_client
            .get_or_try_init(connect_watchman)
            .map(|c| c.clone())
    }
}

fn connect_watchman() -> Result<Arc<watchman_client::Client>> {
    async_runtime::block_on(watchmanfs::connect_watchman()).map(Arc::new)
}
