/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Register factory constructors.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use commits_trait::DagCommits;
use edenapi::EdenApi;
use fs_err as fs;
use storemodel::StoreInfo;

use crate::DoubleWriteCommits;
use crate::HgCommits;
use crate::HybridCommits;
use crate::RevlogCommits;

macro_rules! concat_os_path {
    ($p1:literal, $p2:literal) => {
        // Cannot use std::path::MAIN_SEPARATOR inside concat! yet.
        if cfg!(windows) {
            concat!($p1, '\\', $p2)
        } else {
            concat!($p1, '/', $p2)
        }
    };
}

const HG_COMMITS_PATH: &str = concat_os_path!("hgcommits", "v1");
const LAZY_HASH_PATH: &str = "lazyhashdir";
const SEGMENTS_PATH: &str = concat_os_path!("segments", "v1");

const DOUBLE_WRITE_REQUIREMENT: &str = "doublewritechangelog";
const HYBRID_REQUIREMENT: &str = "hybridchangelog";
const LAZY_TEXT_REQUIREMENT: &str = "lazytextchangelog";
const LAZY_STORE_REQUIREMENT: &str = "lazychangelog";
const SEGMENTS_REQUIREMENT: &str = "segmentedchangelog";

pub(crate) fn setup_commits_constructor() {
    factory::register_constructor("20-hgcommits", maybe_construct_commits);
}

fn get_required_edenapi(info: &dyn StoreInfo) -> anyhow::Result<Arc<dyn EdenApi>> {
    match info.remote_peer()? {
        Some(p) => Ok(p),
        None => {
            anyhow::bail!("The commit graph requires a remote peer but the repo does not have one.")
        }
    }
}

fn maybe_construct_commits(
    info: &dyn StoreInfo,
) -> anyhow::Result<Option<Box<dyn DagCommits + Send + 'static>>> {
    if info.has_requirement(LAZY_STORE_REQUIREMENT) {
        let eden_api = get_required_edenapi(info)?;
        tracing::info!(target: "changelog_info", changelog_backend="lazy");
        Ok(Some(open_hybrid(info.store_path(), eden_api, true, false)?))
    } else if info.has_requirement(DOUBLE_WRITE_REQUIREMENT) {
        tracing::info!(target: "changelog_info", changelog_backend="doublewrite");
        Ok(Some(open_double(info.store_path())?))
    } else if info.has_requirement(HYBRID_REQUIREMENT) {
        let eden_api = get_required_edenapi(info)?;
        tracing::info!(target: "changelog_info", changelog_backend="hybrid");
        Ok(Some(open_hybrid(info.store_path(), eden_api, false, true)?))
    } else if info.has_requirement(LAZY_TEXT_REQUIREMENT) {
        let eden_api = get_required_edenapi(info)?;
        tracing::info!(target: "changelog_info", changelog_backend="lazytext");
        Ok(Some(open_hybrid(
            info.store_path(),
            eden_api,
            false,
            false,
        )?))
    } else if info.has_requirement(SEGMENTS_REQUIREMENT) {
        tracing::info!(target: "changelog_info", changelog_backend="segments");
        Ok(Some(open_segments(info.store_path())?))
    } else {
        tracing::info!(target: "changelog_info", changelog_backend="rustrevlog");
        Ok(Some(Box::new(RevlogCommits::new(info.store_path())?)))
    }
}

fn open_double(store_path: &Path) -> anyhow::Result<Box<dyn DagCommits + Send + 'static>> {
    let segments_path = calculate_segments_path(store_path);
    let hg_commits_path = store_path.join(HG_COMMITS_PATH);
    let double_commits = DoubleWriteCommits::new(
        store_path,
        segments_path.as_path(),
        hg_commits_path.as_path(),
    )?;
    Ok(Box::new(double_commits))
}

fn open_hybrid(
    store_path: &Path,
    eden_api: Arc<dyn EdenApi>,
    lazy_hash: bool,
    use_revlog: bool,
) -> anyhow::Result<Box<dyn DagCommits + Send + 'static>> {
    let segments_path = calculate_segments_path(store_path);
    let hg_commits_path = store_path.join(HG_COMMITS_PATH);
    let lazy_hash_path = get_path_from_file(store_path, LAZY_HASH_PATH);
    let mut hybrid_commits = HybridCommits::new(
        if use_revlog { Some(store_path) } else { None },
        segments_path.as_path(),
        hg_commits_path.as_path(),
        eden_api,
    )?;
    if let Ok(lazy_path) = lazy_hash_path {
        hybrid_commits.enable_lazy_commit_hashes_from_local_segments(lazy_path.as_path())?;
    } else if lazy_hash {
        hybrid_commits.enable_lazy_commit_hashes();
    }
    Ok(Box::new(hybrid_commits))
}

fn calculate_segments_path(store_path: &Path) -> PathBuf {
    store_path.join(SEGMENTS_PATH)
}

fn get_path_from_file(store_path: &Path, target_file: &str) -> Result<PathBuf, std::io::Error> {
    let path_file = store_path.join(target_file);
    fs::read_to_string(path_file).map(PathBuf::from)
}

fn open_segments(store_path: &Path) -> anyhow::Result<Box<dyn DagCommits + Send + 'static>> {
    let commits = HgCommits::new(
        &calculate_segments_path(store_path),
        &store_path.join(HG_COMMITS_PATH),
    )?;
    Ok(Box::new(commits))
}
