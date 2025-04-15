/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;

use async_process_traits::Child;
use async_process_traits::Command;
use async_process_traits::CommandSpawner;
use async_process_traits::TokioCommandSpawner;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;

use crate::error::Result;
use crate::error::SaplingError;
use crate::types::SaplingStatus;
use crate::utils::get_sapling_executable_path;
use crate::utils::get_sapling_options;
use crate::utils::process_one_status_line;

#[derive(Clone, Debug, PartialEq)]
pub enum SaplingGetStatusResult {
    Normal(Vec<(SaplingStatus, String)>),
    TooManyChanges,
}

pub async fn get_status_with_includes(
    first: &str,
    second: Option<&str>,
    limit_results: usize,
    case_insensitive_suffix_compares: bool,
    root: &Option<PathBuf>,
    included_roots: Vec<PathBuf>,
    included_suffixes: Vec<String>,
) -> Result<SaplingGetStatusResult> {
    get_status(
        first,
        second,
        limit_results,
        case_insensitive_suffix_compares,
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
pub async fn get_status(
    first: &str,
    second: Option<&str>,
    limit_results: usize,
    case_insensitive_suffix_compares: bool,
    root: &Option<PathBuf>,
    included_roots: &Option<Vec<PathBuf>>,
    included_suffixes: &Option<Vec<String>>,
    excluded_roots: &Option<Vec<PathBuf>>,
    excluded_suffixes: &Option<Vec<String>>,
) -> Result<SaplingGetStatusResult> {
    get_status_impl(
        &TokioCommandSpawner,
        first,
        second,
        limit_results,
        case_insensitive_suffix_compares,
        root,
        included_roots,
        included_suffixes,
        excluded_roots,
        excluded_suffixes,
    )
    .await
}

pub async fn get_status_impl<Spawner>(
    spawner: &Spawner,
    first: &str,
    second: Option<&str>,
    limit_results: usize,
    case_insensitive_suffix_compares: bool,
    root: &Option<PathBuf>,
    included_roots: &Option<Vec<PathBuf>>,
    included_suffixes: &Option<Vec<String>>,
    excluded_roots: &Option<Vec<PathBuf>>,
    excluded_suffixes: &Option<Vec<String>>,
) -> Result<SaplingGetStatusResult>
where
    Spawner: CommandSpawner,
{
    let included_suffixes = included_suffixes.clone().map(|is| {
        is.into_iter()
            .map(|s| {
                format!(
                    ".{}",
                    if case_insensitive_suffix_compares {
                        s.to_ascii_lowercase()
                    } else {
                        s
                    }
                )
            })
            .collect::<Vec<String>>()
    });
    let excluded_suffixes = excluded_suffixes.clone().map(|is| {
        is.into_iter()
            .map(|s| {
                format!(
                    ".{}",
                    if case_insensitive_suffix_compares {
                        s.to_ascii_lowercase()
                    } else {
                        s
                    }
                )
            })
            .collect::<Vec<String>>()
    });

    let mut args = vec!["status", "-mardu", "--rev", first];
    if let Some(second) = second {
        args.push("--rev");
        args.push(second);
    }

    let root_path_arg: String;
    if let Some(root) = root {
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
        SaplingError::Other("Failed to read stdout when invoking 'sl status'.".to_string())
    })?;
    let reader = BufReader::new(stdout);

    let mut status = vec![];
    let mut lines = reader.lines();
    while let Some(line) = lines.next_line().await? {
        if let Some(status_line) = process_one_status_line(&line)? {
            if is_path_included(
                case_insensitive_suffix_compares,
                &status_line.1,
                included_roots,
                &included_suffixes,
                excluded_roots,
                &excluded_suffixes,
            ) {
                if status.len() >= limit_results {
                    return Ok(SaplingGetStatusResult::TooManyChanges);
                }
                status.push(status_line);
            }
        }
    }

    Ok(SaplingGetStatusResult::Normal(status))
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
        let result = get_status_impl(
            &spawner,
            FIRST_COMMIT_ID,
            Some(SECOND_COMMIT_ID),
            1000,
            false,
            &None,
            &None,
            &None,
            &None,
            &None,
        )
        .await?;
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
