/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Error};
use bookmark_renaming::{get_bookmark_renamers, BookmarkRenamer, BookmarkRenamers};
use bookmarks::BookmarkName;
use context::CoreContext;
use futures::future::try_join;
use live_commit_sync_config::LiveCommitSyncConfig;
use metaconfig_types::{
    CommitSyncConfig, CommitSyncConfigVersion, CommitSyncDirection, CommonCommitSyncConfig,
};
use mononoke_types::RepositoryId;
use movers::{get_movers, Mover, Movers};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::types::{Source, Target};

#[derive(Clone)]
pub enum CommitSyncDataProvider {
    Live(Arc<dyn LiveCommitSyncConfig>),
    Test {
        current_version: Arc<Mutex<CommitSyncConfigVersion>>,
        map: HashMap<CommitSyncConfigVersion, SyncData>,
        source_repo_id: RepositoryId,
        target_repo_id: RepositoryId,
        common_pushrebase_bookmarks: Vec<BookmarkName>,
        bookmark_renamer: BookmarkRenamer,
        reverse_bookmark_renamer: BookmarkRenamer,
    },
}

#[derive(Clone)]
pub struct SyncData {
    pub mover: Mover,
    pub reverse_mover: Mover,
}

impl SyncData {
    fn reverse(self) -> Self {
        let Self {
            mover,
            reverse_mover,
        } = self;

        Self {
            mover: reverse_mover,
            reverse_mover: mover,
        }
    }
}

impl CommitSyncDataProvider {
    pub fn test_new(
        current_version: CommitSyncConfigVersion,
        source_repo_id: Source<RepositoryId>,
        target_repo_id: Target<RepositoryId>,
        map: HashMap<CommitSyncConfigVersion, SyncData>,
        common_pushrebase_bookmarks: Vec<BookmarkName>,
        bookmark_renamer: BookmarkRenamer,
        reverse_bookmark_renamer: BookmarkRenamer,
    ) -> Self {
        Self::Test {
            current_version: Arc::new(Mutex::new(current_version)),
            map,
            source_repo_id: source_repo_id.0,
            target_repo_id: target_repo_id.0,
            common_pushrebase_bookmarks,
            bookmark_renamer,
            reverse_bookmark_renamer,
        }
    }

    fn get_sync_data(
        &self,
        version: &CommitSyncConfigVersion,
        source_repo_id: RepositoryId,
        target_repo_id: RepositoryId,
    ) -> Result<SyncData, Error> {
        match self {
            Self::Live(_) => Err(anyhow!(
                "calling CommitSyncDataProvide::get_sync_data on Live variant is not supported"
            )),
            Self::Test {
                map,
                source_repo_id: stored_source_repo_id,
                target_repo_id: stored_target_repo_id,
                ..
            } => {
                let sync_data = map
                    .get(version)
                    .ok_or_else(|| anyhow!("sync data not found for {}", version))?;

                if source_repo_id == *stored_source_repo_id
                    && target_repo_id == *stored_target_repo_id
                {
                    Ok(sync_data.clone())
                } else if source_repo_id == *stored_target_repo_id
                    && target_repo_id == *stored_source_repo_id
                {
                    Ok(sync_data.clone().reverse())
                } else {
                    Err(anyhow!(
                        "repos unknown to CommitSyncDataProvider: {}->{}",
                        source_repo_id,
                        target_repo_id
                    ))
                }
            }
        }
    }

    pub async fn get_mover(
        &self,
        version: &CommitSyncConfigVersion,
        source_repo_id: RepositoryId,
        target_repo_id: RepositoryId,
    ) -> Result<Mover, Error> {
        use CommitSyncDataProvider::*;

        match self {
            Live(live_commit_sync_config) => {
                let commit_sync_config = live_commit_sync_config
                    .get_commit_sync_config_by_version(source_repo_id, version);
                let common_config = live_commit_sync_config.get_common_config(source_repo_id);

                let (commit_sync_config, common_config) =
                    try_join(commit_sync_config, common_config).await?;

                let Movers { mover, .. } = get_movers_from_config(
                    &common_config,
                    &commit_sync_config,
                    source_repo_id,
                    target_repo_id,
                )?;
                Ok(mover)
            }
            Test { .. } => {
                let sync_data = self.get_sync_data(version, source_repo_id, target_repo_id)?;
                Ok(sync_data.mover)
            }
        }
    }

    pub async fn get_reverse_mover(
        &self,
        version: &CommitSyncConfigVersion,
        source_repo_id: RepositoryId,
        target_repo_id: RepositoryId,
    ) -> Result<Mover, Error> {
        use CommitSyncDataProvider::*;

        match self {
            Live(live_commit_sync_config) => {
                let commit_sync_config = live_commit_sync_config
                    .get_commit_sync_config_by_version(source_repo_id, version);
                let common_config = live_commit_sync_config.get_common_config(source_repo_id);

                let (commit_sync_config, common_config) =
                    try_join(commit_sync_config, common_config).await?;

                let Movers { reverse_mover, .. } = get_movers_from_config(
                    &common_config,
                    &commit_sync_config,
                    source_repo_id,
                    target_repo_id,
                )?;
                Ok(reverse_mover)
            }
            Test { .. } => {
                let sync_data = self.get_sync_data(version, source_repo_id, target_repo_id)?;
                Ok(sync_data.reverse_mover)
            }
        }
    }

