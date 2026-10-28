/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Open-source builds have no remote config service: nothing is cached and
//! every fetch fails, so callers get the snapshot they pass, as fb.rs returns
//! when its fetch fails, or use their defaults. fb.rs provides the same
//! interface with the `fb` feature.

use std::path::Path;

use anyhow::Error;
use anyhow::anyhow;
use serde::de::DeserializeOwned;

pub fn read_local_config<C: DeserializeOwned>(_cache_path: &Path) -> Option<C> {
    None
}

pub fn should_fetch_configs(_limit: u64, _cache_path: &Path) -> bool {
    true
}

pub fn get_remote_configs<C: DeserializeOwned>(
    _is_pub: bool,
    _add_service_user: bool,
    _remote_url: Option<String>,
    _limit: u64,
    _http_config: http_client::Config,
    _cache_path: &Path,
    remote_config_snapshot: Option<&str>,
    _config_hash_index: &str,
    _config_up_to_date_index: &str,
    _config_index: &str,
) -> Result<C, Error> {
    match remote_config_snapshot {
        Some(snapshot) => Ok(serde_json::from_str(snapshot)?),
        None => Err(anyhow!(
            "remote config loading is unavailable in this build"
        )),
    }
}
