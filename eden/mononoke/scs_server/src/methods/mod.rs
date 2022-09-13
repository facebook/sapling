/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use context::CoreContext;
use source_control as thrift;

use crate::errors;
use crate::source_control_impl::SourceControlServiceImpl;

pub(crate) mod commit;
pub(crate) mod commit_lookup_pushrebase_history;
pub(crate) mod commit_path;
pub(crate) mod commit_sparse_profile_info;
pub(crate) mod file;
pub(crate) mod megarepo;
pub(crate) mod repo;
pub(crate) mod tree;

impl SourceControlServiceImpl {
    pub(crate) async fn list_repos(
        &self,
        _ctx: CoreContext,
        _params: thrift::ListReposParams,
    ) -> Result<Vec<thrift::Repo>, errors::ServiceError> {
        let mut repo_names: Vec<_> = self.mononoke.repo_names_in_tier.clone();
        repo_names.sort();
        let rsp = repo_names
            .into_iter()
            .map(|repo_name| thrift::Repo {
                name: repo_name,
                ..Default::default()
            })
            .collect();
        Ok(rsp)
    }
}
