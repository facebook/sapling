/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use blobstore::Loadable;
use bytes::Bytes;
use futures_preview::compat::Future01CompatExt;
use mercurial_types::envelope::HgFileEnvelope;
use remotefilelog::create_getpack_v1_blob;

use crate::errors::MononokeError;

use super::HgRepoContext;

pub use mercurial_types::HgFileNodeId;
pub use mercurial_types::HgParents;

#[derive(Clone)]
pub struct HgFileContext {
    repo: HgRepoContext,
    envelope: HgFileEnvelope,
}

impl HgFileContext {
    pub async fn new(
        repo: HgRepoContext,
        filenode_id: HgFileNodeId,
    ) -> Result<Self, MononokeError> {
        // Fetch and store Mononoke's internal representation of the metadata of this
        // file. The actual file contents are not fetched here.
        let ctx = repo.ctx().clone();
        let blobstore = repo.blob_repo().blobstore();
        let envelope = filenode_id.load(ctx, blobstore).compat().await?;
        Ok(Self { repo, envelope })
    }

    /// Get the filenode hash (HgFileNodeId) for this file version.
    ///
    /// This should be same as the HgFileNodeId specified when this context was created,
    /// but the value returned here comes from the data loaded from Mononoke.
    pub fn node_id(&self) -> HgFileNodeId {
        self.envelope.node_id()
    }

    /// Get the parents of this file version in a strongly typed way.
    ///
    /// Useful for implementing anything that needs to traverse the history
    /// of file nodes, or otherwise needs to use make further queries using
    /// the returned `HgFileNodeId`s.
    pub fn parents(&self) -> (Option<HgFileNodeId>, Option<HgFileNodeId>) {
        self.envelope.parents()
    }

    /// Get the parents of this file version in a format that can be easily
    /// sent to the Mercurial client as part of a serialized response.
    pub fn hg_parents(&self) -> HgParents {
        self.envelope.hg_parents()
    }

    /// Get the content for this file in the format expected by Mercurial's data storage layer.
    /// In particular, this returns the full content of the file, in some cases prefixed with
    /// a small header. Callers should not assume that the data returned by this function
    /// only contains file content.
    pub async fn content(&self) -> Result<Bytes, MononokeError> {
        let ctx = self.repo.ctx().clone();
        let blob_repo = self.repo.blob_repo().clone();
        let filenode_id = self.node_id();

        // TODO(kulshrax): Update this to use getpack_v2, which supports LFS.
        let (_size, content_fut) = create_getpack_v1_blob(ctx, blob_repo, filenode_id, false)
            .compat()
            .await?;

        // TODO(kulshrax): Right now this buffers the entire file content in memory. It would
        // probably be better for this method to return a stream of the file content instead.
        let (_filenode, content) = content_fut.compat().await?;

        Ok(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::{str::FromStr, sync::Arc};

    use context::CoreContext;
    use fbinit::FacebookInit;
    use fixtures::many_files_dirs;
    use mercurial_types::NULL_HASH;

    use crate::repo::{Repo, RepoContext};

    #[fbinit::test]
    fn test_hg_file_context(fb: FacebookInit) -> Result<(), MononokeError> {
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();
        runtime.block_on_std(async move {
            let ctx = CoreContext::test_mock(fb);
            let repo =
                Arc::new(Repo::new_test(ctx.clone(), many_files_dirs::getrepo(fb).await).await?);

            // The `many_files_dirs` test repo contains the following files (at tip):
            //   $ hg manifest --debug
            //   b8e02f6433738021a065f94175c7cd23db5f05be 644   1
            //   5d9299349fc01ddd25d0070d149b124d8f10411e 644   2
            //   e2ac7cbe1f85e0d8b416005e905aa2189434ce6c 644   dir1
            //   0eb86721b74ed44cf176ee48b5e95f0192dc2824 644   dir2/file_1_in_dir2

            let repo_ctx = RepoContext::new(ctx, repo)?;
            let hg = repo_ctx.hg();

            let file_id =
                HgFileNodeId::from_str("b8e02f6433738021a065f94175c7cd23db5f05be").unwrap();
            let hg_file = hg.file(file_id).await?;

            assert_eq!(file_id, hg_file.node_id());

            let content = hg_file.content().await?;
            assert_eq!(content, &b"1\n"[..]);

            let null_id = HgFileNodeId::new(NULL_HASH);
            let null_file = hg.file(null_id).await;
            assert!(null_file.is_err());

            Ok(())
        })
    }
}
