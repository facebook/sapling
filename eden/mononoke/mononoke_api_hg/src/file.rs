/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;
use blobrepo_hg::file_history::get_file_history_maybe_incomplete;
use blobstore::Loadable;
use blobstore::LoadableError;
use bytes::Bytes;
use futures::compat::Future01CompatExt;
use futures::TryStream;
use futures::TryStreamExt;
use getbundle_response::SessionLfsParams;
use mercurial_types::envelope::HgFileEnvelope;
use mercurial_types::FileType;
use mercurial_types::HgFileHistoryEntry;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgNodeHash;
use mercurial_types::HgParents;
use mononoke_api::errors::MononokeError;
use mononoke_types::fsnode::FsnodeFile;
use mononoke_types::ContentMetadata;
use mononoke_types::MPath;
use remotefilelog::create_getpack_v2_blob;
use revisionstore_types::Metadata;

use super::HgDataContext;
use super::HgDataId;
use super::HgRepoContext;

/// An abstraction around a Mercurial filenode.
///
/// In Mercurial's data model, a filenode is addressed by its content along with
/// its history -- a filenode ID is a hash of the file content and its parents'
/// filenode hashes. Notably, filenodes are not addressed by the path of the file
/// within the repo; as such, perhaps counterintuitively, an HgFileContext is not
/// aware of the path to the file to which it refers.
#[derive(Clone)]
pub struct HgFileContext {
    repo: HgRepoContext,
    envelope: HgFileEnvelope,
}

impl HgFileContext {
    /// Create a new `HgFileContext`. The file must exist in the repository.
    ///
    /// To construct an `HgFileContext` for a file that may not exist, use
    /// `new_check_exists`.
    pub async fn new(
        repo: HgRepoContext,
        filenode_id: HgFileNodeId,
    ) -> Result<Self, MononokeError> {
        // Fetch and store Mononoke's internal representation of the metadata of this
        // file. The actual file contents are not fetched here.
        let ctx = repo.ctx();
        let blobstore = repo.blob_repo().blobstore();
        let envelope = filenode_id.load(ctx, blobstore).await?;
        Ok(Self { repo, envelope })
    }

