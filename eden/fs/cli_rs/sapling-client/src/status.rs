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
const STATUS_LRU_CACHE_SIZE: usize = 32;
static STATUS_LRU_CACHE: LazyLock<Mutex<LruCache<GetStatusParams, SaplingGetStatusResult>>> =
    LazyLock::new(|| Mutex::new(LruCache::new(STATUS_LRU_CACHE_SIZE)));

#[derive(Clone, Debug, PartialEq)]
pub enum SaplingGetStatusResult {
    Normal(Vec<(SaplingStatus, String)>),
    TooManyChanges,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct GetStatusParams {
    first: String,
    second: Option<String>,
    limit_results: usize,
    case_insensitive_suffix_compares: bool,
    root: Option<PathBuf>,
    repository_root: Option<PathBuf>,
    included_roots: Option<Vec<PathBuf>>,
    included_suffixes: Option<Vec<String>>,
    excluded_roots: Option<Vec<PathBuf>>,
    excluded_suffixes: Option<Vec<String>>,
}

pub async fn get_status_with_includes(
    first: &str,
    second: Option<&str>,
    limit_results: usize,
    case_insensitive_suffix_compares: bool,
    repository_root: &Option<PathBuf>,
    root: &Option<PathBuf>,
    included_roots: Vec<PathBuf>,
    included_suffixes: Vec<String>,
) -> Result<SaplingGetStatusResult> {
    get_status(
        first,
        second,
        limit_results,
        case_insensitive_suffix_compares,
        repository_root,
        root,
        &Some(included_roots),
        &Some(included_suffixes),
        &None,
        &None,
    )
    .await
}

// Get status between two revisions. If second is None, then it is the working copy.
// Limit the number of results to limit_results. If the number of results is greater than
// limit_results return TooManyResults. Apply root and suffix filters if provided.
// If repository_root is None, default to the current working directory.
pub async fn get_status(
    first: &str,
    second: Option<&str>,
    limit_results: usize,
    case_insensitive_suffix_compares: bool,
    repository_root: &Option<PathBuf>,
    root: &Option<PathBuf>,
    included_roots: &Option<Vec<PathBuf>>,
    included_suffixes: &Option<Vec<String>>,
    excluded_roots: &Option<Vec<PathBuf>>,
    excluded_suffixes: &Option<Vec<String>>,
) -> Result<SaplingGetStatusResult> {
    let params = GetStatusParams {
        first: first.to_string(),
        second: second.map(|s| s.to_string()),
        limit_results,
        case_insensitive_suffix_compares,
        repository_root: repository_root.clone(),
        root: root.clone(),
        included_roots: included_roots.clone(),
        included_suffixes: included_suffixes.clone(),
        excluded_roots: excluded_roots.clone(),
        excluded_suffixes: excluded_suffixes.clone(),
    };

    get_status_impl(&TokioCommandSpawner, params).await
}

async fn get_status_impl<Spawner>(
    spawner: &Spawner,
    mut params: GetStatusParams,
) -> Result<SaplingGetStatusResult>
where
    Spawner: CommandSpawner,
{
    params.included_suffixes = params.included_suffixes.clone().map(|is| {
        is.into_iter()
            .map(|s| {
                format!(
                    ".{}",
                    if params.case_insensitive_suffix_compares {
                        s.to_ascii_lowercase()
                    } else {
                        s
                    }
                )
            })
            .collect::<Vec<String>>()
    });
    params.excluded_suffixes = params.excluded_suffixes.clone().map(|is| {
        is.into_iter()
            .map(|s| {
                format!(
                    ".{}",
                    if params.case_insensitive_suffix_compares {
                        s.to_ascii_lowercase()
                    } else {
                        s
                    }
                )
            })
            .collect::<Vec<String>>()
    });

    {
        let mut lru_cache = STATUS_LRU_CACHE.lock().unwrap();
        let entry = lru_cache.get_mut(&params).cloned();
        if let Some(entry) = entry {
            return Ok(entry);
        }
    }

    let result = {
        let mut args = vec!["status", "-mardu", "--rev", &params.first];
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
        let command = if let Some(repository_root) = &params.repository_root {
            command.current_dir(repository_root)
        } else {
            &mut command
        };

        let mut child = spawner.spawn(command)?;
        let stdout = child.stdout().take().ok_or_else(|| {
            SaplingError::Other("Failed to read stdout when invoking 'sl status'.".to_string())
        })?;
        let reader = BufReader::new(stdout);

        let mut status = vec![];
        let mut lines = reader.lines();
        while let Some(line) = lines.next_line().await? {
            if let Some(status_line) = process_one_status_line(&line)? {
                if is_path_included(
                    params.case_insensitive_suffix_compares,
                    &status_line.1,
                    &params.included_roots,
                    &params.included_suffixes,
                    &params.excluded_roots,
                    &params.excluded_suffixes,
                ) {
                    if status.len() >= params.limit_results {
                        return Ok(SaplingGetStatusResult::TooManyChanges);
                    }
                    status.push(status_line);
                }
            }
        }

        SaplingGetStatusResult::Normal(status)
    };

    let mut lru_cache = STATUS_LRU_CACHE.lock().unwrap();
    lru_cache.insert(params, result.clone());
    Ok(result)
}

fn is_path_included(
    case_insensitive_suffix_compares: bool,
    path: &str,
    included_roots: &Option<Vec<PathBuf>>,
    included_suffixes: &Option<Vec<String>>,
    excluded_roots: &Option<Vec<PathBuf>>,
    excluded_suffixes: &Option<Vec<String>>,
) -> bool {
    if !included_roots.as_ref().is_none_or(|roots| {
        let path = Path::new(path);
        roots
            .iter()
            .any(|included_root| path.starts_with(included_root))
    }) {
        return false;
    }

    if !included_suffixes.as_ref().is_none_or(|suffixes| {
        suffixes.iter().any(|suffix| {
            if case_insensitive_suffix_compares {
                path.to_ascii_lowercase().ends_with(suffix)
            } else {
                path.ends_with(suffix)
            }
        })
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

    if excluded_suffixes.as_ref().is_some_and(|suffixes| {
        suffixes.iter().any(|suffix| {
            if case_insensitive_suffix_compares {
                path.to_ascii_lowercase().ends_with(suffix)
            } else {
                path.ends_with(suffix)
            }
        })
    }) {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use crate::status::*;
    use crate::types::SaplingStatus;
    use crate::utils::tests::get_mock_spawner;

    const FIRST_COMMIT_ID: &str = "0000111122223333444455556666777788889999";
    const SECOND_COMMIT_ID: &str = "999988887777666655554444333322221100000";

    #[tokio::test]
    pub async fn test_get_status_basic() -> Result<()> {
        let output = r"M fbcode/buck2/app/buck2_audit/src/lib.rs
M fbcode/buck2/app/buck2_audit_server/src/lib.rs
A fbcode/buck2/app/buck2_audit/src/perf.rs
A fbcode/buck2/app/buck2_audit/src/perf/configured_graph_size.rs
A fbcode/buck2/app/buck2_audit_server/src/perf.rs
A fbcode/buck2/app/buck2_audit_server/src/perf/configured_graph_size.rs
";
        let spawner = get_mock_spawner(
            get_sapling_executable_path(),
            Some((0, Some(output.as_bytes().to_vec()))),
        );
        let params = GetStatusParams {
            first: FIRST_COMMIT_ID.to_string(),
            second: Some(SECOND_COMMIT_ID.to_string()),
            limit_results: 1000,
            case_insensitive_suffix_compares: false,
            repository_root: None,
            root: None,
            included_roots: None,
            included_suffixes: None,
            excluded_roots: None,
            excluded_suffixes: None,
        };
        let result = get_status_impl(&spawner, params).await?;
        let expected = SaplingGetStatusResult::Normal(vec![
            (
                SaplingStatus::Modified,
                "fbcode/buck2/app/buck2_audit/src/lib.rs".to_string(),
            ),
            (
                SaplingStatus::Modified,
                "fbcode/buck2/app/buck2_audit_server/src/lib.rs".to_string(),
            ),
            (
                SaplingStatus::Added,
                "fbcode/buck2/app/buck2_audit/src/perf.rs".to_string(),
            ),
            (
                SaplingStatus::Added,
                "fbcode/buck2/app/buck2_audit/src/perf/configured_graph_size.rs".to_string(),
            ),
            (
                SaplingStatus::Added,
                "fbcode/buck2/app/buck2_audit_server/src/perf.rs".to_string(),
            ),
            (
                SaplingStatus::Added,
                "fbcode/buck2/app/buck2_audit_server/src/perf/configured_graph_size.rs".to_string(),
            ),
        ]);

        assert_eq!(result, expected);
        Ok(())
    }
}
