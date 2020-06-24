/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::str::FromStr;

use anyhow::{anyhow, Result};
use ascii::AsciiString;
use bookmarks_types::BookmarkName;
use itertools::Itertools;
use metaconfig_types::{
    CommitSyncConfig, CommitSyncDirection, DefaultSmallToLargeCommitSyncPathAction,
    SmallRepoCommitSyncConfig,
};
use mononoke_types::{MPath, RepositoryId};
use repos::{RawCommitSyncConfig, RawCommitSyncSmallRepoConfig};

use crate::convert::Convert;

fn check_no_duplicate_small_repos(small_repos: &[RawCommitSyncSmallRepoConfig]) -> Result<()> {
    let small_repo_counts: HashMap<i32, u32> = {
        let mut counts = HashMap::new();
        for sr in small_repos.iter() {
            let count = counts.entry(sr.repoid).or_insert(0);
            *count += 1;
        }

        counts
    };

    for (small_repo_id, count) in small_repo_counts.iter() {
        if *count == 1 {
            continue;
        }

        return Err(anyhow!(
            "{:?} is present multiple times in the same CommitSyncConfig",
            RepositoryId::new(*small_repo_id)
        ));
    }

    Ok(())
}

/// Validate the commit sync config
///
/// - Check that all the prefixes in the large repo (target prefixes in a map and prefixes
/// from `DefaultSmallToLargeCommitSyncPathAction::PrependPrefix`) are independent, e.g. aren't prefixes
/// of each other, if the sync direction is small-to-large. This is not allowed, because
/// otherwise there is no way to prevent path conflicts. For example, if one repo maps
/// `p1 => foo/bar` and the other maps `p2 => foo`, both repos can accept commits that
/// change `foo` and these commits can contain path conflicts. Given that the repos have
/// already replied successfully to their clients, it's too late to reject these commits.
/// To avoid this problem, we remove the possiblity of path conflicts altogether.
///
/// - Check that no two small repos use the same bookmark prefix. If they did, this would
/// mean potentail bookmark name collisions.
///
/// - Check that large repo from this config is not the same as any of the small repos
fn validate_commit_sync_config(commit_sync_config: &CommitSyncConfig) -> Result<()> {
    if commit_sync_config
        .small_repos
        .contains_key(&commit_sync_config.large_repo_id)
    {
        return Err(anyhow!(
            "Large repo ({}) is one of the small repos too",
            commit_sync_config.large_repo_id
        ));
    }

    let all_prefixes_with_direction: Vec<(&MPath, CommitSyncDirection)> = commit_sync_config
        .small_repos
        .values()
        .flat_map(|small_repo_sync_config| {
            let SmallRepoCommitSyncConfig {
                default_action,
                map,
                direction,
                ..
            } = small_repo_sync_config;
            let all_prefixes = map.values();
            match default_action {
                DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(prefix) => {
                    all_prefixes.chain(vec![prefix].into_iter())
                }
                DefaultSmallToLargeCommitSyncPathAction::Preserve => {
                    all_prefixes.chain(vec![].into_iter())
                }
            }
            .map(move |prefix| (prefix, direction.clone()))
        })
        .collect();

    let bookmark_prefixes: Vec<&AsciiString> = commit_sync_config
        .small_repos
        .iter()
        .map(|(_, sr)| &sr.bookmark_prefix)
        .collect();

    for ((first_prefix, first_direction), (second_prefix, second_direction)) in
        all_prefixes_with_direction
            .iter()
            .tuple_combinations::<(_, _)>()
    {
        if first_prefix == second_prefix
            && *first_direction == CommitSyncDirection::LargeToSmall
            && *second_direction == CommitSyncDirection::LargeToSmall
        {
            // when syncing large-to-small, it is allowed to have identical prefixes,
            // but not prefixes that are proper prefixes of other prefixes
            continue;
        }
        validate_mpath_prefixes(first_prefix, second_prefix)?;
    }

    // No two small repos can have the same bookmark prefix
    for (first_prefix, second_prefix) in bookmark_prefixes.iter().tuple_combinations::<(_, _)>() {
        let fp = first_prefix.as_str();
        let sp = second_prefix.as_str();
        if fp.starts_with(sp) || sp.starts_with(fp) {
            return Err(anyhow!(
                "One bookmark prefix starts with another, which is prohibited: {:?}, {:?}",
                fp,
                sp
            ));
        }
    }

    Ok(())
}

