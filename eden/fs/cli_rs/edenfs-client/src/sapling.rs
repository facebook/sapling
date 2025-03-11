/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs::read_to_string;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;

use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::process::Command;

use crate::changes_since::prefix_paths;

#[derive(Debug, PartialEq)]
pub enum SaplingStatus {
    Modified,
    Added,
    Removed,
    Clean,
    Missing,
    NotTracked,
    Ignored,
    Copied,
}

pub enum SaplingGetStatusResult {
    Normal(Vec<(SaplingStatus, String)>),
    TooManyChanges,
}

#[allow(dead_code)]
pub fn is_fbsource_checkout(mount_point: &Path) -> bool {
    let project_id_path = mount_point.join(".projectid");
    let project_id = read_to_string(project_id_path).ok();
    match project_id {
        Some(project_id) => project_id.trim() == "fbsource",
        None => false,
    }
}

pub async fn get_current_commit_id() -> anyhow::Result<String> {
    let output = Command::new("sl")
        .env("HGPLAIN", "1")
        .args(["whereami", "--traceback"])
        .output()
        .await?;
    Ok(String::from_utf8(output.stdout)?)
}

pub async fn get_commit_timestamp(commit_id: &str) -> anyhow::Result<u64> {
    let output = Command::new("sl")
        .env("HGPLAIN", "1")
        .args(["log", "--traceback", "-T", "{date}", "-r", commit_id])
        .output()
        .await?;
    let date = String::from_utf8(output.stdout)?;
    let date = date.parse::<f64>().map_err(anyhow::Error::msg)?;
    // NOTE: Sapling returns fractional seconds, but we only want whole seconds.
    //       Truncate to the nearest second.
    Ok(date as u64)
}

pub async fn is_commit_in_repo(commit_id: &str) -> anyhow::Result<bool> {
    let output = Command::new("sl")
        .env("HGPLAIN", "1")
        .args([
            "log",
            "--traceback",
            "-r",
            commit_id,
            // Disable fbsource <-> www sync during lookup
            "--config",
            "megarepo.transparent-lookup=",
        ])
        .output()
        .await?;
    Ok(output.status.success())
}

pub async fn get_mergebase(commit: &str, mergegase_with: &str) -> anyhow::Result<Option<String>> {
    let output = Command::new("sl")
        .env("HGPLAIN", "1")
        .args([
            "log",
            "-T",
            "{node}",
            "-r",
            format!("ancestor({}, {})", commit, mergegase_with).as_str(),
        ])
        .output()
        .await?;

    let mergebase = String::from_utf8(output.stdout)?;
    if mergebase.is_empty() {
        Ok(None)
    } else {
        Ok(Some(mergebase))
    }
}

pub async fn get_status_with_includes(
    first: &str,
    second: Option<&str>,
    limit_results: usize,
    case_insensitive_suffix_compares: bool,
    root: &Option<PathBuf>,
    included_roots: Vec<PathBuf>,
    included_suffixes: Vec<String>,
) -> anyhow::Result<SaplingGetStatusResult> {
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
// TODO: replace with a method that returns an iterator over (SaplingStatus, String)
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
) -> anyhow::Result<SaplingGetStatusResult> {
    let included_roots =
        prefix_paths(root, included_roots, |p| p).or_else(|| root.clone().map(|r| vec![r]));
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
    let excluded_roots = prefix_paths(root, excluded_roots, |p| p);
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

    let mut output = Command::new("sl")
        .env("HGPLAIN", "1")
        .args(args)
        .stdout(Stdio::piped())
        .spawn()?;

    let stdout = output
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("Failed to read stdout when invoking 'sl status'."))?;
    let reader = BufReader::new(stdout);

    let mut status = vec![];
    let mut lines = reader.lines();
    while let Some(line) = lines.next_line().await? {
        if let Some(mut status_line) = process_one_status_line(&line)? {
            if is_path_included(
                case_insensitive_suffix_compares,
                &status_line.1,
                &included_roots,
                &included_suffixes,
                &excluded_roots,
                &excluded_suffixes,
            ) {
                if status.len() >= limit_results {
                    return Ok(SaplingGetStatusResult::TooManyChanges);
                }
                status_line.1 = strip_prefix_from_string(root, status_line.1);
                status.push(status_line);
            }
        }
    }

    Ok(SaplingGetStatusResult::Normal(status))
}