    pub async fn get_bookmark_renamer(
        &self,
        source_repo_id: RepositoryId,
        target_repo_id: RepositoryId,
    ) -> Result<BookmarkRenamer, Error> {
        use CommitSyncDataProvider::*;

        match self {
            Live(live_commit_sync_config) => {
                let commit_sync_config = live_commit_sync_config
                    .get_common_config(source_repo_id)
                    .await?;

                let BookmarkRenamers {
                    bookmark_renamer, ..
                } = get_bookmark_renamers_from_config(
                    &commit_sync_config,
                    source_repo_id,
                    target_repo_id,
                )?;
                Ok(bookmark_renamer)
            }
            Test {
                bookmark_renamer, ..
            } => Ok(bookmark_renamer.clone()),
        }
    }

    pub async fn get_reverse_bookmark_renamer(
        &self,
        source_repo_id: RepositoryId,
        target_repo_id: RepositoryId,
    ) -> Result<BookmarkRenamer, Error> {
        use CommitSyncDataProvider::*;

        match self {
            Live(live_commit_sync_config) => {
                let commit_sync_config = live_commit_sync_config
                    .get_common_config(source_repo_id)
                    .await?;

                let BookmarkRenamers {
                    reverse_bookmark_renamer,
                    ..
                } = get_bookmark_renamers_from_config(
                    &commit_sync_config,
                    source_repo_id,
                    target_repo_id,
                )?;
                Ok(reverse_bookmark_renamer)
            }
            Test {
                reverse_bookmark_renamer,
                ..
            } => Ok(reverse_bookmark_renamer.clone()),
        }
    }

    pub async fn get_current_version(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
    ) -> Result<CommitSyncConfigVersion, Error> {
        use CommitSyncDataProvider::*;

        match self {
            Live(live_commit_sync_config) => {
                live_commit_sync_config
                    .get_current_commit_sync_config_version(ctx, repo_id)
                    .await
            }
            Test {
                current_version, ..
            } => Ok(current_version.lock().unwrap().clone()),
        }
    }

    pub async fn version_exists(
        &self,
        repo_id: RepositoryId,
        version: &CommitSyncConfigVersion,
    ) -> Result<bool, Error> {
        match self {
            Self::Live(live_commit_sync_config) => {
                let versions = live_commit_sync_config
                    .get_all_commit_sync_config_versions(repo_id)
                    .await?;
                Ok(versions.contains_key(version))
            }
            Self::Test { map, .. } => Ok(map.contains_key(version)),
        }
    }

    pub async fn get_common_pushrebase_bookmarks(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
    ) -> Result<Vec<BookmarkName>, Error> {
        use CommitSyncDataProvider::*;

        match self {
            Live(live_commit_sync_config) => {
                let commit_sync_config = live_commit_sync_config
                    .get_current_commit_sync_config(ctx, repo_id)
                    .await?;
                Ok(commit_sync_config.common_pushrebase_bookmarks)
            }
            Test {
                common_pushrebase_bookmarks,
                ..
            } => Ok(common_pushrebase_bookmarks.clone()),
        }
    }
}

fn get_movers_from_config(
    common_config: &CommonCommitSyncConfig,
    commit_sync_config: &CommitSyncConfig,
    source_repo_id: RepositoryId,
    target_repo_id: RepositoryId,
) -> Result<Movers, Error> {
    let (direction, small_repo_id) =
        get_direction_and_small_repo_id(common_config, source_repo_id, target_repo_id)?;
    get_movers(&commit_sync_config, small_repo_id, direction)
}

fn get_bookmark_renamers_from_config(
    common_config: &CommonCommitSyncConfig,
    source_repo_id: RepositoryId,
    target_repo_id: RepositoryId,
) -> Result<BookmarkRenamers, Error> {
    let (direction, small_repo_id) =
        get_direction_and_small_repo_id(common_config, source_repo_id, target_repo_id)?;
    get_bookmark_renamers(common_config, small_repo_id, direction)
}

fn get_direction_and_small_repo_id(
    common_config: &CommonCommitSyncConfig,
    source_repo_id: RepositoryId,
    target_repo_id: RepositoryId,
) -> Result<(CommitSyncDirection, RepositoryId), Error> {
    let small_repo_id = if common_config.large_repo_id == source_repo_id
        && common_config.small_repos.contains_key(&target_repo_id)
    {
        target_repo_id
    } else if common_config.large_repo_id == target_repo_id
        && common_config.small_repos.contains_key(&source_repo_id)
    {
        source_repo_id
    } else {
        return Err(anyhow!(
            "CommitSyncMapping incompatible with source repo {:?} and target repo {:?}",
            source_repo_id,
            target_repo_id,
        ));
    };

    let direction = if source_repo_id == small_repo_id {
        CommitSyncDirection::SmallToLarge
    } else {
        CommitSyncDirection::LargeToSmall
    };

    Ok((direction, small_repo_id))
}
