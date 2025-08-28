/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;

use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use configmodel::convert::ByteCount;

use crate::IndexedLogHgIdDataStore;
use crate::IndexedLogHgIdDataStoreConfig;
use crate::StoreType;
use crate::lfs::LfsStore;
use crate::util::get_indexedlogdatastore_path;
use crate::util::get_local_path;

/// Attempt to repair the underlying indexedlog stores.
///
/// Repair should not be called while the stores are in use by other processes.
pub fn repair(
    shared_path: impl AsRef<Path>,
    local_path: Option<impl AsRef<Path>>,
    suffix: Option<impl AsRef<Path>>,
    config: &dyn Config,
) -> Result<String> {
    let mut repair_str = String::new();
    let mut shared_path = shared_path.as_ref().to_path_buf();
    if let Some(suffix) = suffix.as_ref() {
        shared_path.push(suffix);
    }
    let local_path = local_path
        .map(|p| get_local_path(p.as_ref().to_path_buf(), &suffix))
        .transpose()?;

    let max_log_count = config.get_opt::<u8>("indexedlog", "data.max-log-count")?;
    let max_bytes_per_log = config.get_opt::<ByteCount>("indexedlog", "data.max-bytes-per-log")?;
    let max_bytes = config.get_opt::<ByteCount>("remotefilelog", "cachelimit")?;
    let log_config = IndexedLogHgIdDataStoreConfig {
        max_log_count,
        max_bytes_per_log,
        max_bytes,
        btrfs_compression: false,
    };

    repair_str += &IndexedLogHgIdDataStore::repair(
        config,
        get_indexedlogdatastore_path(&shared_path)?,
        &log_config,
        StoreType::Rotated,
    )?;
    if let Some(local_path) = local_path {
        repair_str += &IndexedLogHgIdDataStore::repair(
            config,
            get_indexedlogdatastore_path(local_path)?,
            &log_config,
            StoreType::Permanent,
        )?;
    }
    repair_str += &LfsStore::repair(shared_path)?;

    Ok(repair_str)
}
