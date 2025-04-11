/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;

use anyhow::Context;
use tokio::process::Command;

use crate::error::Result;
use crate::error::SaplingError;
use crate::utils::get_sapling_executable_path;
use crate::utils::get_sapling_options;

pub async fn get_mergebase(commit: &str, mergegase_with: &str) -> Result<Option<String>> {
    let output = Command::new(get_sapling_executable_path())
        .envs(get_sapling_options())
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

#[derive(Clone)]
pub struct MergebaseDetails {
    pub mergebase: String,
    pub timestamp: Option<u64>,
    pub global_rev: Option<u64>,
}

impl PartialEq for MergebaseDetails {
    fn eq(&self, other: &Self) -> bool {
        self.mergebase == other.mergebase
    }
}

// TODO(T219988735): This code is copied from https://www.internalfb.com/code/fbsource/fbcode/buck2/app/buck2_file_watcher/src/edenfs/sapling.rs
// We will work with the buck2 team to remove this duplication, by migrating buck2 to use the edenfs-client & sapling-client crates.
pub async fn get_mergebase_details<D, C, M>(
    current_dir: D,
    commit: C,
    mergegase_with: M,
) -> Result<Option<MergebaseDetails>>
where
    D: AsRef<Path>,
    C: AsRef<str>,
    M: AsRef<str>,
{
    let output = Command::new(get_sapling_executable_path())
        .envs(get_sapling_options())
        .current_dir(current_dir)
        .args([
            "log",
            "--traceback",
            "-T",
            "{node}\n{date}\n{get(extras, \"global_rev\")}",
            "-r",
            format!("ancestor({}, {})", commit.as_ref(), mergegase_with.as_ref()).as_str(),
        ])
        .output()
        .await?;

    if !output.status.success() || !output.stderr.is_empty() {
        Err(SaplingError::Other(format!(
            "Failed to obtain mergebase:\n{}",
            String::from_utf8(output.stderr).unwrap_or("Failed to parse stderr".to_string())
        )))
    } else {
        parse_mergebase_details(output.stdout)
    }
}

fn parse_mergebase_details(output: Vec<u8>) -> Result<Option<MergebaseDetails>> {
    let output = String::from_utf8(output)?;
    if output.is_empty() {
        return Ok(None);
    }
    let v: Vec<&str> = output.trim().splitn(3, '\n').collect();
    let mergebase = v
        .first()
        .with_context(|| "Failed to parse mergebase")?
        .to_string();
    let timestamp = v
        .get(1)
        .and_then(|t| t.parse::<f64>().ok())
        .map(|t| t as u64); // sl returns the fractional seconds
    let global_rev = if let Some(global_rev) = v.get(2) {
        Some(global_rev.parse::<u64>()?)
    } else {
        None
    };

    Ok(Some(MergebaseDetails {
        mergebase,
        timestamp,
        global_rev,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mergebase_details() -> Result<()> {
        // the format is {node}\n{date}\n{global_rev}
        let output =
            "71de423b796418e8ff5300dbe9bd9ad3aef63a9c\n1739790802.028800\n1020164040".to_owned();
        let details = parse_mergebase_details(output.as_bytes().to_vec())?.unwrap();
        assert_eq!(
            details.mergebase,
            "71de423b796418e8ff5300dbe9bd9ad3aef63a9c"
        );
        assert_eq!(details.timestamp, Some(1739790802));
        assert_eq!(details.global_rev, Some(1020164040));
        Ok(())
    }

    #[test]
    fn test_parse_mergebase_details_no_global_rev() -> Result<()> {
        // Not all repos have global revision
        let output = "71de423b796418e8ff5300dbe9bd9ad3aef63a9c\n1739790802.028800\n".to_owned();
        let details = parse_mergebase_details(output.as_bytes().to_vec())?.unwrap();
        assert_eq!(
            details.mergebase,
            "71de423b796418e8ff5300dbe9bd9ad3aef63a9c"
        );
        assert_eq!(details.global_rev, None);
        assert_eq!(details.timestamp, Some(1739790802));
        Ok(())
    }
}