    pub async fn new_check_exists(
        repo: HgRepoContext,
        filenode_id: HgFileNodeId,
    ) -> Result<Option<Self>, MononokeError> {
        let ctx = repo.ctx();
        let blobstore = repo.blob_repo().blobstore();
        match filenode_id.load(ctx, blobstore).await {
            Ok(envelope) => Ok(Some(Self { repo, envelope })),
            Err(LoadableError::Missing(_)) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Get the history of this file (at a particular path in the repo) as a stream of Mercurial
    /// file history entries.
    ///
    /// Note that since this context could theoretically represent a filenode that existed at
    /// multiple paths within the repo (for example, two files with identical content that were
    /// added at different locations), the caller is required to specify the exact path of the
    /// file to query.
    pub fn history(
        &self,
        path: MPath,
        max_length: Option<u32>,
    ) -> impl TryStream<Ok = HgFileHistoryEntry, Error = MononokeError> {
        let ctx = self.repo.ctx().clone();
        let blob_repo = self.repo.blob_repo().clone();
        let filenode_id = self.node_id();
        get_file_history_maybe_incomplete(
            ctx,
            blob_repo,
            filenode_id,
            path,
            max_length.map(|len| len as u64),
        )
        .map_err(MononokeError::from)
    }

    pub async fn content_metadata(&self) -> Result<ContentMetadata, MononokeError> {
        let content_id = self.envelope.content_id();
        let fetch_key = filestore::FetchKey::Canonical(content_id);
        let blobstore = self.repo.blob_repo().blobstore();
        filestore::get_metadata(blobstore, self.repo.ctx(), &fetch_key)
            .await?
            .ok_or_else(|| {
                MononokeError::NotAvailable(format!(
                    "metadata not found for content id {}",
                    content_id
                ))
            })
    }

    /// Fetches the metadata that would be present in this file's corresponding FsNode, returning
    /// it with the FsNode type, but without actually fetching the FsNode.
    ///
    /// Instead, this method separately reads the `ContentId`, uses that to fetch the size, Sha1,
    /// and Sha256, and combines that with the FileType, which the user must be provide (available
    /// in the parent tree manifest).
    pub async fn fetch_fsnode_data(
        &self,
        file_type: FileType,
    ) -> Result<FsnodeFile, MononokeError> {
        let metadata = self.content_metadata().await?;
        Ok(FsnodeFile::new(
            metadata.content_id,
            file_type,
            metadata.total_size,
            metadata.sha1,
            metadata.sha256,
        ))
    }
}

#[async_trait]
impl HgDataContext for HgFileContext {
    type NodeId = HgFileNodeId;

    /// Get the filenode hash (HgFileNodeId) for this file version.
    ///
    /// This should be same as the HgFileNodeId specified when this context was created,
    /// but the value returned here comes from the data loaded from Mononoke.
    fn node_id(&self) -> HgFileNodeId {
        self.envelope.node_id()
    }

    /// Get the parents of this file version in a strongly typed way.
    ///
    /// Useful for implementing anything that needs to traverse the history
    /// of file nodes, or otherwise needs to use make further queries using
    /// the returned `HgFileNodeId`s.
    fn parents(&self) -> (Option<HgFileNodeId>, Option<HgFileNodeId>) {
        self.envelope.parents()
    }

    /// Get the parents of this file version in a format that can be easily
    /// sent to the Mercurial client as part of a serialized response.
    fn hg_parents(&self) -> HgParents {
        self.envelope.hg_parents()
    }

    /// Get the content and metadata for this file in the format expected by
    /// Mercurial's data storage layer. In particular, this returns the full
    /// content of the file, in some cases prefixed with a small header. Callers
    /// should not assume that the data returned by this function only contains
    /// file content.
    async fn content(&self) -> Result<(Bytes, Metadata), MononokeError> {
        let ctx = self.repo.ctx().clone();
        let blob_repo = self.repo.blob_repo().clone();
        let filenode_id = self.node_id();
        let lfs_params = SessionLfsParams {
            threshold: self.repo.config().lfs.threshold,
        };

        let (_size, content_fut) =
            create_getpack_v2_blob(ctx, blob_repo, filenode_id, lfs_params, false)
                .compat()
                .await?;

        // TODO(kulshrax): Right now this buffers the entire file content in memory. It would
        // probably be better for this method to return a stream of the file content instead.
        let (_filenode, content, metadata) = content_fut.compat().await?;

        Ok((content, metadata))
    }
}

#[async_trait]
impl HgDataId for HgFileNodeId {
    type Context = HgFileContext;

    fn from_node_hash(hash: HgNodeHash) -> Self {
        HgFileNodeId::new(hash)
    }

    async fn context(self, repo: HgRepoContext) -> Result<Option<HgFileContext>, MononokeError> {
        HgFileContext::new_check_exists(repo, self).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::str::FromStr;
    use std::sync::Arc;

    use context::CoreContext;
    use fbinit::FacebookInit;
    use fixtures::ManyFilesDirs;
    use fixtures::TestRepoFixture;
    use futures::TryStreamExt;
    use mercurial_types::HgChangesetId;
    use mercurial_types::NULL_HASH;
    use mononoke_api::repo::Repo;
    use mononoke_api::repo::RepoContext;

    use crate::RepoContextHgExt;

    #[fbinit::test]
    async fn test_hg_file_context(fb: FacebookInit) -> Result<(), MononokeError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(Repo::new_test(ctx.clone(), ManyFilesDirs::getrepo(fb).await).await?);

        // The `ManyFilesDirs` test repo contains the following files (at tip):
        //   $ hg manifest --debug
        //   b8e02f6433738021a065f94175c7cd23db5f05be 644   1
        //   5d9299349fc01ddd25d0070d149b124d8f10411e 644   2
        //   e2ac7cbe1f85e0d8b416005e905aa2189434ce6c 644   dir1
        //   0eb86721b74ed44cf176ee48b5e95f0192dc2824 644   dir2/file_1_in_dir2

        let repo_ctx = RepoContext::new_test(ctx, repo).await?;
        let hg = repo_ctx.hg();

        // Test HgFileContext::new.
        let file_id = HgFileNodeId::from_str("b8e02f6433738021a065f94175c7cd23db5f05be").unwrap();
        let hg_file = HgFileContext::new(hg.clone(), file_id).await?;

        assert_eq!(file_id, hg_file.node_id());

        let content = hg_file.content().await?;
        assert_eq!(&content.0, &b"1\n"[..]);
        assert_eq!(&content.1, &Metadata::default());

        // Test HgFileContext::new_check_exists.
        let hg_file = HgFileContext::new_check_exists(hg.clone(), file_id).await?;
        assert!(hg_file.is_some());

        let null_id = HgFileNodeId::new(NULL_HASH);
        let null_file = HgFileContext::new(hg.clone(), null_id).await;
        assert!(null_file.is_err());

        let null_file = HgFileContext::new_check_exists(hg.clone(), null_id).await?;
        assert!(null_file.is_none());

        Ok(())
    }

    #[fbinit::test]
    async fn test_hg_file_history(fb: FacebookInit) -> Result<(), MononokeError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(Repo::new_test(ctx.clone(), ManyFilesDirs::getrepo(fb).await).await?);

        // The `ManyFilesDirs` test repo contains the following files (at tip):
        //   $ hg manifest --debug
        //   b8e02f6433738021a065f94175c7cd23db5f05be 644   1
        //   5d9299349fc01ddd25d0070d149b124d8f10411e 644   2
        //   e2ac7cbe1f85e0d8b416005e905aa2189434ce6c 644   dir1
        //   0eb86721b74ed44cf176ee48b5e95f0192dc2824 644   dir2/file_1_in_dir2

        let repo_ctx = RepoContext::new_test(ctx, repo).await?;
        let hg = repo_ctx.hg();

        // Test HgFileContext::new.
        let file_id = HgFileNodeId::from_str("b8e02f6433738021a065f94175c7cd23db5f05be").unwrap();
        let hg_file = HgFileContext::new(hg.clone(), file_id).await?;

        let path = MPath::new("1")?;
        let history = hg_file.history(path, None).try_collect::<Vec<_>>().await?;

        let expected = vec![HgFileHistoryEntry::new(
            file_id,
            HgParents::None,
            HgChangesetId::new(NULL_HASH),
            None,
        )];
        assert_eq!(history, expected);

        Ok(())
    }
}
