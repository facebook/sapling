/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;
use blobstore::Loadable;
use bytes::Bytes;
use manifest::Entry;
use manifest::Manifest;
use mercurial_types::fetch_manifest_envelope;
use mercurial_types::fetch_manifest_envelope_opt;
use mercurial_types::HgAugmentedManifestEntry;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgBlobEnvelope;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestEnvelope;
use mercurial_types::HgManifestId;
use mercurial_types::HgNodeHash;
use mercurial_types::HgParents;
use mercurial_types::HgPreloadedAugmentedManifest;
use mononoke_api::errors::MononokeError;
use mononoke_types::file_change::FileType;
use mononoke_types::MPathElement;
use repo_blobstore::RepoBlobstoreRef;
use revisionstore_types::Metadata;

use super::HgDataContext;
use super::HgDataId;
use super::HgRepoContext;

#[derive(Clone)]
pub struct HgTreeContext {
    #[allow(dead_code)]
    repo: HgRepoContext,
    envelope: HgManifestEnvelope,
}

impl HgTreeContext {
    /// Create a new `HgTreeContext`, representing a single tree manifest node.
    ///
    /// The tree node must exist in the repository. To construct an `HgTreeContext`
    /// for a tree node that may not exist, use `new_check_exists`.
    pub async fn new(
        repo: HgRepoContext,
        manifest_id: HgManifestId,
    ) -> Result<Self, MononokeError> {
        let ctx = repo.ctx();
        let blobstore = repo.blob_repo().repo_blobstore();
        let envelope = fetch_manifest_envelope(ctx, blobstore, manifest_id).await?;
        Ok(Self { repo, envelope })
    }

    pub async fn new_check_exists(
        repo: HgRepoContext,
        manifest_id: HgManifestId,
    ) -> Result<Option<Self>, MononokeError> {
        let ctx = repo.ctx();
        let blobstore = repo.blob_repo().repo_blobstore();
        let envelope = fetch_manifest_envelope_opt(ctx, blobstore, manifest_id).await?;
        Ok(envelope.map(move |envelope| Self { repo, envelope }))
    }

    /// Get the content for this tree manifest node in the format expected
    /// by Mercurial's data storage layer.
    pub fn content_bytes(&self) -> Bytes {
        self.envelope.contents().clone()
    }

    pub fn into_blob_manifest(self) -> anyhow::Result<mercurial_types::blobs::HgBlobManifest> {
        mercurial_types::blobs::HgBlobManifest::parse(self.envelope)
    }

    pub fn entries(
        &self,
    ) -> anyhow::Result<
        impl Iterator<Item = (MPathElement, Entry<HgManifestId, (FileType, HgFileNodeId)>)>,
    > {
        Ok(self.clone().into_blob_manifest()?.list())
    }
}

#[derive(Clone)]
pub struct HgAugmentedTreeContext {
    #[allow(dead_code)]
    repo: HgRepoContext,
    preloaded_manifest: HgPreloadedAugmentedManifest,
}

impl HgAugmentedTreeContext {
    /// Create a new `HgAugmentedTreeContext`, representing a single augmented tree manifest node.
    pub async fn new_check_exists(
        repo: HgRepoContext,
        augmented_manifest_id: HgAugmentedManifestId,
    ) -> Result<Option<Self>, MononokeError> {
        let ctx = repo.ctx();
        let blobstore = repo.blob_repo().repo_blobstore();
        let envelope = augmented_manifest_id.load(ctx, blobstore).await?;
        let preloaded_manifest = HgPreloadedAugmentedManifest::load_from_sharded(
            envelope.augmented_manifest,
            ctx,
            blobstore,
        )
        .await?;
        Ok(Some(Self {
            repo,
            preloaded_manifest,
        }))
    }

    pub fn augmented_children_entries(&self) -> impl Iterator<Item = &HgAugmentedManifestEntry> {
        self.preloaded_manifest.children_augmented_metadata.iter()
    }

    /// Get the content for this tree manifest node in the format expected
    /// by Mercurial's data storage layer.
    pub fn content_bytes(&self) -> Bytes {
        self.preloaded_manifest.contents.clone()
    }
}

#[async_trait]
impl HgDataContext for HgTreeContext {
    type NodeId = HgManifestId;

    /// Get the manifest node hash (HgManifestId) for this tree.
    ///
    /// This should be same as the HgManifestId specified when this context was created,
    /// but the value returned here comes from the data loaded from Mononoke.
    fn node_id(&self) -> HgManifestId {
        HgManifestId::new(self.envelope.node_id())
    }

    /// Get the parents of this tree node in a strongly-typed manner.
    ///
    /// Useful for implementing anything that needs to traverse the history
    /// of tree nodes, or otherwise needs to use make further queries using
    /// the returned `HgManifestId`s.
    fn parents(&self) -> (Option<HgManifestId>, Option<HgManifestId>) {
        let (p1, p2) = self.envelope.parents();
        (p1.map(HgManifestId::new), p2.map(HgManifestId::new))
    }