//
// Single line looks like:
//    <status> <path>
//
// Where status is one of:
//   M = modified
//   A = added
//   R = removed
//   C = clean
//   ! = missing (deleted by a non-sl command, but still tracked)
//   ? = not tracked
//   I = ignored
//     = origin of the previous file (with --copies)
// Note:
//   Paths can have spaces, but are not quoted.
fn process_one_status_line(line: &str) -> anyhow::Result<Option<(SaplingStatus, String)>> {
    // Must include a status and at least one char path.
    let mut chars = line.chars();
    let status = chars
        .next()
        .ok_or_else(|| anyhow::anyhow!("Invalid status line: {line}"))?;
    let space = chars
        .next()
        .ok_or_else(|| anyhow::anyhow!("Invalid status line: {line}"))?;
    if space != ' ' {
        return Err(anyhow::anyhow!("Invalid status line: {line}"));
    }
    let path = line.chars().skip(1).collect::<String>().trim().to_owned();
    let result = match status {
        'M' => Some((SaplingStatus::Modified, path)),
        'A' => Some((SaplingStatus::Added, path)),
        'R' => Some((SaplingStatus::Removed, path)),
        'C' => Some((SaplingStatus::Clean, path)),
        '!' => Some((SaplingStatus::Missing, path)),
        '?' => Some((SaplingStatus::NotTracked, path)),
        'I' => Some((SaplingStatus::Ignored, path)),
        ' ' => Some((SaplingStatus::Copied, path)),
        _ => None, // Skip all others
    };

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
    if !included_roots.as_ref().map_or(true, |roots| {
        let path = Path::new(path);
        roots
            .iter()
            .any(|included_root| path.starts_with(included_root))
    }) {
        return false;
    }

    if !included_suffixes.as_ref().map_or(true, |suffixes| {
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

/// Given a prefix and a path string, return the path with the prefix removed.
///
/// If the prefix is None, the path is returned as-is.
pub fn strip_prefix_from_string(prefix: &Option<PathBuf>, path: String) -> String {
    if let Some(prefix) = prefix {
        let path = Path::new(&path);
        path.strip_prefix(prefix)
            .map_or(path, |stripped_path| stripped_path)
            .to_string_lossy()
            .to_string()
    } else {
        path
    }
}

#[cfg(test)]
mod tests {
    use crate::sapling::*;
    use crate::utils::get_mount_point;

    const FBSOURCE_COMMIT_ID: &str = "5496dd87e5fe7430a1a399530cc339a479097524";
    const WWW_COMMIT_ID: &str = "1061662d6db2072dd30308d1626a45ac11db3467";

    #[fbinit::test]
    pub async fn test_current_commit_id() -> anyhow::Result<()> {
        let commit_id = get_current_commit_id().await?;
        assert!(!commit_id.is_empty());
        Ok(())
    }

    #[fbinit::test]
    pub async fn test_is_commit_in_repo() -> anyhow::Result<()> {
        let mount_point = get_mount_point(&None)?;
        let commit_id = get_current_commit_id().await?;
        assert!(is_commit_in_repo(&commit_id).await?);
        assert_eq!(
            is_commit_in_repo(FBSOURCE_COMMIT_ID).await?,
            is_fbsource_checkout(&mount_point)
        );
        assert_eq!(
            is_commit_in_repo(WWW_COMMIT_ID).await?,
            !is_fbsource_checkout(&mount_point)
        );

        Ok(())
    }

    #[fbinit::test]
    pub async fn test_is_fbsource_checkout() -> anyhow::Result<()> {
        let mount_point = get_mount_point(&None)?;
        assert!(is_fbsource_checkout(&mount_point));
        Ok(())
    }

    #[fbinit::test]
    pub async fn test_get_commit_timestamp() -> anyhow::Result<()> {
        // sl log of commit in fbsource:
        //   changeset:   5496dd87e5fe7430a1a399530cc339a479097524  D68746950
        //   user:        John Elliott <jdelliot@fb.com>
        //   date:        Tue, 28 Jan 2025 10:35:17 -0800
        //   summary:     [meerkat] Improve saved state support
        // timestamp should be 1738089317.028800, but we truncate to the nearest second
        let timestamp = get_commit_timestamp(FBSOURCE_COMMIT_ID).await?;
        assert_eq!(timestamp, 1738089317);
        Ok(())
    }

    #[test]
    fn test_process_one_status_line() -> anyhow::Result<()> {
        assert_eq!(
            process_one_status_line("M buck2/app/buck2_file_watcher/src/edenfs/sapling.rs")?,
            Some((
                SaplingStatus::Modified,
                "buck2/app/buck2_file_watcher/src/edenfs/sapling.rs".to_owned()
            ))
        );

        assert_eq!(
            process_one_status_line("A buck2/app/buck2_file_watcher/src/edenfs/interface.rs")?,
            Some((
                SaplingStatus::Added,
                "buck2/app/buck2_file_watcher/src/edenfs/interface.rs".to_owned()
            ))
        );

        assert_eq!(
            process_one_status_line("R buck2/app/buck2_file_watcher/src/edenfs/utils.rs")?,
            Some((
                SaplingStatus::Removed,
                "buck2/app/buck2_file_watcher/src/edenfs/utils.rs".to_owned()
            ))
        );

        assert_eq!(
            process_one_status_line("! buck2/app/buck2_file_watcher/src/edenfs/sapling.rs")?,
            Some((
                SaplingStatus::Missing,
                "buck2/app/buck2_file_watcher/src/edenfs/sapling.rs".to_owned()
            ))
        );

        assert_eq!(
            process_one_status_line("? buck2/app/buck2_file_watcher/src/edenfs/sapling.rs")?,
            Some((
                SaplingStatus::NotTracked,
                "buck2/app/buck2_file_watcher/src/edenfs/sapling.rs".to_owned()
            ))
        );

        assert_eq!(
            process_one_status_line("C buck2/app/buck2_file_watcher/src/edenfs/sapling.rs")?,
            Some((
                SaplingStatus::Clean,
                "buck2/app/buck2_file_watcher/src/edenfs/sapling.rs".to_owned()
            ))
        );

        assert_eq!(
            process_one_status_line("I buck2/app/buck2_file_watcher/src/edenfs/sapling.rs")?,
            Some((
                SaplingStatus::Ignored,
                "buck2/app/buck2_file_watcher/src/edenfs/sapling.rs".to_owned()
            ))
        );

        assert_eq!(
            process_one_status_line("  buck2/app/buck2_file_watcher/src/edenfs/sapling.rs")?,
            Some((
                SaplingStatus::Copied,
                "buck2/app/buck2_file_watcher/src/edenfs/sapling.rs".to_owned()
            ))
        );

        // Space in path
        assert_eq!(
            process_one_status_line("M ovrsource-legacy/unity/socialvr/modules/wb_unity_asset_bundles/Assets/MetaHorizonUnityAssetBundle/Editor/Unity Dependencies/ABDataSource.cs")?,
            Some((
                SaplingStatus::Modified,
                "ovrsource-legacy/unity/socialvr/modules/wb_unity_asset_bundles/Assets/MetaHorizonUnityAssetBundle/Editor/Unity Dependencies/ABDataSource.cs".to_owned()
            ))
        );

        // Invalid status
        assert!(process_one_status_line("Invalid status").is_err());

        // Invalid status (missing status), but valid path with space in it
        assert!(
            process_one_status_line(" ovrsource-legacy/unity/socialvr/modules/wb_unity_asset_bundles/Assets/MetaHorizonUnityAssetBundle/Editor/Unity Dependencies/ABDataSource.cs")
            .is_err());

        // Malformed status (no space)
        assert!(
            process_one_status_line("Mbuck2/app/buck2_file_watcher/src/edenfs/sapling.rs").is_err()
        );

        // Malformed status (colon instead of space)
        assert!(
            process_one_status_line("M:buck2/app/buck2_file_watcher/src/edenfs/sapling.rs")
                .is_err()
        );

        Ok(())
    }
}
