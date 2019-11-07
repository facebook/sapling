/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::{Path, PathBuf};

use failure::{format_err, Fallible};

use configparser::{config::ConfigSet, hg::ConfigSetHgExt};
use util::path::create_dir;

fn get_repo_name(config: &ConfigSet) -> Fallible<String> {
    let name = config
        .get("remotefilelog", "reponame")
        .ok_or_else(|| format_err!("remotefilelog.reponame is not set"))?;
    Ok(String::from_utf8(name.to_vec())?)
}

fn get_cache_path(config: &ConfigSet) -> Fallible<PathBuf> {
    let reponame = get_repo_name(config)?;
    let config_path: PathBuf = config
        .get_or_default::<Option<_>>("remotefilelog", "cachepath")?
        .ok_or_else(|| format_err!("remotefilelog.cachepath is not set"))?;
    let mut path = PathBuf::new();
    path.push(config_path);
    path.push(reponame);
    create_dir(&path)?;
    Ok(path)
}

pub fn get_cache_packs_path(config: &ConfigSet, suffix: Option<&Path>) -> Fallible<PathBuf> {
    let mut path = get_cache_path(config)?;
    path.push("packs");
    create_dir(&path)?;
    if let Some(suffix) = suffix {
        path.push(suffix);
    }
    create_dir(&path)?;
    Ok(path)
}

pub fn get_cache_indexedlogdatastore_path(config: &ConfigSet) -> Fallible<PathBuf> {
    let mut path = get_cache_path(config)?;
    path.push("indexedlogdatastore");
    create_dir(&path)?;
    Ok(path)
}

pub fn get_cache_indexedloghistorystore_path(config: &ConfigSet) -> Fallible<PathBuf> {
    let mut path = get_cache_path(config)?;
    path.push("indexedloghistorystore");
    create_dir(&path)?;
    Ok(path)
}

pub fn get_local_packs_path(path: impl AsRef<Path>, suffix: Option<&Path>) -> Fallible<PathBuf> {
    let mut path = path.as_ref().to_owned();
    path.push("packs");
    create_dir(&path)?;

    if let Some(suffix) = suffix {
        path.push(suffix);
    }

    create_dir(&path)?;
    Ok(path)
}
