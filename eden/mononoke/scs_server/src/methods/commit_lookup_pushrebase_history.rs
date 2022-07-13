/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::iter::once;
use std::sync::Arc;

use context::CoreContext;
use metaconfig_types::CommonCommitSyncConfig;
use mononoke_api::Mononoke;
use mononoke_api::RepoContext;
use mononoke_types::ChangesetId;
use phases::PhasesRef;
use source_control as thrift;
use synced_commit_mapping::SyncedCommitSourceRepo;

use crate::errors;
use crate::source_control_impl::SourceControlServiceImpl;

#[derive(Debug, Clone)]
struct RepoChangeset(String, ChangesetId);

impl RepoChangeset {
    fn into_thrift(self) -> thrift::CommitSpecifier {
        let RepoChangeset(name, cs_id) = self;
        thrift::CommitSpecifier {
            repo: thrift::RepoSpecifier {
                name,
                ..Default::default()
            },
            id: thrift::CommitId::bonsai(cs_id.as_ref().to_vec()),
            ..Default::default()
        }
    }
}

// Struct that represents current pushrebase history. Provides a simple
// interface which allows extending the history by looking into different
// mappings.
struct RepoChangesetsPushrebaseHistory {
    ctx: CoreContext,
    mononoke: Arc<Mononoke>,
    head: RepoChangeset,
    changesets: Vec<RepoChangeset>,
}

impl RepoChangesetsPushrebaseHistory {
    fn new(
        ctx: CoreContext,
        mononoke: Arc<Mononoke>,
        repo_name: String,
        changeset: ChangesetId,
    ) -> Self {
        Self {
            ctx,
            mononoke,
            head: RepoChangeset(repo_name, changeset),
            changesets: vec![],
        }
    }

    fn last(&self) -> RepoChangeset {
        match self.changesets.last() {
            Some(rc) => rc.clone(),
            None => self.head.clone(),
        }
    }

    async fn repo(&self, repo_name: &String) -> Result<RepoContext, errors::ServiceError> {
        let repo = self
            .mononoke
            .repo(self.ctx.clone(), repo_name)
            .await?
            .ok_or_else(|| errors::repo_not_found(repo_name.clone()))?
            .build()
            .await?;
        Ok(repo)
    }

    async fn ensure_head_is_public(&self) -> Result<(), errors::ServiceError> {
        let RepoChangeset(repo_name, bcs_id) = &self.head;
        let repo = self.repo(repo_name).await?;
        let is_public = repo
            .blob_repo()
            .phases()
            .get_public(&self.ctx, vec![*bcs_id], true /* ephemeral_derive */)
            .await
            .map_err(errors::internal_error)?
            .contains(bcs_id);
        if !is_public {
            return Err(errors::invalid_request(format!(
                "changeset {} is not public, and only public commits could be pushrebased",
                bcs_id,
            ))
            .into());
        }
        Ok(())
    }

    async fn try_traverse_pushrebase(&mut self) -> Result<bool, errors::ServiceError> {
        let RepoChangeset(repo_name, bcs_id) = self.last();
        let repo = self.repo(&repo_name).await?;
        let bcs_ids = repo
            .blob_repo()
            .pushrebase_mutation_mapping()
            .get_prepushrebase_ids(&self.ctx, bcs_id)
            .await
            .map_err(errors::internal_error)?;
        let mut iter = bcs_ids.iter();
        match (iter.next(), iter.next()) {
            (None, _) => Ok(false),
            (Some(bcs_id), None) => {
                self.changesets
                    .push(RepoChangeset(repo_name.clone(), *bcs_id));
                Ok(true)
            }
            (Some(_), Some(_)) => Err(errors::internal_error(format!(
                "pushrebase mapping is ambiguous in repo {} for {}: {:?} (expected only one)",
                repo_name, bcs_id, bcs_ids,
            ))
            .into()),
        }
    }

