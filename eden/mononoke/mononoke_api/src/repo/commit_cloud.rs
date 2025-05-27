/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use blobrepo_hg::BlobRepoHg;
use bonsai_git_mapping::BonsaiGitMappingArc;
use bonsai_hg_mapping::BonsaiHgMappingArc;
use borrowed::borrowed;
use cloned::cloned;
use commit_cloud::CommitCloudRef;
use commit_cloud::Phase;
use commit_cloud::ctx::CommitCloudContext;
use commit_cloud::utils::get_bonsai_from_cloud_ids;
use commit_cloud_types::ClientInfo;
use commit_cloud_types::HistoricalVersion;
use commit_cloud_types::LocalBookmarksMap;
use commit_cloud_types::ReferencesData;
use commit_cloud_types::RemoteBookmarksMap;
use commit_cloud_types::SmartlogData;
use commit_cloud_types::SmartlogFilter;
use commit_cloud_types::SmartlogFlag;
use commit_cloud_types::SmartlogNode;
use commit_cloud_types::UpdateReferencesParams;
use commit_cloud_types::WorkspaceData;
use commit_cloud_types::WorkspaceSharingData;
use commit_cloud_types::changeset::CloudChangesetId;
use commit_graph::CommitGraphRef;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use futures_util::future::try_join_all;
use mononoke_types::ChangesetId;
use phases::PhasesRef;

use crate::ChangesetContext;
use crate::ChangesetSpecifier;
use crate::MononokeError;
use crate::MononokeRepo;
use crate::RepoContext;
impl<R: MononokeRepo> RepoContext<R> {
    pub async fn cloud_workspace(
        &self,
        workspace: &str,
        reponame: &str,
    ) -> Result<WorkspaceData, MononokeError> {
        let mut cc_ctx = CommitCloudContext::new(workspace, reponame)?;
        let authz = self.authorization_context();
        authz
            .require_commitcloud_operation(self.ctx(), self.repo(), &mut cc_ctx, "read")
            .await?;
        Ok(self.repo().commit_cloud().get_workspace(&cc_ctx).await?)
    }

    pub async fn cloud_workspaces(
        &self,
        prefix: &str,
        reponame: &str,
    ) -> Result<Vec<WorkspaceData>, MononokeError> {
        Ok(self
            .repo()
            .commit_cloud()
            .get_workspaces(prefix, reponame)
            .await?)
    }

    pub async fn cloud_references(
        &self,
        workspace: &str,
        reponame: &str,
        version: u64,
        _client_info: Option<ClientInfo>,
    ) -> Result<ReferencesData, MononokeError> {
        let mut cc_ctx = CommitCloudContext::new(workspace, reponame)?;
        let authz = self.authorization_context();
        authz
            .require_commitcloud_operation(self.ctx(), self.repo(), &mut cc_ctx, "read")
            .await?;
        Ok(self
            .repo()
            .commit_cloud()
            .get_references(&cc_ctx, version)
            .await?)
    }

    pub async fn cloud_update_references(
        &self,
        params: &UpdateReferencesParams,
    ) -> Result<ReferencesData, MononokeError> {
        let mut cc_ctx = CommitCloudContext::new(&params.workspace, &params.reponame)?;
        if params.version == 0 {
            cc_ctx.check_workspace_name()?;
        }

        let authz = self.authorization_context();
        authz
            .require_commitcloud_operation(self.ctx(), self.repo(), &mut cc_ctx, "write")
            .await?;

        Ok(self
            .repo()
            .commit_cloud()
            .update_references(&cc_ctx, params)
            .await?)
    }

    pub async fn cloud_smartlog(
        &self,
        workspace: &str,
        reponame: &str,
        flags: &[SmartlogFlag],
    ) -> Result<SmartlogData, MononokeError> {
        let cc_ctx = CommitCloudContext::new(workspace, reponame)?;
        let raw_data = self
            .repo()
            .commit_cloud()
            .get_smartlog_raw_info(&cc_ctx)
            .await?;
        let hg_ids = raw_data.collapse_into_vec(flags);

        let nodes = self
            .form_smartlog_with_info(
                cc_ctx,
                hg_ids,
                raw_data.local_bookmarks,
                raw_data.remote_bookmarks,
                flags,
            )
            .await?;

        Ok(SmartlogData {
            nodes,
            version: None,
            timestamp: None,
        })
    }

