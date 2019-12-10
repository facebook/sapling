/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::{format_err, Error};
use cloned::cloned;
use configerator::{ConfigLoader, ConfigSource};
use configerator_cached::CachedConfigHandler;
use fbinit::FacebookInit;
use serde::{Deserialize, Serialize};
use slog::{info, warn, Logger};
use std::default::Default;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const FETCH_TIMEOUT: u64 = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Limit {
    pub counter: String,
    pub limit: i64,
    pub sleep_ms: i64,
}

/// Struct representing actual config data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub track_bytes_sent: bool,
    pub enable_consistent_routing: bool,
    pub throttle_limits: Vec<Limit>,
    pub acl_check: bool,
    pub enforce_acl_check: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            track_bytes_sent: false,
            enable_consistent_routing: false,
            throttle_limits: vec![],
            acl_check: false,
            enforce_acl_check: false,
        }
    }
}

/// Accessor for the config
#[derive(Clone)]
pub struct ServerConfigHandle {
    inner: Arc<CachedConfigHandler<ServerConfig>>,
}

impl ServerConfigHandle {
    fn new(inner: CachedConfigHandler<ServerConfig>) -> Self {
        Self {
            inner: Arc::new(inner),
        }
    }

    fn maybe_refresh(&self, timeout: Duration) -> Result<bool, Error> {
        self.inner.maybe_refresh(timeout)
    }

    pub fn get(&self) -> Arc<ServerConfig> {
        // We rely on the loop in `spawn_config_poller` updating us, so we should never be badly stale
        self.inner.get_maybe_stale()
    }
}

pub fn spawn_config_poller(
    fb: FacebookInit,
    logger: Logger,
    will_exit: Arc<AtomicBool>,
    source_spec: Option<&str>,
    fetch_interval: u64,
) -> Result<(JoinHandle<()>, ServerConfigHandle), Error> {
    let timeout = Duration::from_secs(FETCH_TIMEOUT);

    let loader = {
        match source_spec {
            Some(source_spec) => {
                // NOTE: This means we don't support file paths with ":" in them, but it also means we can
                // add other options after the first ":" later if we want.
                let mut iter = source_spec.split(":");

                // NOTE: We match None as the last element to make sure the input doesn't contain
                // disallowed trailing parts.
                match (iter.next(), iter.next(), iter.next()) {
                    (Some("configerator"), Some(source), None) => {
                        let config_source = ConfigSource::configerator(fb)?;
                        ConfigLoader::new(config_source, source.to_string())?
                    }
                    (Some("file"), Some(file), None) => {
                        let config_source = ConfigSource::file(PathBuf::new(), String::new());
                        ConfigLoader::new(config_source, file.to_string())?
                    }
                    (Some("default"), None, None) => ConfigLoader::default_content(
                        serde_json::to_string(&ServerConfig::default())?,
                    ),
                    _ => return Err(format_err!("Invalid configuration spec: {:?}", source_spec)),
                }
            }
            None => ConfigLoader::default_content(serde_json::to_string(&ServerConfig::default())?),
        }
    };

    info!(
        &logger,
        "Loading initial LFS configuration through {:?} with timeout {:?}", loader, timeout,
    );

    let config = ServerConfigHandle::new(CachedConfigHandler::new(
        loader,
        Duration::from_secs(fetch_interval),
        timeout,
    )?);

    let handle = thread::spawn({
        cloned!(config);
        move || loop {
            if will_exit.load(Ordering::Relaxed) {
                info!(&logger, "Shutting down configuration poller...");
                return;
            }

            match config.maybe_refresh(timeout) {
                Ok(false) => {}
                Ok(true) => info!(&logger, "Updated LFS configuration"),
                Err(e) => warn!(&logger, "Updating LFS configuration failed: {:?}", e),
            }
            // NOTE: We only sleep for 1 second here in order to exit the thread quickly if we are
            // asked to exit.
            thread::sleep(Duration::from_secs(1));
        }
    });

    Ok((handle, config))
}
