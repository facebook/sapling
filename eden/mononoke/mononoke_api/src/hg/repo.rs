/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use blobrepo::BlobRepo;
use context::CoreContext;
use futures::{compat::Stream01CompatExt, TryStream, TryStreamExt};
use hgproto::GettreepackArgs;
use mercurial_types::{HgFileNodeId, HgManifestId};
use mononoke_types::MPath;
use repo_client::gettreepack_entries;

use crate::errors::MononokeError;
use crate::path::MononokePath;
use crate::repo::RepoContext;

use super::{HgFileContext, HgTreeContext};

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

    /// Look up a tree in the repo by `HgManifestId`.
    pub async fn tree(
        &self,
        manifest_id: HgManifestId,
    ) -> Result<Option<HgTreeContext>, MononokeError> {
        HgTreeContext::new_check_exists(self.clone(), manifest_id).await
    }

    /// Request all of the tree nodes in the repo under a given path.
    ///
    /// The caller must specify a list of desired versions of the subtree for
    /// this path, specified as a list of manifest IDs of tree nodes
    /// corresponding to different versions of the root node of the subtree.
    ///
    /// The caller may also specify a list of versions of the subtree to
    /// delta against. The server will only return tree nodes that are in
    /// the requested subtrees that are not in the base subtrees.
    ///
    /// Returns a stream of `HgTreeContext`s, each corresponding to a node in
    /// the requested versions of the subtree, along with its associated path.
    ///
    /// This method is equivalent to Mercurial's `gettreepack` wire protocol
    /// command.
    pub fn trees_under_path(
        &self,
        path: MononokePath,
        root_versions: impl IntoIterator<Item = HgManifestId>,
        base_versions: impl IntoIterator<Item = HgManifestId>,
        depth: Option<usize>,
    ) -> impl TryStream<Ok = (HgTreeContext, MononokePath), Error = MononokeError> {
        let ctx = self.ctx().clone();
        let blob_repo = self.blob_repo();
        let args = GettreepackArgs {
            rootdir: path.into_mpath(),
            mfnodes: root_versions.into_iter().collect(),
            basemfnodes: base_versions.into_iter().collect(),
            directories: vec![], // Not supported.
            depth,
        };

        gettreepack_entries(ctx, blob_repo, args)
            .compat()
            .map_err(MononokeError::from)
            .and_then({
                let repo = self.clone();
                move |(mfid, path): (HgManifestId, Option<MPath>)| {
                    let repo = repo.clone();
                    async move {
                        let tree = HgTreeContext::new(repo, mfid).await?;
                        let path = MononokePath::new(path);
                        Ok((tree, path))
                    }
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use fbinit::FacebookInit;
    use fixtures::linear;

    use crate::repo::Repo;

    #[fbinit::compat_test]
    async fn test_new_hg_context(fb: FacebookInit) -> Result<(), MononokeError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = Repo::new_test(ctx.clone(), linear::getrepo(fb).await).await?;
        let repo_ctx = RepoContext::new(ctx, Arc::new(repo))?;

        let hg = repo_ctx.hg();
        assert_eq!(hg.repo().name(), "test");

        Ok(())
    }
}
