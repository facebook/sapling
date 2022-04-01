/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::errors;
use crate::source_control_impl::SourceControlServiceImpl;
use context::CoreContext;
use itertools::Itertools;
use source_control as thrift;

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

    pub(crate) async fn commit_sparse_profile_size(
        &self,
        _ctx: CoreContext,
        _commit: thrift::CommitSpecifier,
        _params: thrift::CommitSparseProfileSizeParams,
    ) -> Result<thrift::CommitSparseProfileSizeResponse, errors::ServiceError> {
        Err(errors::ServiceError::Request(errors::not_available(
            "Not implemented".to_string(),
        )))
    }
}
