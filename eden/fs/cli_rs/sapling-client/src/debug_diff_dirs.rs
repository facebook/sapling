/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::LazyLock;
use std::sync::Mutex;

use async_process_traits::Child;
use async_process_traits::Command;
use async_process_traits::CommandSpawner;
use async_process_traits::TokioCommandSpawner;
use lru_cache::LruCache;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;

use crate::error::Result;
use crate::error::SaplingError;
use crate::types::SaplingStatus;
use crate::utils::get_sapling_executable_path;
use crate::utils::get_sapling_options;
use crate::utils::process_one_status_line;

// NOTE: We might wish to cache Results here, but we would want to add a way to evict
// Err entries from the cache based on some policy - e.g. a TTL in seconds.
// For now, we just cache Ok entries.
const DIFF_DIRS_LRU_CACHE_SIZE: usize = 32;
static DIFF_DIRS_LRU_CACHE: LazyLock<Mutex<LruCache<GetDiffDirsParams, SaplingGetDiffDirsResult>>> =
    LazyLock::new(|| Mutex::new(LruCache::new(DIFF_DIRS_LRU_CACHE_SIZE)));

#[derive(Clone, Debug, PartialEq)]
pub enum SaplingGetDiffDirsResult {
    Normal(Vec<(SaplingStatus, String)>),
    TooManyChanges,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct GetDiffDirsParams {
    first: String,
    second: Option<String>,
    limit_results: usize,
    root: Option<PathBuf>,
    included_roots: Option<Vec<PathBuf>>,
    excluded_roots: Option<Vec<PathBuf>>,
}

pub async fn get_diff_dirs_with_includes(
    first: &str,
    second: Option<&str>,
    limit_results: usize,
    root: &Option<PathBuf>,
    included_roots: Vec<PathBuf>,
) -> Result<SaplingGetDiffDirsResult> {
    get_diff_dirs(
        first,
        second,
        limit_results,
        root,
        &Some(included_roots),
        &None,
    )
    .await
}

// Get directory differences between two revisions. If second is None, then it is the working copy.
// Limit the number of results to limit_results. If the number of results is greater than
// limit_results return TooManyResults. Apply root filters if provided.
pub async fn get_diff_dirs(
    first: &str,
    second: Option<&str>,
    limit_results: usize,
    root: &Option<PathBuf>,
    included_roots: &Option<Vec<PathBuf>>,
    excluded_roots: &Option<Vec<PathBuf>>,
) -> Result<SaplingGetDiffDirsResult> {
    let params = GetDiffDirsParams {
        first: first.to_string(),
        second: second.map(|s| s.to_string()),
        limit_results,
        root: root.clone(),
        included_roots: included_roots.clone(),
        excluded_roots: excluded_roots.clone(),
    };

    get_diff_dirs_impl(&TokioCommandSpawner, params).await
}

async fn get_diff_dirs_impl<Spawner>(
    spawner: &Spawner,
    params: GetDiffDirsParams,
) -> Result<SaplingGetDiffDirsResult>
where
    Spawner: CommandSpawner,
{
    {
        let mut lru_cache = DIFF_DIRS_LRU_CACHE.lock().unwrap();
        let entry = lru_cache.get_mut(&params).cloned();
        if let Some(entry) = entry {
            return Ok(entry);
        }
    }

    let result = {
        let mut args = vec!["debugdiffdirs", "--rev", &params.first];
        let second: String;
        if let Some(second_) = &params.second {
            second = second_.to_string();
            args.push("--rev");
            args.push(&second);
        }

        let root_path_arg: String;
        if let Some(root) = &params.root {
            root_path_arg = format!("path:{}", root.display());
            args.push(&root_path_arg);
        };

        let mut command = Spawner::Command::new(get_sapling_executable_path());
        command
            .envs(get_sapling_options())
            .args(args)
            .stdout(Stdio::piped());
        let mut child = spawner.spawn(&mut command)?;
        let stdout = child.stdout().take().ok_or_else(|| {
            SaplingError::Other(
                "Failed to read stdout when invoking 'sl debugdiffdirs'.".to_string(),
            )
        })?;
        let reader = BufReader::new(stdout);

        let mut status = vec![];
        let mut lines = reader.lines();
        while let Some(line) = lines.next_line().await? {
            if let Some(status_line) = process_one_status_line(&line)? {
                if is_path_included(
                    &status_line.1,
                    &params.included_roots,
                    &params.excluded_roots,
                ) {
                    if status.len() >= params.limit_results {
                        return Ok(SaplingGetDiffDirsResult::TooManyChanges);
                    }
                    status.push(status_line);
                }
            }
        }

        SaplingGetDiffDirsResult::Normal(status)
    };

    let mut lru_cache = DIFF_DIRS_LRU_CACHE.lock().unwrap();
    lru_cache.insert(params, result.clone());
    Ok(result)
}

fn is_path_included(
    path: &str,
    included_roots: &Option<Vec<PathBuf>>,
    excluded_roots: &Option<Vec<PathBuf>>,
) -> bool {
    if !included_roots.as_ref().is_none_or(|roots| {
        let path = Path::new(path);
        roots
            .iter()
            .any(|included_root| path.starts_with(included_root))
    }) {
        return false;
    }

    if excluded_roots.as_ref().is_some_and(|roots| {
        let path = Path::new(path);
        roots
            .iter()
            .any(|excluded_root| path.starts_with(excluded_root))
    }) {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use crate::debug_diff_dirs::*;
    use crate::types::SaplingStatus;
    use crate::utils::tests::get_mock_spawner;

    const FIRST_COMMIT_ID: &str = "0000111122223333444455556666777788889999";
    const SECOND_COMMIT_ID: &str = "999988887777666655554444333322221100000";

    #[tokio::test]
    pub async fn test_get_diff_dirs_basic() -> Result<()> {
        let output = r"M fbcode/buck2/app/buck2_audit/src
M fbcode/buck2/app/buck2_audit_server/src
A fbcode/buck2/app/buck2_audit/src/perf
A fbcode/buck2/app/buck2_audit_server/src/perf
";
        let spawner = get_mock_spawner(
            get_sapling_executable_path(),
            Some((0, Some(output.as_bytes().to_vec()))),
        );
        let params = GetDiffDirsParams {
            first: FIRST_COMMIT_ID.to_string(),
            second: Some(SECOND_COMMIT_ID.to_string()),
            limit_results: 1000,
            root: None,
            included_roots: None,
            excluded_roots: None,
        };
        let result = get_diff_dirs_impl(&spawner, params).await?;
        let expected = SaplingGetDiffDirsResult::Normal(vec![
            (
                SaplingStatus::Modified,
                "fbcode/buck2/app/buck2_audit/src".to_string(),
            ),
            (
                SaplingStatus::Modified,
                "fbcode/buck2/app/buck2_audit_server/src".to_string(),
            ),
            (
                SaplingStatus::Added,
                "fbcode/buck2/app/buck2_audit/src/perf".to_string(),
            ),
            (
                SaplingStatus::Added,
                "fbcode/buck2/app/buck2_audit_server/src/perf".to_string(),
            ),
        ]);

        assert_eq!(result, expected);
        Ok(())
    }
}