    /// Get the parents of this tree node in a format that can be easily
    /// sent to the Mercurial client as part of a serialized response.
    fn hg_parents(&self) -> HgParents {
        self.envelope.get_parents()
    }

    /// The manifest envelope actually contains the underlying tree bytes
    /// inline, so they can be accessed synchronously and infallibly with the
    /// `content_bytes` method. This method just wraps the bytes in a TryFuture
    /// that immediately succeeds. Note that tree blobs don't have associated
    /// metadata so we just return the default value here.
    async fn content(&self) -> Result<(Bytes, Metadata), MononokeError> {
        Ok((self.content_bytes(), Metadata::default()))
    }
}

#[async_trait]
impl HgDataContext for HgAugmentedTreeContext {
    type NodeId = HgManifestId;

    /// Get the manifest node hash (HgAugmentedManifestId) for this tree.
    ///
    /// This should be same as the HgAugmentedManifestId specified when this context was created,
    /// but the value returned here comes from the data loaded from Mononoke.
    fn node_id(&self) -> HgManifestId {
        HgManifestId::new(self.preloaded_manifest.hg_node_id)
    }

    /// Get the parents of this tree node in a strongly-typed manner.
    ///
    /// Useful for implementing anything that needs to traverse the history
    /// of tree nodes, or otherwise needs to use make further queries using
    /// the returned `HgManifestId`s.
    fn parents(&self) -> (Option<HgManifestId>, Option<HgManifestId>) {
        (
            self.preloaded_manifest.p1.map(HgManifestId::new),
            self.preloaded_manifest.p2.map(HgManifestId::new),
        )
    }

    /// Get the parents of this tree node in a format that can be easily
    /// sent to the Mercurial client as part of a serialized response.
    fn hg_parents(&self) -> HgParents {
        HgParents::new(self.preloaded_manifest.p1, self.preloaded_manifest.p2)
    }

    async fn content(&self) -> Result<(Bytes, Metadata), MononokeError> {
        Ok((self.content_bytes(), Metadata::default()))
    }
}

#[async_trait]
impl HgDataId for HgManifestId {
    type Context = HgTreeContext;

    fn from_node_hash(hash: HgNodeHash) -> Self {
        HgManifestId::new(hash)
    }

    async fn context(self, repo: HgRepoContext) -> Result<Option<HgTreeContext>, MononokeError> {
        HgTreeContext::new_check_exists(repo, self).await
    }
}

#[async_trait]
impl HgDataId for HgAugmentedManifestId {
    type Context = HgAugmentedTreeContext;

    fn from_node_hash(hash: HgNodeHash) -> Self {
        HgAugmentedManifestId::new(hash)
    }

    async fn context(
        self,
        repo: HgRepoContext,
    ) -> Result<Option<HgAugmentedTreeContext>, MononokeError> {
        HgAugmentedTreeContext::new_check_exists(repo, self).await
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::sync::Arc;

    use blobstore::Loadable;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use fixtures::Linear;
    use fixtures::TestRepoFixture;
    use mercurial_types::NULL_HASH;
    use mononoke_api::repo::Repo;
    use mononoke_api::repo::RepoContext;
    use mononoke_api::specifiers::HgChangesetId;

    use super::*;
    use crate::RepoContextHgExt;

    #[fbinit::test]
    async fn test_hg_tree_context(fb: FacebookInit) -> Result<(), MononokeError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(Linear::get_custom_test_repo::<Repo>(fb).await);
        let rctx = RepoContext::new_test(ctx.clone(), repo.clone()).await?;

        // Get the HgManifestId of the root tree manifest for a commit in this repo.
        // (Commit hash was found by inspecting the source of the `fixtures` crate.)
        let hg_cs_id = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
        let hg_cs = hg_cs_id
            .load(&ctx, rctx.blob_repo().repo_blobstore())
            .await?;
        let manifest_id = hg_cs.manifestid();

        let hg = rctx.hg();

        let tree = HgTreeContext::new(hg.clone(), manifest_id).await?;
        assert_eq!(manifest_id, tree.node_id());

        let content = tree.content_bytes();

        // The content here is the representation of the format in which
        // the Mercurial client would store a tree manifest node.
        let expected = &b"1\0b8e02f6433738021a065f94175c7cd23db5f05be\nfiles\0b8e02f6433738021a065f94175c7cd23db5f05be\n"[..];
        assert_eq!(content, expected);

        let tree = HgTreeContext::new_check_exists(hg.clone(), manifest_id).await?;
        assert!(tree.is_some());

        let null_id = HgManifestId::new(NULL_HASH);
        let null_tree = HgTreeContext::new(hg.clone(), null_id).await;
        assert!(null_tree.is_err());

        let null_tree = HgTreeContext::new_check_exists(hg.clone(), null_id).await?;
        assert!(null_tree.is_none());

        Ok(())
    }
}
