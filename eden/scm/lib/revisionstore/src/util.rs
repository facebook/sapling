/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::ErrorKind;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use edenapi::Stats;
use fn_error_context::context;
use fs_err::OpenOptions;
use fs_err::read_to_string;
use hgtime::HgTime;
use repourl::encode_repo_name;
use tracing::Span;
use util::path::create_dir;
use util::path::create_shared_dir;
use util::path::create_shared_dir_all;

fn get_config_cache_path(config: &dyn Config) -> Result<Option<PathBuf>> {
    let reponame = match config.get_nonempty("remotefilelog", "reponame") {
        Some(name) => name,
        None => return Ok(None),
    };

    let mut path: PathBuf = match config.get_nonempty_opt("remotefilelog", "cachepath")? {
        Some(path) => path,
        None => return Ok(None),
    };

    create_shared_dir_all(&path)?;
    path.push(encode_repo_name(reponame));
    create_shared_dir(&path)?;

    Ok(Some(path))
}

#[context("get_cache_path")]
pub fn get_cache_path(
    config: &dyn Config,
    suffix: &Option<impl AsRef<Path>>,
) -> Result<Option<PathBuf>> {
    let mut path = match get_config_cache_path(config) {
        Ok(Some(path)) => path,
        res => return res,
    };

    if let Some(suffix) = suffix {
        path.push(suffix);
        create_shared_dir(&path)?;
    }

    Ok(Some(path))
}

#[context("get_local_path")]
pub fn get_local_path(mut path: PathBuf, suffix: &Option<impl AsRef<Path>>) -> Result<PathBuf> {
    create_dir(&path)?;

    if let Some(suffix) = suffix {
        path.push(suffix);
        create_dir(&path)?;
    }

    Ok(path)
}

#[context("get_indexedlogdatastore_path")]
pub fn get_indexedlogdatastore_path(path: impl AsRef<Path>) -> Result<PathBuf> {
    let mut path = path.as_ref().to_owned();
    path.push("indexedlogdatastore");
    create_shared_dir_all(&path)?;
    Ok(path)
}

#[context("get_indexedlogdatastore_aux_path")]
pub fn get_indexedlogdatastore_aux_path(path: impl AsRef<Path>) -> Result<PathBuf> {
    let mut path = path.as_ref().to_owned();
    path.push("indexedlogdatastore_aux");
    create_shared_dir_all(&path)?;
    Ok(path)
}

#[context("get_indexedloghistorystore_path")]
pub fn get_indexedloghistorystore_path(path: impl AsRef<Path>) -> Result<PathBuf> {
    let mut path = path.as_ref().to_owned();
    path.push("indexedloghistorystore");
    create_shared_dir_all(&path)?;
    Ok(path)
}

#[context("get_tree_aux_store_path")]
pub fn get_tree_aux_store_path(path: impl AsRef<Path>) -> Result<PathBuf> {
    let mut path = path.as_ref().to_owned();
    path.push("treeaux");
    create_dir(&path)?;
    Ok(path)
}

#[context("get_packs_path")]
pub fn get_packs_path(path: impl AsRef<Path>, suffix: &Option<PathBuf>) -> Result<PathBuf> {
    let mut path = path.as_ref().to_owned();
    path.push("packs");
    create_shared_dir_all(&path)?;

    if let Some(suffix) = suffix {
        path.push(suffix);
        create_shared_dir(&path)?;
    }

    Ok(path)
}

pub fn get_cache_packs_path(
    config: &dyn Config,
    suffix: &Option<PathBuf>,
) -> Result<Option<PathBuf>> {
    let cache_path = match get_config_cache_path(config) {
        Ok(Some(path)) => path,
        res => return res,
    };
    Ok(Some(get_packs_path(cache_path, suffix)?))
}

#[context("get_lfs_path")]
fn get_lfs_path(store_path: impl AsRef<Path>) -> Result<PathBuf> {
    let mut path = store_path.as_ref().to_owned();
    path.push("lfs");
    create_shared_dir_all(&path)?;

    Ok(path)
}

#[context("get_lfs_pointers_path")]
pub fn get_lfs_pointers_path(store_path: impl AsRef<Path>) -> Result<PathBuf> {
    let mut path = get_lfs_path(store_path)?;
    path.push("pointers");
    create_shared_dir(&path)?;

    Ok(path)
}

#[context("get_lfs_objects_path")]
pub fn get_lfs_objects_path(store_path: impl AsRef<Path>) -> Result<PathBuf> {
    let mut path = get_lfs_path(store_path)?;
    path.push("objects");
    create_shared_dir(&path)?;

    Ok(path)
}

#[context("get_lfs_blobs_path")]
pub fn get_lfs_blobs_path(store_path: impl AsRef<Path>) -> Result<PathBuf> {
    let mut path = get_lfs_path(store_path)?;
    path.push("blobs");
    create_shared_dir(&path)?;

    Ok(path)
}

pub const RUN_ONCE_FILENAME: &str = "runoncemarker";
pub fn check_run_once(store_path: impl AsRef<Path>, key: &str, cutoff: HgTime) -> bool {
    if HgTime::now() > Some(cutoff) {
        return false;
    }

    let marker_path = store_path.as_ref().join(RUN_ONCE_FILENAME);
    let line = format!("\n{}\n", key);
    let marked = match read_to_string(&marker_path) {
        Ok(contents) => contents.contains(&line),
        // If the file doesn't exist, it hasn't run yet.
        Err(e) if e.kind() == ErrorKind::NotFound => false,
        // If it's some other IO error (permission denied, etc), just give up.
        _ => return false,
    };

    if !marked {
        let mut fp = OpenOptions::new()
            .create(true)
            .append(true)
            .open(marker_path)
            .unwrap();
        return write!(fp, "{}", line).is_ok();
    }

    false
}

pub fn record_edenapi_stats(span: &Span, stats: &Stats) {
    // Bytes
    span.record("downloaded", stats.downloaded);
    // Bytes
    span.record("uploaded", stats.uploaded);
    span.record("requests", stats.requests);
    // Milliseconds
    span.record(
        "time",
        u64::try_from(stats.time.as_millis()).unwrap_or(u64::MAX),
    );
    // Milliseconds
    span.record(
        "latency",
        u64::try_from(stats.latency.as_millis()).unwrap_or(u64::MAX),
    );
    // Compute the speed in MB/s
    let time = stats.time.as_millis() as f64 / 1000.0;
    let size = stats.downloaded as f64 / 1024.0 / 1024.0;
    span.record("download_speed", format!("{:.2}", size / time).as_str());
}