    async fn try_traverse_commit_sync(&mut self) -> Result<bool, errors::ServiceError> {
        let RepoChangeset(repo_name, bcs_id) = self.last();
        let repo = self.repo(&repo_name).await?;

        let maybe_common_commit_sync_config = repo
            .live_commit_sync_config()
            .get_common_config_if_exists(repo.repoid())
            .map_err(errors::internal_error)?;

        if let Some(config) = maybe_common_commit_sync_config {
            self.try_traverse_commit_sync_inner(repo, bcs_id, config)
                .await
        } else {
            Ok(false)
        }
    }

    // Helper function that wraps traversing logic from try_traverse_commit_sync.
    // Only needed for better code readability
    async fn try_traverse_commit_sync_inner(
        &mut self,
        repo: RepoContext,
        bcs_id: ChangesetId,
        config: CommonCommitSyncConfig,
    ) -> Result<bool, errors::ServiceError> {
        let mut synced_changesets = vec![];
        let (target_repo_ids, expected_sync_origin) = if config.large_repo_id == repo.repoid() {
            (
                config.small_repos.keys().copied().collect(),
                SyncedCommitSourceRepo::Small,
            )
        } else {
            (vec![config.large_repo_id], SyncedCommitSourceRepo::Large)
        };

        for target_repo_id in target_repo_ids.into_iter() {
            let entries = repo
                .synced_commit_mapping()
                .get(&self.ctx, repo.repoid(), bcs_id, target_repo_id)
                .await
                .map_err(errors::internal_error)?;
            if let Some(target_repo_name) = self.mononoke.repo_name_from_id(target_repo_id) {
                synced_changesets.extend(entries.into_iter().filter_map(
                    |(cs, _, maybe_source_repo)| {
                        let traverse = match maybe_source_repo {
                            // source_repo information can be absent e.g. for old commits but
                            // let's still traverse the mapping because in most cases we will
                            // get the correct result.
                            None => true,
                            Some(source_repo) if source_repo == expected_sync_origin => true,
                            _ => false,
                        };
                        if traverse {
                            Some(RepoChangeset(target_repo_name.clone(), cs))
                        } else {
                            None
                        }
                    },
                ));
            }
        }

        let mut iter = synced_changesets.iter();

        match (iter.next(), iter.next()) {
            (None, _) => Ok(false),
            (Some(rc), None) => {
                self.changesets.push(rc.clone());
                Ok(true)
            }
            (Some(_), Some(_)) => Err(errors::internal_error(format!(
                "commit sync mapping is ambiguous in repo {} for {}: {:?} (expected only one)",
                repo.name(),
                bcs_id,
                synced_changesets,
            ))
            .into()),
        }
    }

    fn into_thrift(self) -> thrift::CommitLookupPushrebaseHistoryResponse {
        let origin = self.last().into_thrift();
        let history: Vec<_> = once(self.head)
            .chain(self.changesets.into_iter())
            .map(|rc| rc.into_thrift())
            .collect();
        thrift::CommitLookupPushrebaseHistoryResponse {
            history,
            origin,
            ..Default::default()
        }
    }
}

impl SourceControlServiceImpl {
    // Look up commit history over Pushrebase mutations
    pub(crate) async fn commit_lookup_pushrebase_history(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        _params: thrift::CommitLookupPushrebaseHistoryParams,
    ) -> Result<thrift::CommitLookupPushrebaseHistoryResponse, errors::ServiceError> {
        let (repo, changeset) = self.repo_changeset(ctx.clone(), &commit).await?;
        let mut history = RepoChangesetsPushrebaseHistory::new(
            ctx,
            self.mononoke.clone(),
            repo.name().to_string(),
            changeset.id(),
        );

        // Ensure that commit is public
        history.ensure_head_is_public().await?;

        let mut pushrebased = false;
        if history.try_traverse_pushrebase().await? {
            pushrebased = true;
        } else if history.try_traverse_commit_sync().await? {
            pushrebased = history.try_traverse_pushrebase().await?;
        }

        if pushrebased {
            history.try_traverse_commit_sync().await?;
        }

        Ok(history.into_thrift())
    }
}