    async fn form_smartlog_with_info(
        &self,
        cc_ctx: CommitCloudContext,
        c_ids: Vec<CloudChangesetId>,
        local_bookmarks: LocalBookmarksMap,
        remote_bookmarks: RemoteBookmarksMap,
        flags: &[SmartlogFlag],
    ) -> anyhow::Result<Vec<SmartlogNode>> {
        let ctx = self.ctx();
        let repo = self.repo();

        let cs_ids = get_bonsai_from_cloud_ids(
            ctx,
            &cc_ctx,
            repo.bonsai_hg_mapping_arc(),
            repo.bonsai_git_mapping_arc(),
            c_ids,
        )
        .await?
        .into_iter()
        .map(|(_, cs_id)| cs_id)
        .collect::<Vec<ChangesetId>>();

        let public_frontier = repo
            .commit_graph()
            .ancestors_frontier_with(ctx, cs_ids.clone(), |csid| {
                borrowed!(ctx, repo);
                async move {
                    Ok(repo
                        .phases()
                        .get_cached_public(ctx, vec![csid])
                        .await?
                        .contains(&csid))
                }
            })
            .await?;

        let draft_commits_ctx = repo
            .commit_graph()
            .ancestors_difference_stream(ctx, cs_ids, public_frontier.clone())
            .await?
            .map_ok({
                |cs_id| async move { self.changeset(ChangesetSpecifier::Bonsai(cs_id)).await }
            })
            .map_err(MononokeError::from)
            .try_buffered(100)
            .try_collect::<Vec<Option<ChangesetContext<R>>>>()
            .await?;

        let public_commits_ctx = if !flags.contains(&SmartlogFlag::SkipPublicCommitsMetadata) {
            try_join_all(
                public_frontier
                    .into_iter()
                    .map(|cs_id| self.changeset(ChangesetSpecifier::Bonsai(cs_id))),
            )
            .await?
        } else {
            Vec::new()
        };
        let mut nodes = Vec::new();

        let rbs = Arc::new(remote_bookmarks);
        let lbs = Arc::new(local_bookmarks);
        for (phase, changesets) in [
            (Phase::Public, public_commits_ctx),
            (Phase::Draft, draft_commits_ctx),
        ] {
            let changesets = stream::iter(changesets.into_iter().flatten())
                .map(|changeset| {
                    cloned!(rbs, lbs, phase);
                    async move {
                        let res = repo
                            .get_hg_changeset_and_parents_from_bonsai(
                                self.ctx().clone(),
                                changeset.id(),
                            )
                            .await;
                        match res {
                            Ok((hg_id, hg_parents)) => {
                                let c_id = CloudChangesetId::from(hg_id);
                                let parents_c_ids = hg_parents
                                    .into_iter()
                                    .map(CloudChangesetId::from)
                                    .collect::<Vec<_>>();
                                self.repo().commit_cloud().make_smartlog_node(
                                    &c_id,
                                    &parents_c_ids,
                                    &changeset.changeset_info().await?,
                                    &lbs.get(&c_id).cloned(),
                                    &rbs.get(&c_id).cloned(),
                                    &phase,
                                )
                            }
                            Err(e) => Err(e),
                        }
                    }
                })
                .buffer_unordered(100)
                .try_collect::<Vec<SmartlogNode>>()
                .await?;
            nodes.extend(changesets);
        }
        Ok(nodes)
    }

    pub async fn cloud_share_workspace(
        &self,
        workspace: &str,
        reponame: &str,
    ) -> Result<WorkspaceSharingData, MononokeError> {
        let mut ctx = CommitCloudContext::new(workspace, reponame)?;

        let authz = self.authorization_context();
        authz
            .require_commitcloud_operation(self.ctx(), self.repo(), &mut ctx, "maintainers")
            .await?;

        Ok(self.repo().commit_cloud().share_workspace(&ctx).await?)
    }

    pub async fn cloud_update_archive(
        &self,
        workspace: &str,
        reponame: &str,
        archived: bool,
    ) -> Result<String, MononokeError> {
        let mut cc_ctx = CommitCloudContext::new(workspace, reponame)?;

        let authz = self.authorization_context();
        authz
            .require_commitcloud_operation(self.ctx(), self.repo(), &mut cc_ctx, "write")
            .await?;

        Ok(self
            .repo()
            .commit_cloud()
            .update_workspace_archive(&cc_ctx, archived)
            .await?)
    }

    pub async fn cloud_rename_workspace(
        &self,
        workspace: &str,
        reponame: &str,
        new_workspace: &str,
    ) -> Result<String, MononokeError> {
        let mut ctx = CommitCloudContext::new(workspace, reponame)?;

        let authz = self.authorization_context();
        authz
            .require_commitcloud_operation(self.ctx(), self.repo(), &mut ctx, "write")
            .await?;

        Ok(self
            .repo()
            .commit_cloud()
            .rename_workspace(&ctx, new_workspace)
            .await?)
    }

    pub async fn cloud_smartlog_by_version(
        &self,
        workspace: &str,
        reponame: &str,
        filter: &SmartlogFilter,
        flags: &[SmartlogFlag],
    ) -> Result<SmartlogData, MononokeError> {
        let mut cc_ctx = CommitCloudContext::new(workspace, reponame)?;

        let authz = self.authorization_context();
        authz
            .require_commitcloud_operation(self.ctx(), self.repo(), &mut cc_ctx, "read")
            .await?;

        let history = self
            .repo()
            .commit_cloud()
            .get_history_by(&cc_ctx, filter)
            .await?;
        let lbs = history.local_bookmarks_as_map();
        let rbs = history.remote_bookmarks_as_map();

        let hg_ids = history.collapse_into_vec(&rbs, &lbs, flags);

        let nodes = self
            .form_smartlog_with_info(cc_ctx, hg_ids, lbs, rbs, flags)
            .await?;

        Ok(SmartlogData {
            nodes,
            version: Some(history.version as i64),
            timestamp: history.timestamp.map(|ts| ts.timestamp_seconds()),
        })
    }

    pub async fn cloud_historical_versions(
        &self,
        workspace: &str,
        reponame: &str,
    ) -> Result<Vec<HistoricalVersion>, MononokeError> {
        let mut cc_ctx = CommitCloudContext::new(workspace, reponame)?;
        let authz = self.authorization_context();
        authz
            .require_commitcloud_operation(self.ctx(), self.repo(), &mut cc_ctx, "read")
            .await?;

        Ok(self
            .repo()
            .commit_cloud()
            .get_historical_versions(&cc_ctx)
            .await?)
    }

    pub async fn cloud_rollback_workspace(
        &self,
        workspace: &str,
        reponame: &str,
        version: u64,
    ) -> Result<String, MononokeError> {
        let mut cc_ctx = CommitCloudContext::new(workspace, reponame)?;
        let authz = self.authorization_context();
        authz
            .require_commitcloud_operation(self.ctx(), self.repo(), &mut cc_ctx, "write")
            .await?;

        Ok(self
            .repo()
            .commit_cloud()
            .rollback_workspace(&cc_ctx, version)
            .await?)
    }
}
