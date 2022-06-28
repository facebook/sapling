/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::Result;
use ascii::AsciiString;
use bookmarks_types::BookmarkName;
use commitsync::types::CommonCommitSyncConfig as RawCommonCommitSyncConfig;
use itertools::Itertools;
use metaconfig_types::CommitSyncConfig;
use metaconfig_types::CommitSyncConfigVersion;
use metaconfig_types::CommonCommitSyncConfig;
use metaconfig_types::DefaultSmallToLargeCommitSyncPathAction;
use metaconfig_types::SmallRepoCommitSyncConfig;
use metaconfig_types::SmallRepoPermanentConfig;
use mononoke_types::MPath;
use mononoke_types::RepositoryId;
use repos::RawCommitSyncConfig;
use repos::RawCommitSyncSmallRepoConfig;

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

    Ok(())
}

fn validate_common_commit_sync_config(
    common_commit_sync_config: &CommonCommitSyncConfig,
) -> Result<()> {
    if common_commit_sync_config
        .small_repos
        .contains_key(&common_commit_sync_config.large_repo_id)
    {
        return Err(anyhow!(
            "Large repo ({}) is one of the small repos too",
            common_commit_sync_config.large_repo_id
        ));
    }

    let bookmark_prefixes: Vec<&AsciiString> = common_commit_sync_config
        .small_repos
        .iter()
        .map(|(_, sr)| &sr.bookmark_prefix)
        .collect();

    // No two small repos can have the bookmark prefix as prefix of another
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

        let version_name = CommitSyncConfigVersion(version_name.unwrap_or_default());
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
            mapping,
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
                    ));
                }
            },
            other => return Err(anyhow!("unknown default_action: {:?}", other)),
        };

        let map = mapping
            .into_iter()
            .map(|(k, v)| Ok((MPath::new(k)?, MPath::new(v)?)))
            .collect::<Result<HashMap<_, _>>>()?;

        Ok(SmallRepoCommitSyncConfig {
            default_action,
            map,
        })
    }
}

impl Convert for RawCommonCommitSyncConfig {
    type Output = CommonCommitSyncConfig;

    fn convert(self) -> Result<Self::Output> {
        let large_repo_id = RepositoryId::new(self.large_repo_id);
        let common_pushrebase_bookmarks: Result<Vec<BookmarkName>> = self
            .common_pushrebase_bookmarks
            .into_iter()
            .map(BookmarkName::new)
            .try_collect();
        let common_pushrebase_bookmarks = common_pushrebase_bookmarks?;
        let small_repos: HashMap<_, _> = self
            .small_repos
            .into_iter()
            .map(|(repo_id, small_repo_config)| {
                let repo_id = RepositoryId::new(repo_id);
                let bookmark_prefix = AsciiString::from_str(&small_repo_config.bookmark_prefix)
                    .map_err(|_| {
                        anyhow!(
                            "failed to parse ascii string from: {:?}",
                            small_repo_config.bookmark_prefix
                        )
                    })?;

                let config = SmallRepoPermanentConfig { bookmark_prefix };
                Ok((repo_id, config))
            })
            .collect::<Result<_>>()?;

        let config = CommonCommitSyncConfig {
            large_repo_id,
            common_pushrebase_bookmarks,
            small_repos,
        };

        validate_common_commit_sync_config(&config)?;

        Ok(config)
    }
}
