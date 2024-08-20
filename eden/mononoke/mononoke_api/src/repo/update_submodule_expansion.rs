/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

use anyhow::anyhow;
use anyhow::Context;
use maplit::btreemap;
use metaconfig_types::DefaultSmallToLargeCommitSyncPathAction;
use mononoke_types::hash::GitSha1;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime as MononokeDateTime;
use mononoke_types::MPath;
use mononoke_types::NonRootMPath;
use mononoke_types::RepositoryId;
use repo_cross_repo::RepoCrossRepoRef;
use repo_identity::RepoIdentityRef;
use synced_commit_mapping::WorkingCopyEquivalence;

use super::RepoContextBuilder;
use crate::changeset::ChangesetContext;
use crate::errors::MononokeError;
use crate::file::FileType;
use crate::repo::create_changeset::CreateInfo;
use crate::repo::RepoContext;
use crate::CreateChange;
use crate::CreateChangeFile;
use crate::CreateChangeFileContents;
use crate::MononokeRepo;
use crate::XRepoLookupExactBehaviour;
use crate::XRepoLookupSyncBehaviour;

pub enum SubmoduleExpansionUpdate {
    /// Expand a new submodule commit
    UpdateCommit(GitSha1),
    /// Delete the submodule
    Delete,
}

pub struct SubmoduleExpansionUpdateCommitInfo {
    pub message: Option<String>,
    pub author: Option<String>,
    pub author_date: Option<MononokeDateTime>,
}

impl<R: MononokeRepo> RepoContext<R> {
    /// Create a commit in the large repo updating a submodule expansion.
    ///
    /// This is done by creating a commit in the small repo with a single
    /// FileChange of type GitSubmodule and syncing this commit to the large
    /// repo.
    pub async fn update_submodule_expansion(
        &self,
        base_changeset_id: ChangesetId,
        submodule_expansion_path: NonRootMPath,
        submodule_expansion_update: SubmoduleExpansionUpdate,
        commit_info: SubmoduleExpansionUpdateCommitInfo,
    ) -> Result<ChangesetContext<R>, MononokeError> {
        let ctx = &self.ctx;

        // Get the small repo to which the submodule expansion belongs and the
        // adjusted submodule path (removing small repo prefix)
        let (small_repo_id, submodule_expansion_path) = self
            .get_small_repo_id(base_changeset_id, &submodule_expansion_path)
            .await?;

        let small_repo = self
            .repos
            .get_by_id(small_repo_id.id())
            .ok_or_else(|| anyhow!("Failed to open small repo with id {small_repo_id}"))?;
        let small_repo_ctx = RepoContextBuilder::new(ctx.clone(), small_repo, self.repos.clone())
            .await?
            .build()
            .await?;

        // Create a commit in the small repo with a GitSubmodule file change
        // or deleting the submodule, based on `submodule_expansion_update`
        let small_repo_cs_ctx = self
            .create_small_repo_commit(
                &small_repo_ctx,
                base_changeset_id,
                submodule_expansion_path,
                submodule_expansion_update,
                commit_info,
            )
            .await?;

        // Sync this commit from small to large repo, expanding the submodule
        // commit or deleting the expansion and the submodule metadata file, if
        // the update operation was Delete.
        let mb_large_repo_cs_ctx = small_repo_ctx
            .xrepo_commit_lookup(
                self,
                small_repo_cs_ctx.id(),
                None,
                XRepoLookupSyncBehaviour::SyncIfAbsent,
                XRepoLookupExactBehaviour::OnlyExactMapping,
            )
            .await
            .context("Failed to sync small repo commit updating submodule to large repo")?;

        match mb_large_repo_cs_ctx {
            Some(cs_ctx) => Ok(cs_ctx),
            None => Err(anyhow!(
                "Small repo commit updating submodule wasn't synced to large repo"
            )
            .into()),
        }
    }

