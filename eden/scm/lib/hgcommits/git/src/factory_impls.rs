/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Register factory constructors.

use std::path::Path;
use std::path::PathBuf;

use commits_trait::DagCommits;
use fs_err as fs;
use metalog::MetaLog;
use storemodel::StoreInfo;

use crate::git::GitSegmentedCommits;

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

const SEGMENTS_PATH: &str = concat_os_path!("segments", "v1");
const GIT_STORE_REQUIREMENT: &str = "git-store";
const GIT_FILE: &str = "gitdir";

pub(crate) fn setup_commits_constructor() {
    factory::register_constructor("10-git-commits", maybe_construct_commits);
}

fn maybe_construct_commits(
    info: &dyn StoreInfo,
) -> anyhow::Result<Option<Box<dyn DagCommits + Send + 'static>>> {
    if info.has_requirement(GIT_STORE_REQUIREMENT) {
        tracing::info!(target: "changelog_info", changelog_backend="git");
        Ok(Some(open_git(info.store_path())?))
    } else {
        Ok(None)
    }
}

fn open_git(store_path: &Path) -> anyhow::Result<Box<dyn DagCommits + Send + 'static>> {
    // This is a hacky way to sync back from git references to metalog so we
    // pick up effects after git commands like `push` or `fetch`, or if the
    // user manually run git commands in the repo.
    //
    // Ideally we do this after running the git commands, or just use our own
    // store without needing to sync with git references.
    //
    // Since this is considered as a temporary hack, the metalog requirement
    // does not justify a `StoreInfo::get_metalog` API. So we construct a
    // metalog directly here.
    let metalog_path = store_path.join("metalog");
    let mut metalog = MetaLog::open_from_env(&metalog_path)?;
    let git_path = calculate_git_path(store_path)?;
    let segments_path = calculate_segments_path(store_path);
    let git_segmented_commits = GitSegmentedCommits::new(&git_path, &segments_path)?;
    git_segmented_commits.git_references_to_metalog(&mut metalog)?;
    Ok(Box::new(git_segmented_commits))
}

fn calculate_git_path(store_path: &Path) -> Result<PathBuf, std::io::Error> {
    let git_file_contents = get_path_from_file(store_path, GIT_FILE)?;
    let git_path = PathBuf::from(&git_file_contents);
    if !git_path.is_absolute() {
        return Ok(store_path.join(git_path));
    }
    Ok(git_path)
}

fn calculate_segments_path(store_path: &Path) -> PathBuf {
    store_path.join(SEGMENTS_PATH)
}

fn get_path_from_file(store_path: &Path, target_file: &str) -> Result<PathBuf, std::io::Error> {
    let path_file = store_path.join(target_file);
    fs::read_to_string(path_file).map(PathBuf::from)
}
