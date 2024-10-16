/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_requests::types::CommitSparseProfileSizeToken;
use context::CoreContext;
use mononoke_api::sparse_profile::get_profile_delta_size;
use mononoke_api::sparse_profile::MonitoringProfiles;
use mononoke_api::sparse_profile::ProfileSizeChange;
use mononoke_api::sparse_profile::SparseProfileMonitoring;
use mononoke_api::ChangesetContext;
use mononoke_api::Repo;
use mononoke_api::RepoContext;
use source_control as thrift;

use crate::async_requests::enqueue;
use crate::async_requests::poll;
use crate::source_control_impl::SourceControlServiceImpl;

impl SourceControlServiceImpl {
    pub(crate) async fn commit_sparse_profile_size(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitSparseProfileSizeParams,
    ) -> Result<thrift::CommitSparseProfileSizeResponse, scs_errors::ServiceError> {
        let (repo, changeset) = self.repo_changeset(ctx.clone(), &commit).await?;
        commit_sparse_profile_size_impl(&ctx, repo, changeset, params.profiles).await
    }

    pub(crate) async fn commit_sparse_profile_delta(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitSparseProfileDeltaParams,
    ) -> Result<thrift::CommitSparseProfileDeltaResponse, scs_errors::ServiceError> {
        let (repo, changeset, other) = self
            .repo_changeset_pair(ctx.clone(), &commit, &params.other_id)
            .await?;
        let profiles = convert_profiles_params(params.profiles).await?;
        let monitor = SparseProfileMonitoring::new(
            repo.name(),
            repo.sparse_profiles(),
            repo.config().sparse_profiles_config.clone(),
            profiles,
        )?;
        let profiles = monitor.get_monitoring_profiles(&changeset).await?;
        let sizes_hashmap =
            get_profile_delta_size(&ctx, &monitor, &changeset, &other, profiles).await;
        let sizes = sizes_hashmap?
            .into_iter()
            .map(|(source, change)| {
                (
                    source,
                    thrift::SparseProfileChange {
                        change: convert(change),
                        ..Default::default()
                    },
                )
            })
            .collect();
        Ok(thrift::CommitSparseProfileDeltaResponse {
            changed_sparse_profiles: Some(thrift::SparseProfileDeltaSizes {
                size_changes: sizes,
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    pub(crate) async fn commit_sparse_profile_size_async(
        &self,
        ctx: CoreContext,
        params: thrift::CommitSparseProfileSizeParamsV2,
    ) -> Result<thrift::CommitSparseProfileSizeToken, scs_errors::ServiceError> {
        let (repo, _changeset) = self.repo_changeset(ctx.clone(), &params.commit).await?;
        enqueue::<thrift::CommitSparseProfileSizeParamsV2>(
            &ctx,
            &self.async_requests_queue,
            Some(&repo.repoid()),
            params,
        )
        .await
    }

    pub(crate) async fn commit_sparse_profile_size_poll(
        &self,
        ctx: CoreContext,
        token: thrift::CommitSparseProfileSizeToken,
    ) -> Result<thrift::CommitSparseProfileSizePollResponse, scs_errors::ServiceError> {
        let token = CommitSparseProfileSizeToken(token);
        poll::<CommitSparseProfileSizeToken>(&ctx, &self.async_requests_queue, token).await
    }
}

pub async fn commit_sparse_profile_size_impl(
    ctx: &CoreContext,
    repo: RepoContext<Repo>,
    changeset: ChangesetContext<Repo>,
    profiles: thrift::SparseProfiles,
) -> Result<thrift::CommitSparseProfileSizeResponse, scs_errors::ServiceError> {
    let profiles = convert_profiles_params(profiles).await?;
    let monitor = SparseProfileMonitoring::new(
        repo.name(),
        repo.sparse_profiles(),
        repo.config().sparse_profiles_config.clone(),
        profiles,
    )?;
    let profiles = monitor.get_monitoring_profiles(&changeset).await?;
    let sizes_hashmap = monitor.get_profile_size(ctx, &changeset, profiles).await?;
    let sizes = sizes_hashmap
        .into_iter()
        .map(|(source, size)| {
            (
                source,
                thrift::SparseProfileSize {
                    size: size as i64,
                    ..Default::default()
                },
            )
        })
        .collect();
    Ok(thrift::CommitSparseProfileSizeResponse {
        profiles_size: thrift::SparseProfileSizes {
            sizes,
            ..Default::default()
        },
        ..Default::default()
    })
}

async fn convert_profiles_params(
    params_profiles: thrift::SparseProfiles,
) -> Result<MonitoringProfiles, scs_errors::ServiceError> {
    match params_profiles {
        thrift::SparseProfiles::all_profiles(_) => Ok(MonitoringProfiles::All),
        thrift::SparseProfiles::profiles(profiles) => Ok(MonitoringProfiles::Exact { profiles }),
        thrift::SparseProfiles::UnknownField(_) => Err(scs_errors::ServiceError::Request(
            scs_errors::not_available("Not implemented".to_string()),
        )),
    }
}

fn convert(change: ProfileSizeChange) -> thrift::SparseProfileChangeElement {
    match change {
        ProfileSizeChange::Added(size) => {
            thrift::SparseProfileChangeElement::added(thrift::SparseProfileAdded {
                size: size as i64,
                ..Default::default()
            })
        }
        ProfileSizeChange::Removed(size) => {
            thrift::SparseProfileChangeElement::removed(thrift::SparseProfileRemoved {
                previous_size: size as i64,
                ..Default::default()
            })
        }
        ProfileSizeChange::Changed(size) => {
            thrift::SparseProfileChangeElement::changed(thrift::SparseProfileSizeChanged {
                size_change: size,
                ..Default::default()
            })
        }
    }
}
