/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use mercurial_types::HgAugmentedManifestEntry;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgBlobEnvelope;
use mercurial_types::HgManifestEnvelope;
use mercurial_types::HgManifestId;
use mercurial_types::HgNodeHash;
use mercurial_types::HgParents;
use mercurial_types::HgPreloadedAugmentedManifest;
use mercurial_types::fetch_augmented_manifest_envelope_opt;
use mercurial_types::fetch_manifest_envelope;
use mercurial_types::fetch_manifest_envelope_opt;
use mononoke_api::MononokeRepo;
use mononoke_api::errors::MononokeError;
use mononoke_macros::mononoke;
use mononoke_types::MPathElement;
use mononoke_types::hash::Blake3;
use repo_blobstore::RepoBlobstoreRef;
use restricted_paths::ManifestId;
use restricted_paths::ManifestType;
use restricted_paths::RestrictedPathsArc;
use revisionstore_types::Metadata;

use super::HgDataContext;
use super::HgDataId;
use super::HgRepoContext;

#[derive(Clone)]
pub struct HgTreeContext<R> {
    #[allow(dead_code)]
    repo_ctx: HgRepoContext<R>,
    envelope: HgManifestEnvelope,
}

impl<R: MononokeRepo> HgTreeContext<R> {
    /// Create a new `HgTreeContext`, representing a single tree manifest node.
    ///
    /// The tree node must exist in the repository. To construct an `HgTreeContext`
    /// for a tree node that may not exist, use `new_check_exists`.
    pub async fn new(
        repo_ctx: HgRepoContext<R>,
        manifest_id: HgManifestId,
    ) -> Result<Self, MononokeError> {
        let ctx = repo_ctx.ctx();
        let blobstore = repo_ctx.repo().repo_blobstore();
        let envelope = fetch_manifest_envelope(ctx, blobstore, manifest_id).await?;
        Ok(Self { repo_ctx, envelope })
    }

    pub async fn new_check_exists(
        repo_ctx: HgRepoContext<R>,
        manifest_id: HgManifestId,
    ) -> Result<Option<Self>, MononokeError> {
        let ctx = repo_ctx.ctx();
        let blobstore = repo_ctx.repo().repo_blobstore();
        let envelope = fetch_manifest_envelope_opt(ctx, blobstore, manifest_id).await?;

        let restricted_paths_enabled = justknobs::eval(
            "scm/mononoke:enabled_restricted_paths_access_logging",
            None, // hashing
            // Adding a switch value to be able to disable writes only
            Some("hg_tree_context_new_check_exists"),
        )?;
        if restricted_paths_enabled {
            let ctx_clone = ctx.clone();
            let manifest_id = ManifestId::new(manifest_id.as_bytes().into());
            let restricted_paths = repo_ctx.repo_ctx().repo().restricted_paths_arc();

            // Spawn asynchronous task for logging restricted path access
            let _spawned_task = mononoke::spawn_task(async move {
                let _is_restricted = restricted_paths
                    .log_access_by_manifest_if_restricted(&ctx_clone, manifest_id, ManifestType::Hg)
                    .await;
            });
        }

        Ok(envelope.map(move |envelope| Self { repo_ctx, envelope }))
    }

    /// Get the content for this tree manifest node in the format expected
    /// by Mercurial's data storage layer.
    pub fn content_bytes(&self) -> Bytes {
        self.envelope.contents().clone()
    }

    pub fn into_blob_manifest(self) -> anyhow::Result<mercurial_types::blobs::HgBlobManifest> {
        mercurial_types::blobs::HgBlobManifest::parse(self.envelope)
    }
}

#[derive(Clone)]
pub struct HgAugmentedTreeContext<R> {
    #[allow(dead_code)]
    repo_ctx: HgRepoContext<R>,
    preloaded_manifest: HgPreloadedAugmentedManifest,
}

