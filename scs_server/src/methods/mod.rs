/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use context::CoreContext;
use source_control as thrift;

use crate::errors;
use crate::source_control_impl::SourceControlServiceImpl;

pub(crate) mod commit;
pub(crate) mod commit_path;
pub(crate) mod file;
pub(crate) mod repo;
pub(crate) mod tree;

impl SourceControlServiceImpl {
    pub(crate) async fn list_repos(
        &self,
        _ctx: CoreContext,
        _params: thrift::ListReposParams,
    ) -> Result<Vec<thrift::Repo>, errors::ServiceError> {
        let mut repo_names: Vec<_> = self
            .mononoke
            .repo_names()
            .map(|repo_name| repo_name.to_string())
            .collect();
        repo_names.sort();
        let rsp = repo_names
            .into_iter()
            .map(|repo_name| thrift::Repo { name: repo_name })
            .collect();
        Ok(rsp)
    }
}
