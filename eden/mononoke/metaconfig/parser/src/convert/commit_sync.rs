/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use ascii::AsciiString;
use bookmarks_types::BookmarkKey;
use commitsync::CommonCommitSyncConfig as RawCommonCommitSyncConfig;
use itertools::Itertools;
use metaconfig_types::CommitSyncConfig;
use metaconfig_types::CommitSyncConfigVersion;
use metaconfig_types::CommonCommitSyncConfig;
use metaconfig_types::DefaultSmallToLargeCommitSyncPathAction;
use metaconfig_types::GitSubmodulesChangesAction;
use metaconfig_types::SmallRepoCommitSyncConfig;
use metaconfig_types::SmallRepoGitSubmoduleConfig;
use metaconfig_types::SmallRepoPermanentConfig;
use metaconfig_types::DEFAULT_GIT_SUBMODULE_METADATA_FILE_PREFIX;
use mononoke_types::hash::GitSha1;
use mononoke_types::NonRootMPath;
use mononoke_types::RepositoryId;
use repos::RawCommitSyncConfig;
use repos::RawCommitSyncSmallRepoConfig;
use repos::RawGitSubmodulesChangesAction;

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
///   mean potential bookmark name collisions.
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
        .values()
        .map(|sr| &sr.bookmark_prefix)
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
            .map(BookmarkKey::new)
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

impl Convert for RawGitSubmodulesChangesAction {
    type Output = GitSubmodulesChangesAction;
    fn convert(self) -> Result<Self::Output> {
        let converted = match self {
            RawGitSubmodulesChangesAction::KEEP => GitSubmodulesChangesAction::Keep,
            RawGitSubmodulesChangesAction::STRIP => GitSubmodulesChangesAction::Strip,
            RawGitSubmodulesChangesAction::EXPAND => GitSubmodulesChangesAction::Expand,
            RawGitSubmodulesChangesAction::UNKNOWN => GitSubmodulesChangesAction::default(),
            v => {
                return Err(anyhow!(
                    "Invalid value {} for enum GitSubmodulesChangesAction",
                    v
                ));
            }
        };
        Ok(converted)
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
            git_submodules_action,
            submodule_dependencies,
            submodule_metadata_file_prefix,
            dangling_submodule_pointers,
            ..
        } = self;

        let submodule_metadata_file_prefix = submodule_metadata_file_prefix
            .unwrap_or(DEFAULT_GIT_SUBMODULE_METADATA_FILE_PREFIX.to_string());

        let default_action = match default_action.as_str() {
            "preserve" => DefaultSmallToLargeCommitSyncPathAction::Preserve,
            "prepend_prefix" => match default_prefix {
                Some(prefix_to_prepend) => {
                    let prefix_to_prepend = NonRootMPath::new(prefix_to_prepend)?;
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
        let submodule_dependencies = submodule_dependencies
            .unwrap_or_default()
            .into_iter()
            .map(|(path, id)| Ok((NonRootMPath::new(path)?, RepositoryId::new(id))))
            .collect::<Result<HashMap<_, _>>>()
            .context("Failed to get small repo dependencies from config")?;

        let map = mapping
            .into_iter()
            .map(|(k, v)| Ok((NonRootMPath::new(k)?, NonRootMPath::new(v)?)))
            .collect::<Result<HashMap<_, _>>>()?;

        let git_submodules_action = match git_submodules_action {
            Some(git_submodules_action) => git_submodules_action.convert()?,
            None => GitSubmodulesChangesAction::default(),
        };
        let dangling_submodule_pointers = dangling_submodule_pointers
            .unwrap_or(Vec::new())
            .into_iter()
            .map(|git_commit_str| GitSha1::from_str(&git_commit_str))
            .collect::<Result<Vec<_>>>()?;

        Ok(SmallRepoCommitSyncConfig {
            default_action,
            map,
            submodule_config: SmallRepoGitSubmoduleConfig {
                git_submodules_action,
                submodule_dependencies,
                submodule_metadata_file_prefix,
                dangling_submodule_pointers,
            },
        })
    }
}

impl Convert for RawCommonCommitSyncConfig {
    type Output = CommonCommitSyncConfig;

    fn convert(self) -> Result<Self::Output> {
        let large_repo_id = RepositoryId::new(self.large_repo_id);
        let common_pushrebase_bookmarks: Result<Vec<BookmarkKey>> = self
            .common_pushrebase_bookmarks
            .into_iter()
            .map(BookmarkKey::new)
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
                let common_pushrebase_bookmarks_map = small_repo_config
                    .common_pushrebase_bookmarks_map
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(k, v)| Ok((BookmarkKey::from_str(&k)?, BookmarkKey::from_str(&v)?)))
                    .collect::<Result<_>>()?;

                let config = SmallRepoPermanentConfig {
                    bookmark_prefix,
                    common_pushrebase_bookmarks_map,
                };
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
