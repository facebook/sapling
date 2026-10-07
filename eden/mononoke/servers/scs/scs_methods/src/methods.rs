/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use context::CoreContext;
use itertools::Itertools;
use metaconfig_types::CommitIdentityScheme;
use source_control as thrift;

use crate::from_request::FromRequest;
use crate::source_control_impl::SourceControlServiceImpl;

pub(crate) mod cloud;
pub(crate) mod commit;
pub(crate) mod commit_lookup_pushrebase_history;
pub(crate) mod commit_path;
pub(crate) mod commit_restricted_paths;
pub mod commit_sparse_profile_info;
pub(crate) mod create_repos;
pub(crate) mod file;
pub(crate) mod git;
pub(crate) mod git_repo_state;
pub(crate) mod megarepo;
pub(crate) mod repo;
pub(crate) mod tree;

/// Partial-response verdict for a unary method, from the method's total
/// omitted-restricted count. `Some` carrying the exact signal when partial
/// responses are allowed for this request (`None`, field unset, when not).
/// The caller sets `nocache: 1` from the same flag, so a cached partial can
/// never be served to an entitled caller.
pub(crate) fn partial_response_info(
    omitted_restricted_paths_count: Option<usize>,
) -> Option<thrift::PartialResponseInfo> {
    omitted_restricted_paths_count.map(|omitted_restricted_paths_count| {
        thrift::PartialResponseInfo {
            partial: omitted_restricted_paths_count > 0,
            omitted_restricted_paths_count: Some(omitted_restricted_paths_count as i64),
            ..Default::default()
        }
    })
}

impl SourceControlServiceImpl {
    pub(crate) async fn list_repos(
        &self,
        _ctx: CoreContext,
        params: thrift::ListReposParams,
    ) -> Result<Vec<thrift::Repo>, scs_errors::ServiceError> {
        let snapshot = self.mononoke.repo_names_in_tier.load();
        let names = snapshot.iter();
        let names: Box<dyn Iterator<Item = _>> =
            if let Some(identity_schemes) = params.identity_schemes {
                let schemes = identity_schemes
                    .iter()
                    .map(CommitIdentityScheme::from_request)
                    .collect::<Result<Vec<_>, _>>()?;

                Box::new(names.filter(move |(_, default_scheme)| schemes.contains(default_scheme)))
            } else {
                Box::new(names)
            };

        Ok(names
            .sorted_by(|(a, _), (b, _)| a.cmp(b))
            .map(|(repo_name, _)| thrift::Repo {
                name: repo_name.clone(),
                ..Default::default()
            })
            .collect())
    }

    pub(crate) async fn repo_exists(
        &self,
        _ctx: CoreContext,
        params: thrift::RepoExistsParams,
    ) -> Result<thrift::RepoExistsResponse, scs_errors::ServiceError> {
        let exists = self
            .mononoke
            .repo_names_in_tier
            .load()
            .contains_key(&params.repo_name);
        Ok(thrift::RepoExistsResponse {
            exists,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_partial_response_info() {
        // Not allowed: no field.
        assert!(partial_response_info(None).is_none());
        // Allowed: always populated; `partial` iff count is positive,
        // agreeing with the `nocache` decision (set iff count positive) by
        // construction.
        let complete = partial_response_info(Some(0)).expect("populated when allowed");
        assert!(!complete.partial);
        assert_eq!(complete.omitted_restricted_paths_count, Some(0));
        let partial = partial_response_info(Some(3)).expect("populated when allowed");
        assert!(partial.partial);
        assert_eq!(partial.omitted_restricted_paths_count, Some(3));
    }
}
