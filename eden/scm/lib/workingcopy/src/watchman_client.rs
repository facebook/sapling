/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use configmodel::Config;
use once_cell::sync::OnceCell;

use crate::watchmanfs;

pub struct DeferredWatchmanClient {
    config: Arc<dyn Config>,
    watchman_client: OnceCell<Arc<watchman_client::Client>>,
}

// Defer connection attempt to watchman until necessary.
impl DeferredWatchmanClient {
    pub fn new(config: Arc<dyn Config>) -> Self {
        Self {
            config,
            watchman_client: Default::default(),
        }
    }

    pub fn get(&self) -> Result<Arc<watchman_client::Client>> {
        self.watchman_client
            .get_or_try_init(|| connect_watchman(&self.config))
            .map(|c| c.clone())
    }
}

fn connect_watchman(config: &dyn Config) -> Result<Arc<watchman_client::Client>> {
    async_runtime::block_on(watchmanfs::connect_watchman(config)).map(Arc::new)
}
