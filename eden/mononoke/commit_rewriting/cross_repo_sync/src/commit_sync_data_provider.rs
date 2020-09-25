/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Error};
use bookmark_renaming::{get_bookmark_renamers, BookmarkRenamer, BookmarkRenamers};
use live_commit_sync_config::LiveCommitSyncConfig;
use metaconfig_types::{CommitSyncConfig, CommitSyncConfigVersion, CommitSyncDirection};
use mononoke_types::RepositoryId;
use movers::{get_movers, Mover, Movers};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub enum CommitSyncDataProvider {
    Live(Arc<dyn LiveCommitSyncConfig>),
    Test(HashMap<CommitSyncConfigVersion, SyncData>),
}

#[derive(Clone)]
pub struct SyncData {
    pub mover: Mover,
    pub reverse_mover: Mover,
    pub bookmark_renamer: BookmarkRenamer,
    pub reverse_bookmark_renamer: BookmarkRenamer,
}

impl CommitSyncDataProvider {
    pub fn get_mover(
        &self,
        version: &CommitSyncConfigVersion,
        source_repo_id: RepositoryId,
        target_repo_id: RepositoryId,
    ) -> Result<Mover, Error> {
        use CommitSyncDataProvider::*;

        match self {
            Live(live_commit_sync_config) => {
                let commit_sync_config = live_commit_sync_config
                    .get_commit_sync_config_by_version(source_repo_id, version)?;

                let Movers { mover, .. } =
                    get_movers_from_config(&commit_sync_config, source_repo_id, target_repo_id)?;
                Ok(mover)
            }
            Test(map) => {
                let sync_data = map
                    .get(version)
                    .ok_or_else(|| anyhow!("sync data not found for {}", version))?;
                Ok(sync_data.mover.clone())
            }
        }
    }

    pub fn get_reverse_mover(
        &self,
        version: &CommitSyncConfigVersion,
        source_repo_id: RepositoryId,
        target_repo_id: RepositoryId,
    ) -> Result<Mover, Error> {
        use CommitSyncDataProvider::*;

        match self {
            Live(live_commit_sync_config) => {
                let commit_sync_config = live_commit_sync_config
                    .get_commit_sync_config_by_version(source_repo_id, version)?;

                let Movers { reverse_mover, .. } =
                    get_movers_from_config(&commit_sync_config, source_repo_id, target_repo_id)?;
                Ok(reverse_mover)
            }
            Test(map) => {
                let sync_data = map
                    .get(version)
                    .ok_or_else(|| anyhow!("sync data not found for {}", version))?;
                Ok(sync_data.reverse_mover.clone())
            }
        }
    }

    pub fn get_bookmark_renamer(
        &self,
        version: &CommitSyncConfigVersion,
        source_repo_id: RepositoryId,
        target_repo_id: RepositoryId,
    ) -> Result<BookmarkRenamer, Error> {
        use CommitSyncDataProvider::*;

        match self {
            Live(live_commit_sync_config) => {
                let commit_sync_config = live_commit_sync_config
                    .get_commit_sync_config_by_version(source_repo_id, version)?;

                let BookmarkRenamers {
                    bookmark_renamer, ..
                } = get_bookmark_renamers_from_config(
                    &commit_sync_config,
                    source_repo_id,
                    target_repo_id,
                )?;
                Ok(bookmark_renamer)
            }
            Test(map) => {
                let sync_data = map
                    .get(version)
                    .ok_or_else(|| anyhow!("sync data not found for {}", version))?;
                Ok(sync_data.bookmark_renamer.clone())
            }
        }
    }

    pub fn get_reverse_bookmark_renamer(
        &self,
        version: &CommitSyncConfigVersion,
        source_repo_id: RepositoryId,
        target_repo_id: RepositoryId,
    ) -> Result<BookmarkRenamer, Error> {
        use CommitSyncDataProvider::*;

        match self {
            Live(live_commit_sync_config) => {
                let commit_sync_config = live_commit_sync_config
                    .get_commit_sync_config_by_version(source_repo_id, version)?;

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
            Test(map) => {
                let sync_data = map
                    .get(version)
                    .ok_or_else(|| anyhow!("sync data not found for {}", version))?;
                Ok(sync_data.reverse_bookmark_renamer.clone())
            }
        }
    }
}

fn get_movers_from_config(
    commit_sync_config: &CommitSyncConfig,
    source_repo_id: RepositoryId,
    target_repo_id: RepositoryId,
) -> Result<Movers, Error> {
    let (direction, small_repo_id) =
        get_direction_and_small_repo_id(commit_sync_config, source_repo_id, target_repo_id)?;
    get_movers(&commit_sync_config, small_repo_id, direction)
}

fn get_bookmark_renamers_from_config(
    commit_sync_config: &CommitSyncConfig,
    source_repo_id: RepositoryId,
    target_repo_id: RepositoryId,
) -> Result<BookmarkRenamers, Error> {
    let (direction, small_repo_id) =
        get_direction_and_small_repo_id(commit_sync_config, source_repo_id, target_repo_id)?;
    get_bookmark_renamers(commit_sync_config, small_repo_id, direction)
}

fn get_direction_and_small_repo_id(
    commit_sync_config: &CommitSyncConfig,
    source_repo_id: RepositoryId,
    target_repo_id: RepositoryId,
) -> Result<(CommitSyncDirection, RepositoryId), Error> {
    let small_repo_id = if commit_sync_config.large_repo_id == source_repo_id
        && commit_sync_config.small_repos.contains_key(&target_repo_id)
    {
        target_repo_id
    } else if commit_sync_config.large_repo_id == target_repo_id
        && commit_sync_config.small_repos.contains_key(&source_repo_id)
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
