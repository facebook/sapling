/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::{format_err, Error};
use configerator_cached::{ConfigHandle, ConfigStore};
use fbinit::FacebookInit;
use serde::{Deserialize, Serialize};
use slog::Logger;
use std::default::Default;
use std::path::PathBuf;
use std::time::Duration;

const CONFIGERATOR_FETCH_TIMEOUT: u64 = 10;

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

pub fn get_server_config(
    fb: FacebookInit,
    logger: Logger,
    source_spec: Option<&str>,
    poll_interval: u64,
) -> Result<ConfigHandle<ServerConfig>, Error> {
    let timeout = Duration::from_secs(CONFIGERATOR_FETCH_TIMEOUT);
    let poll_interval = Duration::from_secs(poll_interval);
    match source_spec {
        Some(source_spec) => {
            // NOTE: This means we don't support file paths with ":" in them, but it also means we can
            // add other options after the first ":" later if we want.
            let mut iter = source_spec.split(":");

            // NOTE: We match None as the last element to make sure the input doesn't contain
            // disallowed trailing parts.
            match (iter.next(), iter.next(), iter.next()) {
                (Some("configerator"), Some(source), None) => Ok(Some((
                    ConfigStore::configerator(fb, logger, poll_interval, timeout)?,
                    source.to_string(),
                ))),
                (Some("file"), Some(file), None) => Ok(Some((
                    ConfigStore::file(
                        logger,
                        PathBuf::new(),
                        String::new(),
                        Duration::from_secs(1),
                    ),
                    file.to_string(),
                ))),
                (Some("default"), None, None) => Ok(None),
                _ => Err(format_err!("Invalid configuration spec: {:?}", source_spec)),
            }
        }
        None => Ok(None),
    }
    .and_then(|config| match config {
        None => Ok(ConfigHandle::default()),
        Some((source, path)) => source.get_config_handle(path),
    })
}
