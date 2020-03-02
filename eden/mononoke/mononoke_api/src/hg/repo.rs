/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use blobrepo::BlobRepo;
use context::CoreContext;
use mercurial_types::HgFileNodeId;

use crate::errors::MononokeError;
use crate::repo::RepoContext;

use super::HgFileContext;

#[derive(Clone)]
pub struct HgRepoContext {
    repo: RepoContext,
}

impl HgRepoContext {
    pub(crate) fn new(repo: RepoContext) -> Self {
        Self { repo }
    }

    /// The `CoreContext` for this query.
    pub(crate) fn ctx(&self) -> &CoreContext {
        &self.repo.ctx()
    }

    /// The `RepoContext` for this query.
    pub(crate) fn repo(&self) -> &RepoContext {
        &self.repo
    }

    /// The underlying Mononoke `BlobRepo` backing this repo.
    pub(crate) fn blob_repo(&self) -> &BlobRepo {
        &self.repo().blob_repo()
    }

    /// Look up a file in the repo by `HgFileNodeId`.
    pub async fn file(
        &self,
        filenode_id: HgFileNodeId,
    ) -> Result<Option<HgFileContext>, MononokeError> {
        HgFileContext::new_check_exists(self.clone(), filenode_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use fbinit::FacebookInit;
    use fixtures::linear;

    use crate::repo::Repo;

    #[fbinit::test]
    fn test_new_hg_context(fb: FacebookInit) -> Result<(), MononokeError> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on_std(async move {
            let ctx = CoreContext::test_mock(fb);
            let repo = Repo::new_test(ctx.clone(), linear::getrepo(fb).await).await?;
            let repo_ctx = RepoContext::new(ctx, Arc::new(repo))?;

            let hg = repo_ctx.hg();
            assert_eq!(hg.repo().name(), "test");

            Ok(())
        })
    }
}
