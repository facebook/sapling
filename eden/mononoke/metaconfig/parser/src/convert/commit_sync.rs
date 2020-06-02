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
use metaconfig_types::{
    CommitSyncConfig, CommitSyncDirection, DefaultSmallToLargeCommitSyncPathAction,
    SmallRepoCommitSyncConfig,
};
use mononoke_types::{MPath, RepositoryId};
use repos::{RawCommitSyncConfig, RawCommitSyncSmallRepoConfig};

use crate::convert::Convert;

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

        Ok(CommitSyncConfig {
            large_repo_id,
            common_pushrebase_bookmarks,
            small_repos,
            version_name,
        })
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
