/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobrepo::BlobRepo;
use metaconfig_types::CommitSyncConfig;
use mononoke_types::RepositoryId;
use std::fmt;
use synced_commit_mapping::SyncedCommitMapping;

use crate::{CommitSyncRepos, CommitSyncer};

/// An auxillary struct to hold data necessary for `CommitSyncer` instantiation
#[derive(Clone)]
pub struct CommitSyncerArgs<T> {
    source_repo: BlobRepo,
    target_repo: BlobRepo,
    mapping: T,
}

impl<T: SyncedCommitMapping + Clone + 'static> CommitSyncerArgs<T> {
    pub fn new(source_repo: BlobRepo, target_repo: BlobRepo, mapping: T) -> Self {
        Self {
            source_repo,
            target_repo,
            mapping,
        }
    }

    pub fn get_source_repo(&self) -> &BlobRepo {
        &self.source_repo
    }

    pub fn get_target_repo(&self) -> &BlobRepo {
        &self.target_repo
    }

    pub fn get_target_repo_id(&self) -> RepositoryId {
        self.target_repo.get_repoid()
    }

    pub fn get_source_repo_id(&self) -> RepositoryId {
        self.source_repo.get_repoid()
    }

    pub fn try_into_commit_syncer(
        self,
        commit_sync_config: &CommitSyncConfig,
    ) -> Result<CommitSyncer<T>, Error> {
        let Self {
            source_repo,
            target_repo,
            mapping,
        } = self;

        let commit_sync_repos = CommitSyncRepos::new(source_repo, target_repo, commit_sync_config)?;
        Ok(CommitSyncer::new(mapping, commit_sync_repos))
    }
}

impl<M> fmt::Debug for CommitSyncerArgs<M>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let source_repo_id = self.source_repo.get_repoid();
        let target_repo_id = self.target_repo.get_repoid();
        write!(
            f,
            "CommitSyncerArgs{{{}->{}}}",
            source_repo_id, target_repo_id
        )
    }
}
