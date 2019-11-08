/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use cloned::cloned;
use configerator::{ConfigLoader, ConfigSource, Entity};
use failure::{format_err, Error};
use fbinit::FacebookInit;
use serde::{Deserialize, Serialize};
use slog::{debug, info, warn, Logger};
use std::default::Default;
use std::result::Result;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, RwLock,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const FETCH_TIMEOUT: u64 = 10;

/// Struct representing actual config data.
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub track_bytes_sent: bool,
    pub enable_consistent_routing: bool,
    pub max_bytes_sent_5s: Option<u64>,
    pub max_bytes_sent_15s: Option<u64>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            track_bytes_sent: false,
            enable_consistent_routing: false,
            max_bytes_sent_5s: None,
            max_bytes_sent_15s: None,
        }
    }
}

/// Struct representing a stored config and its source's idea of freshness.
#[derive(Debug, Clone)]
struct ServerConfigInner {
    mod_time: u64,
    version: Option<String>,
    config: ServerConfig,
}

impl ServerConfigInner {
    fn new(entity: Entity) -> Result<Self, Error> {
        let Entity {
            mod_time,
            version,
            contents,
        } = entity;

        let config = serde_json::from_str(&contents)?;

        Ok(Self {
            mod_time,
            version,
            config,
        })
    }

    fn maybe_update(&self, entity: Entity) -> Option<Result<Self, Error>> {
        // NOTE: We look at both mod time and version because canaries don't have a mod_time.
        if entity.mod_time == self.mod_time && entity.version == self.version {
            return None;
        }

        Some(Self::new(entity))
    }
}

/// Accessor for the config
#[derive(Debug, Clone)]
pub struct ServerConfigHandle {
    inner: Arc<RwLock<ServerConfigInner>>,
}

impl ServerConfigHandle {
    fn new(inner: ServerConfigInner) -> Self {
        Self {
            inner: Arc::new(RwLock::new(inner)),
        }
    }

    fn maybe_update(&self, entity: Entity) -> Result<Option<u64>, Error> {
        let new_inner = self.with_inner(|inner| inner.maybe_update(entity));

        match new_inner {
            None => Ok(None),
            Some(Err(err)) => Err(err),
            Some(Ok(new_inner)) => {
                let mod_time = new_inner.mod_time;

                let mut inner = self.inner.write().expect("Lock poisoned");
                *inner = new_inner;

                Ok(Some(mod_time))
            }
        }
    }

    fn with_inner<T, F>(&self, f: F) -> T
    where
        F: FnOnce(&ServerConfigInner) -> T,
    {
        let inner = self.inner.read().expect("Lock poisoned");
        f(&inner)
    }

    pub fn get(&self) -> ServerConfig {
        self.with_inner(|inner| inner.config)
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
                        let config_source = ConfigSource::file();
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

    let entity = loader.load(timeout)?;
    let config = ServerConfigHandle::new(ServerConfigInner::new(entity)?);

    let handle = thread::spawn({
        cloned!(config);
        let mut last_poll = Instant::now();

        move || loop {
            if will_exit.load(Ordering::Relaxed) {
                info!(&logger, "Shutting down configuration poller...");
                return;
            }

            // NOTE: We only sleep for 1 second here in order to exit the thread quickly if we are
            // asked to exit.
            if last_poll.elapsed() <= Duration::from_secs(fetch_interval) {
                thread::sleep(Duration::from_secs(1));
                continue;
            }

            last_poll = Instant::now();

            let outcome = loader.load(timeout).and_then(|entity| {
                debug!(&logger, "Polled LFS Configuration: {:?}", entity);
                config.maybe_update(entity)
            });

            match outcome {
                Ok(None) => {}
                Ok(Some(mod_time)) => info!(&logger, "Updated LFS configuration ({})", mod_time),
                Err(e) => warn!(&logger, "Updating LFS configuration failed: {:?}", e),
            };
        }
    });

    Ok((handle, config))
}