impl<R: MononokeRepo> HgAugmentedTreeContext<R> {
    /// Create a new `HgAugmentedTreeContext`, representing a single augmented tree manifest node.
    pub async fn new_check_exists(
        repo_ctx: HgRepoContext<R>,
        augmented_manifest_id: HgAugmentedManifestId,
    ) -> Result<Option<Self>, MononokeError> {
        let ctx = repo_ctx.ctx();
        let blobstore = repo_ctx.repo().repo_blobstore();
        let envelope =
            fetch_augmented_manifest_envelope_opt(ctx, blobstore, augmented_manifest_id).await?;

        let restricted_paths_enabled = justknobs::eval(
            "scm/mononoke:enabled_restricted_paths_access_logging",
            None, // hashing
            // Adding a switch value to be able to disable writes only
            Some("hg_augmented_tree_context_new_check_exists"),
        )?;
        if restricted_paths_enabled {
            let ctx_clone = ctx.clone();
            let manifest_id = ManifestId::new(augmented_manifest_id.as_bytes().into());
            let restricted_paths = repo_ctx.repo_ctx().repo().restricted_paths_arc();

            // Spawn asynchronous task for logging restricted path access
            let _spawned_task = mononoke::spawn_task(async move {
                let _is_restricted = restricted_paths
                    .log_access_by_manifest_if_restricted(
                        &ctx_clone,
                        manifest_id,
                        ManifestType::HgAugmented,
                    )
                    .await;
            });
        }

        if let Some(envelope) = envelope {
            let preloaded_manifest =
                HgPreloadedAugmentedManifest::load_from_sharded(envelope, ctx, blobstore).await?;
            Ok(Some(Self {
                repo_ctx,
                preloaded_manifest,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn augmented_manifest_id(&self) -> Blake3 {
        self.preloaded_manifest.augmented_manifest_id
    }

    pub fn augmented_manifest_size(&self) -> u64 {
        self.preloaded_manifest.augmented_manifest_size
    }

    pub fn augmented_children_entries(
        &self,
    ) -> impl Iterator<Item = &(MPathElement, HgAugmentedManifestEntry)> {
        self.preloaded_manifest.children_augmented_metadata.iter()
    }

    /// Get the content for this tree manifest node in the format expected
    /// by Mercurial's data storage layer.
    pub fn content_bytes(&self) -> Bytes {
        self.preloaded_manifest.contents.clone()
    }
}

#[async_trait]
impl<R: MononokeRepo> HgDataContext<R> for HgTreeContext<R> {
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
impl<R: MononokeRepo> HgDataContext<R> for HgAugmentedTreeContext<R> {
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
impl<R: MononokeRepo> HgDataId<R> for HgManifestId {
    type Context = HgTreeContext<R>;

    fn from_node_hash(hash: HgNodeHash) -> Self {
        HgManifestId::new(hash)
    }

    async fn context(
        self,
        repo: HgRepoContext<R>,
    ) -> Result<Option<HgTreeContext<R>>, MononokeError> {
        HgTreeContext::new_check_exists(repo, self).await
    }
}

#[async_trait]
impl<R: MononokeRepo> HgDataId<R> for HgAugmentedManifestId {
    type Context = HgAugmentedTreeContext<R>;

    fn from_node_hash(hash: HgNodeHash) -> Self {
        HgAugmentedManifestId::new(hash)
    }

    async fn context(
        self,
        repo: HgRepoContext<R>,
    ) -> Result<Option<HgAugmentedTreeContext<R>>, MononokeError> {
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
    use mononoke_macros::mononoke;

    use super::*;
    use crate::RepoContextHgExt;

    #[mononoke::fbinit_test]
    async fn test_hg_tree_context(fb: FacebookInit) -> Result<(), MononokeError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(Linear::get_repo::<Repo>(fb).await);
        let rctx = RepoContext::new_test(ctx.clone(), repo.clone()).await?;

        // Get the HgManifestId of the root tree manifest for a commit in this repo.
        // (Commit hash was found by inspecting the source of the `fixtures` crate.)
        let hg_cs_id = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
        let hg_cs = hg_cs_id.load(&ctx, rctx.repo().repo_blobstore()).await?;
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
