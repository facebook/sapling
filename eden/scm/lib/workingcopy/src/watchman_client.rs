/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use configmodel::Config;
use once_cell::sync::OnceCell;

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
    async_runtime::block_on(connect_watchman_async(config)).map(Arc::new)
}

pub(crate) async fn connect_watchman_async(config: &dyn Config) -> Result<watchman_client::Client> {
    let sockpath: Option<OsString> = std::env::var_os("WATCHMAN_SOCK").or_else(|| {
        config
            .get_nonempty("fsmonitor", "sockpath")
            .map(|p| p.replace("%i", &whoami::username()).into())
    });

    let mut connector = watchman_client::Connector::new();

    if let Some(sockpath) = sockpath {
        let sockpath: &Path = sockpath.as_ref();
        if sockpath.exists() {
            tracing::debug!(?sockpath);
            connector = connector.unix_domain_socket(sockpath);
        }
    }

    Ok(connector.connect().await?)
}
