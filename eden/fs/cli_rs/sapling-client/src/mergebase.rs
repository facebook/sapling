/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::sync::LazyLock;
use std::sync::Mutex;

use anyhow::Context;
use lru_cache::LruCache;
use tokio::process::Command;

use crate::error::Result;
use crate::error::SaplingError;
use crate::utils::get_sapling_executable_path;
use crate::utils::get_sapling_options;

// NOTE: We might wish to cache Results here, but we would want to add a way to evict
// Err entries from the cache based on some policy - e.g. a TTL in seconds.
// For now, we just cache Ok entries.
const MERGEBASE_LRU_CACHE_SIZE: usize = 32;
static MERGEBASE_LRU_CACHE: LazyLock<Mutex<LruCache<String, Option<MergebaseDetails>>>> =
    LazyLock::new(|| Mutex::new(LruCache::new(MERGEBASE_LRU_CACHE_SIZE)));

pub async fn get_mergebase<D, C, M>(
    current_dir: D,
    commit: C,
    mergegase_with: M,
) -> Result<Option<String>>
where
    D: AsRef<Path>,
    C: AsRef<str>,
    M: AsRef<str>,
{
    let details = get_mergebase_details(current_dir, commit, mergegase_with).await?;
    Ok(details.map(|d| d.mergebase))
}

#[derive(Clone, Debug)]
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
    let lru_key = format!("{}:{}", commit.as_ref(), mergegase_with.as_ref());
    {
        let mut lru_cache = MERGEBASE_LRU_CACHE.lock().unwrap();
        let entry = lru_cache.get_mut(&lru_key).cloned();
        if let Some(entry) = entry {
            return Ok(entry);
        }
    }

    let result = {
        let output = Command::new(get_sapling_executable_path())
            .current_dir(current_dir)
            .envs(get_sapling_options())
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
    }?;

    let mut lru_cache = MERGEBASE_LRU_CACHE.lock().unwrap();
    lru_cache.insert(lru_key, result.clone());
    Ok(result)
}

fn parse_mergebase_details(mergebase_details: Vec<u8>) -> Result<Option<MergebaseDetails>> {
    let mergebase_details = String::from_utf8(mergebase_details)?;
    if mergebase_details.is_empty() {
        return Ok(None);
    }
    let v: Vec<&str> = mergebase_details.trim().splitn(3, '\n').collect();
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
