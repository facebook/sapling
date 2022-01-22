/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! EdenFS utils

use anyhow::anyhow;
use edenfs_error::{EdenFsError, Result, ResultExt};
use std::ffi::OsString;
use std::path::PathBuf;

pub mod humantime;
pub mod metadata;

pub fn path_from_bytes(bytes: &[u8]) -> Result<PathBuf> {
    Ok(PathBuf::from(std::str::from_utf8(bytes).from_err()?))
}

pub fn bytes_from_path(path: PathBuf) -> Result<Vec<u8>> {
    Ok(path
        .into_os_string()
        .into_string()
        .map_err(|e| EdenFsError::Other(anyhow!("invalid checkout path {:?}", e)))?
        .as_bytes()
        .to_vec())
}

const ENV_KEYS_TO_REMOVE: &[&str] = &[
    "DYLD_LIBRARY_PATH",
    "DYLD_INSERT_LIBRARIES",
    "PAR_LAUNCH_TIMESTAMP",
];
// some processes like hg and arc are sensitive about their environments, we
// clear variables that might make problems for their dynamic linking.
// note buck is even more sensitive see buck.run_buck_command
//
// Clean out par related environment so that we don't cause problems
// for our child process
pub fn get_environment_suitable_for_subprocess() -> Vec<(OsString, OsString)> {
    std::env::vars()
        .filter_map(|(k, v)| {
            if ENV_KEYS_TO_REMOVE.contains(&k.as_str())
                || k.starts_with("FB_PAR")
                || k.starts_with("PYTHON")
            {
                None
            } else {
                Some((k.into(), v.into()))
            }
        })
        .collect()
}
