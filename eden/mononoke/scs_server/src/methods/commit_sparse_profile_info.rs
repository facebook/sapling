/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use context::CoreContext;
use itertools::Itertools;
use mononoke_api::sparse_profile::get_profile_delta_size;
use mononoke_api::sparse_profile::MonitoringProfiles;
use mononoke_api::sparse_profile::ProfileSizeChange;
use mononoke_api::sparse_profile::SparseProfileMonitoring;
use source_control as thrift;

use crate::errors;
use crate::source_control_impl::SourceControlServiceImpl;

pub(crate) trait SparseProfilesExt {
    fn to_string(&self) -> String;
}

impl SparseProfilesExt for thrift::SparseProfiles {
    fn to_string(&self) -> String {
        match self {
            thrift::SparseProfiles::all_profiles(_) => "all sparse profiles".to_string(),
            thrift::SparseProfiles::profiles(profiles) => profiles
                .iter()
                .format_with("\n", |item, f| f(&item))
                .to_string(),
            thrift::SparseProfiles::UnknownField(t) => format!("unknown SparseProfiles type {}", t),
        }
    }
}

impl SourceControlServiceImpl {
    pub(crate) async fn commit_sparse_profile_size(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitSparseProfileSizeParams,
    ) -> Result<thrift::CommitSparseProfileSizeResponse, errors::ServiceError> {
        let (repo, changeset) = self.repo_changeset(ctx.clone(), &commit).await?;
        let profiles = convert_profiles_params(params.profiles).await?;
        let monitor = SparseProfileMonitoring::new(
            repo.name(),
            repo.sparse_profiles().clone(),
            repo.config().sparse_profiles_config.clone(),
            profiles,
        )?;
        let profiles = monitor.get_monitoring_profiles(&changeset).await?;
        let sizes_hashmap = monitor.get_profile_size(&ctx, &changeset, profiles).await?;
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

    pub(crate) async fn commit_sparse_profile_delta(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitSparseProfileDeltaParams,
    ) -> Result<thrift::CommitSparseProfileDeltaResponse, errors::ServiceError> {
        let (repo, changeset, other) = self
            .repo_changeset_pair(ctx.clone(), &commit, &params.other_id)
            .await?;
        let profiles = convert_profiles_params(params.profiles).await?;
        let monitor = SparseProfileMonitoring::new(
            repo.name(),
            repo.sparse_profiles().clone(),
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
}

async fn convert_profiles_params(
    params_profiles: thrift::SparseProfiles,
) -> Result<MonitoringProfiles, errors::ServiceError> {
    match params_profiles {
        thrift::SparseProfiles::all_profiles(_) => Ok(MonitoringProfiles::All),
        thrift::SparseProfiles::profiles(profiles) => Ok(MonitoringProfiles::Exact { profiles }),
        thrift::SparseProfiles::UnknownField(_) => Err(errors::ServiceError::Request(
            errors::not_available("Not implemented".to_string()),
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
