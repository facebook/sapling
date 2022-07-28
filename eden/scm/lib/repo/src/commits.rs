/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use crate::repo::Repo;
use edenapi::EdenApi;
use hgcommits::DagCommits;
use hgcommits::DoubleWriteCommits;
use hgcommits::Error as CommitError;
use hgcommits::GitSegmentedCommits;
use hgcommits::HgCommits;
use hgcommits::HybridCommits;
use hgcommits::RevlogCommits;
use metalog::MetaLog;
use parking_lot::RwLock;

static HG_COMMITS_PATH: &str = "hgcommits/v1";
static LAZY_HASH_PATH: &str = "lazyhashdir";
static SEGMENTS_PATH: &str = "segments/v1";

static DOUBLE_WRITE_REQUIREMENT: &str = "doublewritechangelog";
static HYBRID_REQUIREMENT: &str = "hybridchangelog";
static LAZY_TEXT_REQUIREMENT: &str = "lazytextchangelog";
static GIT_STORE_REQUIREMENT: &str = "git-store";
static LAZY_STORE_REQUIREMENT: &str = "lazychangelog";
static SEGMENTS_REQUIREMENT: &str = "segmentedchangelog";

static GIT_FILE: &str = "gitdir";

pub(crate) fn open_dag_commits(
    repo: &mut Repo,
) -> anyhow::Result<Box<dyn DagCommits + Send + 'static>> {
    let commits = if repo.store_requirements.contains(GIT_STORE_REQUIREMENT) {
        let metalog = repo.metalog()?;
        tracing::info!(target: "changelog_info", changelog_backend="git");
        open_git(repo.store_path(), metalog)?
    } else if repo.store_requirements.contains(LAZY_STORE_REQUIREMENT) {
        let eden_api = repo.eden_api()?;
        tracing::info!(target: "changelog_info", changelog_backend="lazy");
        open_hybrid(repo.store_path(), eden_api, true, false)?
    } else if repo.store_requirements.contains(DOUBLE_WRITE_REQUIREMENT) {
        tracing::info!(target: "changelog_info", changelog_backend="doublewrite");
        open_double(repo.store_path())?
    } else if repo.store_requirements.contains(HYBRID_REQUIREMENT) {
        let eden_api = repo.eden_api()?;
        tracing::info!(target: "changelog_info", changelog_backend="hybrid");
        open_hybrid(repo.store_path(), eden_api, false, true)?
    } else if repo.store_requirements.contains(LAZY_TEXT_REQUIREMENT) {
        let eden_api = repo.eden_api()?;
        tracing::info!(target: "changelog_info", changelog_backend="lazytext");
        open_hybrid(repo.store_path(), eden_api, false, false)?
    } else if repo.store_requirements.contains(SEGMENTS_REQUIREMENT) {
        tracing::info!(target: "changelog_info", changelog_backend="segments");
        open_segments(repo.store_path())?
    } else {
        tracing::info!(target: "changelog_info", changelog_backend="rustrevlog");
        Box::new(RevlogCommits::new(repo.store_path())?)
    };
    Ok(commits)
}

fn open_git(
    store_path: &Path,
    metalog: Arc<RwLock<MetaLog>>,
) -> Result<Box<dyn DagCommits + Send + 'static>, CommitError> {
    let git_path =
        calculate_git_path(store_path).map_err(|err| CommitError::FileReadError("gitdir", err))?;
    let segments_path = calculate_segments_path(store_path);
    let git_segmented_commits = GitSegmentedCommits::new(&git_path, &segments_path)?;
    git_segmented_commits.git_references_to_metalog(&mut metalog.write())?;
    Ok(Box::new(git_segmented_commits))
}

fn open_double(store_path: &Path) -> Result<Box<dyn DagCommits + Send + 'static>, CommitError> {
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
) -> Result<Box<dyn DagCommits + Send + 'static>, CommitError> {
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

fn open_segments(store_path: &Path) -> Result<Box<dyn DagCommits + Send + 'static>, CommitError> {
    let commits = HgCommits::new(
        &calculate_segments_path(store_path),
        &store_path.join(HG_COMMITS_PATH),
    )?;
    Ok(Box::new(commits))
}
