/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use context::CoreContext;
use itertools::Itertools;
use mononoke_api::sparse_profile::{get_all_profiles, get_profile_size};
use mononoke_types::MPath;
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
        let (_repo, changeset) = self.repo_changeset(ctx.clone(), &commit).await?;
        let profiles = match params.profiles {
            thrift::SparseProfiles::all_profiles(_) => get_all_profiles(&changeset)
                .await
                .map_err(errors::internal_error)?,
            thrift::SparseProfiles::profiles(profiles) => profiles
                .into_iter()
                .filter_map(|path| {
                    let path: &str = &path;
                    MPath::try_from(path).ok()
                })
                .collect(),
            thrift::SparseProfiles::UnknownField(_) => {
                return Err(errors::ServiceError::Request(errors::not_available(
                    "Not implemented".to_string(),
                )));
            }
        };
        let sizes_hashmap = get_profile_size(&ctx, &changeset, profiles).await?;
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
        _tx: CoreContext,
        _commit: thrift::CommitSpecifier,
        _params: thrift::CommitSparseProfileDeltaParams,
    ) -> Result<thrift::CommitSparseProfileDeltaResponse, errors::ServiceError> {
        Err(errors::ServiceError::Request(errors::not_available(
            "Not implemented".to_string(),
        )))
    }
}
