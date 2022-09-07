/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;

use clidispatch::ReqCtx;
use configmodel::ConfigExt;

use super::NoOpts;
use super::Repo;
use super::Result;

pub fn run(_ctx: ReqCtx<NoOpts>, repo: &mut Repo) -> Result<u8> {
    let store_path = repo.store_path();
    let patterns = [
        "indexedlogdatastore/*",
        "indexedloghistorystore/*",
        "00changelog.*",
        "hgcommits/**/*",
        "segments/**/*",
        "mutation/**/*",
        "metalog/**/*",
        "allheads/**/*",
        "lfs/**/*",
    ];
    fsyncglob::fsync_glob(store_path, &patterns, None);

    let dot_hg_path = repo.dot_hg_path();
    let patterns = ["treestate/*", "dirstate"];
    fsyncglob::fsync_glob(dot_hg_path, &patterns, None);

    if let Some(Some(cache_path)) = repo
        .config()
        .get_opt::<String>("remotefilelog", "cachepath")
        .ok()
    {
        let patterns = ["*/indexedlog*/*", "*/lfs/*"];
        fsyncglob::fsync_glob(Path::new(&cache_path), &patterns, None);
    }

    Ok(0)
}

pub fn name() -> &'static str {
    "debugfsync"
}

pub fn doc() -> &'static str {
    "call fsync on newly modified key storage files"
}

pub fn synopsis() -> Option<&'static str> {
    None
}