    /// Creates a commit in the **small repo** with a single file change of type
    /// GitSubmodule, updating the submodule pointer to the provided submodule
    /// commit.
    ///
    /// This commit will then be forward synced to the large repo, updating
    /// its expansion.
    pub async fn create_small_repo_commit(
        &self,
        small_repo_ctx: &RepoContext<R>,
        base_changeset_id: ChangesetId,
        submodule_expansion_path: NonRootMPath,
        submodule_expansion_update: SubmoduleExpansionUpdate,
        commit_info: SubmoduleExpansionUpdateCommitInfo,
    ) -> Result<ChangesetContext<R>, MononokeError> {
        let large_repo_ctx = self; // For readability

        let synced_commit_mapping = large_repo_ctx
            .repo()
            .repo_cross_repo()
            .synced_commit_mapping()
            .clone();
        let large_repo_id = large_repo_ctx.repo().repo_identity().id();

        // Get small repo changeset with working copy equivalence to the provided
        // large repo base changeset. This will be the parent of the small repo
        // changeset modifying the submodule expansion.
        let small_repo_base_commit_wc = synced_commit_mapping
            .get_equivalent_working_copy(
                &self.ctx,
                large_repo_id,
                base_changeset_id,
                small_repo_ctx.repo().repo_identity().id(),
            )
            .await?
            .ok_or(anyhow!(
                "Couldn't find small repo commit that's working copy equivalent to base commit {base_changeset_id}")
            )?;

        let small_repo_base_cs = match small_repo_base_commit_wc {
            WorkingCopyEquivalence::WorkingCopy(cs_id, _) => cs_id,
            WorkingCopyEquivalence::NoWorkingCopy(_) => {
                return Err(anyhow!(
                    "No working copy equivalent small repo changeset found for large repo changeset {base_changeset_id}"
                ).into());
            }
        };

        let default_commit_msg = match &submodule_expansion_update {
            SubmoduleExpansionUpdate::UpdateCommit(git_commit) => format!(
                "Update submodule {submodule_expansion_path} in {0} to {1}",
                small_repo_ctx.name(),
                git_commit,
            ),
            SubmoduleExpansionUpdate::Delete => format!(
                "Delete submodule {submodule_expansion_path} in {0}",
                small_repo_ctx.name()
            ),
        };

        // If not author_date was provided, default to the current time
        let author_date = commit_info.author_date.unwrap_or(MononokeDateTime::now());

        let create_info = CreateInfo {
            message: commit_info.message.unwrap_or(default_commit_msg),
            author: commit_info.author.unwrap_or("svcscm".to_string()),
            author_date: author_date.into(),
            committer: None,
            committer_date: None,
            extra: btreemap! {},
            git_extra_headers: None,
        };

        let parents = vec![small_repo_base_cs];

        let create_change = match submodule_expansion_update {
            SubmoduleExpansionUpdate::UpdateCommit(git_sha1) => {
                let oid = git_sha1.to_object_id()?;
                let git_commit_id = oid.as_slice().to_vec();
                let create_change_file = CreateChangeFile {
                    contents: CreateChangeFileContents::New {
                        bytes: git_commit_id.into(),
                    },
                    file_type: FileType::GitSubmodule,
                    git_lfs: None,
                };
                CreateChange::Tracked(create_change_file, None)
            }
            SubmoduleExpansionUpdate::Delete => CreateChange::Deletion,
        };

        let changes: BTreeMap<MPath, CreateChange> = btreemap! {
            submodule_expansion_path.into() => create_change
        };

        let (_, new_commit) = small_repo_ctx
            .create_changeset(parents, create_info, changes, None)
            .await?;
        Ok(new_commit)
    }

    /// Get the id of the small repo where the submodule expansion is
    pub async fn get_small_repo_id(
        &self,
        base_changeset_id: ChangesetId,
        submodule_expansion_path: &NonRootMPath,
    ) -> Result<(RepositoryId, NonRootMPath), MononokeError> {
        let large_repo_ctx = self; // For readability

        let synced_commit_mapping = large_repo_ctx
            .repo()
            .repo_cross_repo()
            .synced_commit_mapping()
            .clone();
        let large_repo_id = large_repo_ctx.repo().repo_identity().id();

        // Get the commit sync mapping from the provided large repo base commit
        let commit_sync_version = synced_commit_mapping
            .get_large_repo_commit_version(&self.ctx, large_repo_id, base_changeset_id)
            .await
            .context("Failed to find large repo commit sync config version for base commit")?
            .ok_or(anyhow!(
                "No large repo commit sync config version found for base commit"
            ))?;
        let live_commit_sync_config = large_repo_ctx.live_commit_sync_config();

        // Get the large and small repo sync configs from the mapping
        let large_repo_sync_config = live_commit_sync_config
            .get_commit_sync_config_by_version_if_exists(large_repo_id, &commit_sync_version)
            .await
            .with_context(|| {
                anyhow!("Failed to fetch commit sync config version {commit_sync_version}")
            })?
            .ok_or_else(|| anyhow!("Commit sync config version {commit_sync_version} not found"))?;
        let small_repo_configs = large_repo_sync_config.small_repos;

        // Iterate over all small repo configs and find the one that is a prefix
        // of the submodule expansion path. This is the small repo being modified.
        let mut small_repo_ids_and_adj_paths =
            small_repo_configs
                .into_iter()
                .filter_map(|(repo_id, small_repo_cfg)| {
                    let small_repo_path = match &small_repo_cfg.default_action {
                        DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(prefix) => prefix,
                        DefaultSmallToLargeCommitSyncPathAction::Preserve => {
                            return None;
                        }
                    };

                    submodule_expansion_path
                        .remove_prefix_component(small_repo_path)
                        .map(|sm_path| (repo_id, sm_path))
                });

        let (small_repo_id, adjusted_sm_exp_path) = small_repo_ids_and_adj_paths.next().ok_or(
            anyhow!("No small repo being modified under path {submodule_expansion_path}"),
        )?;

        // Make sure that the submodule expansion path provided is under only
        // one small repo
        if let Some((small_repo_id_clash, _)) = small_repo_ids_and_adj_paths.next() {
            return Err(anyhow!(
                "Multiple small repos being modified under path {submodule_expansion_path}: {small_repo_id} and {small_repo_id_clash}"
            ).into());
        }

        Ok((small_repo_id, adjusted_sm_exp_path))
    }
}