/// Verify that two mpaths are not a prefix of each other
fn validate_mpath_prefixes(first_prefix: &MPath, second_prefix: &MPath) -> Result<()> {
    if first_prefix.is_prefix_of(second_prefix) {
        return Err(anyhow!(
            "{:?} is a prefix of {:?}, which is disallowed",
            first_prefix,
            second_prefix
        ));
    }
    if second_prefix.is_prefix_of(first_prefix) {
        return Err(anyhow!(
            "{:?} is a prefix of {:?}, which is disallowed",
            second_prefix,
            first_prefix
        ));
    }
    Ok(())
}

impl Convert for RawCommitSyncConfig {
    type Output = CommitSyncConfig;

    fn convert(self) -> Result<Self::Output> {
        let RawCommitSyncConfig {
            small_repos,
            common_pushrebase_bookmarks,
            large_repo_id,
            version_name,
            ..
        } = self;

        // Unfortunately, deserializer would not fail if there are
        // multiple small repos with the same repo ids: it would just
        // insert them into a hashmap, and result in error silencing.
        // Let's check this explicitly
        check_no_duplicate_small_repos(&small_repos)?;

        let small_repos = small_repos
            .into_iter()
            .map(|small_repo| {
                let repo_id = RepositoryId::new(small_repo.repoid);
                let small_repo = small_repo.convert()?;
                Ok((repo_id, small_repo))
            })
            .collect::<Result<HashMap<_, _>>>()?;

        let common_pushrebase_bookmarks = common_pushrebase_bookmarks
            .into_iter()
            .map(BookmarkName::new)
            .collect::<Result<Vec<_>>>()?;

        let large_repo_id = RepositoryId::new(large_repo_id);

        let version_name = version_name.unwrap_or_default();
        let commit_sync_config = CommitSyncConfig {
            large_repo_id,
            common_pushrebase_bookmarks,
            small_repos,
            version_name,
        };

        validate_commit_sync_config(&commit_sync_config)?;
        Ok(commit_sync_config)
    }
}

impl Convert for RawCommitSyncSmallRepoConfig {
    type Output = SmallRepoCommitSyncConfig;

    fn convert(self) -> Result<Self::Output> {
        let RawCommitSyncSmallRepoConfig {
            repoid: _,
            default_action,
            default_prefix,
            bookmark_prefix,
            mapping,
            direction,
            ..
        } = self;

        let default_action = match default_action.as_str() {
            "preserve" => DefaultSmallToLargeCommitSyncPathAction::Preserve,
            "prepend_prefix" => match default_prefix {
                Some(prefix_to_prepend) => {
                    let prefix_to_prepend = MPath::new(prefix_to_prepend)?;
                    DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(prefix_to_prepend)
                }
                None => {
                    return Err(anyhow!(
                        "default_prefix must be provided when default_action=\"prepend_prefix\""
                    ))
                }
            },
            other => return Err(anyhow!("unknown default_action: {:?}", other)),
        };

        let map = mapping
            .into_iter()
            .map(|(k, v)| Ok((MPath::new(k)?, MPath::new(v)?)))
            .collect::<Result<HashMap<_, _>>>()?;

        let bookmark_prefix = AsciiString::from_str(&bookmark_prefix)
            .map_err(|_| anyhow!("failed to parse ascii string from: {:?}", bookmark_prefix))?;

        let direction = match direction.as_str() {
            "large_to_small" => CommitSyncDirection::LargeToSmall,
            "small_to_large" => CommitSyncDirection::SmallToLarge,
            other => return Err(anyhow!("unknown commit sync direction: {:?}", other)),
        };

        Ok(SmallRepoCommitSyncConfig {
            default_action,
            map,
            bookmark_prefix,
            direction,
        })
    }
}
