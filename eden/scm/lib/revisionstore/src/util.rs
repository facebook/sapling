/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs::read_to_string;
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use edenapi::Stats;
use hgtime::HgTime;
use thiserror::Error;
use tracing::Span;
use util::path::create_dir;
use util::path::create_shared_dir;

#[derive(Error, Debug)]
pub enum Error {
    #[error("could not find config option {0}")]
    ConfigNotSet(String),
}

pub fn get_str_config(config: &dyn Config, section: &str, name: &str) -> Result<String> {
    let name = config
        .get(section, name)
        .ok_or_else(|| Error::ConfigNotSet(format!("{}.{}", section, name)))?;
    Ok(name.to_string())
}

pub fn get_repo_name(config: &dyn Config) -> Result<String> {
    get_str_config(config, "remotefilelog", "reponame")
}

fn get_config_cache_path(config: &dyn Config) -> Result<PathBuf> {
    let reponame = get_repo_name(config)?;
    let config_path: PathBuf = config
        .get_or_default::<Option<_>>("remotefilelog", "cachepath")?
        .ok_or_else(|| Error::ConfigNotSet("remotefilelog.cachepath".into()))?;
    let mut path = PathBuf::new();
    path.push(config_path);
    create_shared_dir(&path)?;
    path.push(reponame);
    create_shared_dir(&path)?;
    Ok(path)
}

pub fn get_cache_path(config: &dyn Config, suffix: &Option<impl AsRef<Path>>) -> Result<PathBuf> {
    let mut path = get_config_cache_path(config)?;

    if let Some(ref suffix) = suffix {
        path.push(suffix);
        create_shared_dir(&path)?;
    }

    Ok(path)
}

pub fn get_local_path(local_path: PathBuf, suffix: &Option<impl AsRef<Path>>) -> Result<PathBuf> {
    let mut path = local_path;
    create_dir(&path)?;

    if let Some(ref suffix) = suffix {
        path.push(suffix);
        create_dir(&path)?;
    }

    Ok(path)
}

pub fn get_indexedlogdatastore_path(path: impl AsRef<Path>) -> Result<PathBuf> {
    let mut path = path.as_ref().to_owned();
    path.push("indexedlogdatastore");
    create_shared_dir(&path)?;
    Ok(path)
}

pub fn get_indexedlogdatastore_aux_path(path: impl AsRef<Path>) -> Result<PathBuf> {
    let mut path = path.as_ref().to_owned();
    path.push("indexedlogdatastore_aux");
    create_shared_dir(&path)?;
    Ok(path)
}

pub fn get_indexedloghistorystore_path(path: impl AsRef<Path>) -> Result<PathBuf> {
    let mut path = path.as_ref().to_owned();
    path.push("indexedloghistorystore");
    create_shared_dir(&path)?;
    Ok(path)
}

pub fn get_packs_path(path: impl AsRef<Path>, suffix: &Option<PathBuf>) -> Result<PathBuf> {
    let mut path = path.as_ref().to_owned();
    path.push("packs");
    create_shared_dir(&path)?;

    if let Some(suffix) = suffix {
        path.push(suffix);
        create_shared_dir(&path)?;
    }

    Ok(path)
}

pub fn get_cache_packs_path(config: &dyn Config, suffix: &Option<PathBuf>) -> Result<PathBuf> {
    get_packs_path(get_config_cache_path(config)?, suffix)
}

fn get_lfs_path(store_path: impl AsRef<Path>) -> Result<PathBuf> {
    let mut path = store_path.as_ref().to_owned();
    path.push("lfs");
    create_shared_dir(&path)?;

    Ok(path)
}

pub fn get_lfs_pointers_path(store_path: impl AsRef<Path>) -> Result<PathBuf> {
    let mut path = get_lfs_path(store_path)?;
    path.push("pointers");
    create_shared_dir(&path)?;

    Ok(path)
}

pub fn get_lfs_objects_path(store_path: impl AsRef<Path>) -> Result<PathBuf> {
    let mut path = get_lfs_path(store_path)?;
    path.push("objects");
    create_shared_dir(&path)?;

    Ok(path)
}

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

    return false;
}

pub fn record_edenapi_stats(span: &Span, stats: &Stats) {
    // Bytes
    span.record("downloaded", &stats.downloaded);
    // Bytes
    span.record("uploaded", &stats.uploaded);
    span.record("requests", &stats.requests);
    // Milliseconds
    span.record(
        "time",
        &u64::try_from(stats.time.as_millis()).unwrap_or(u64::MAX),
    );
    // Milliseconds
    span.record(
        "latency",
        &u64::try_from(stats.latency.as_millis()).unwrap_or(u64::MAX),
    );
    // Compute the speed in MB/s
    let time = stats.time.as_millis() as f64 / 1000.0;
    let size = stats.downloaded as f64 / 1024.0 / 1024.0;
    span.record("download_speed", &format!("{:.2}", size / time).as_str());
}
