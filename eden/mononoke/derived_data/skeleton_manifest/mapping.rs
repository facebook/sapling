/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivationContext;
use derived_data_manager::dependencies;
use derived_data_service_if as thrift;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use manifest::ManifestOps;
use manifest::ManifestParentReplacement;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use mononoke_types::SkeletonManifestId;

use crate::batch::derive_skeleton_manifests_in_batch;
use crate::derive::derive_skeleton_manifest_with_subtree_changes;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RootSkeletonManifestId(pub(crate) SkeletonManifestId);

impl RootSkeletonManifestId {
    pub fn skeleton_manifest_id(&self) -> &SkeletonManifestId {
        &self.0
    }
    pub fn into_skeleton_manifest_id(self) -> SkeletonManifestId {
        self.0
    }
}

impl TryFrom<BlobstoreBytes> for RootSkeletonManifestId {
    type Error = Error;

    fn try_from(blob_bytes: BlobstoreBytes) -> Result<Self> {
        SkeletonManifestId::from_bytes(blob_bytes.into_bytes()).map(RootSkeletonManifestId)
    }
}

impl TryFrom<BlobstoreGetData> for RootSkeletonManifestId {
    type Error = Error;

    fn try_from(blob_get_data: BlobstoreGetData) -> Result<Self> {
        blob_get_data.into_bytes().try_into()
    }
}

impl From<RootSkeletonManifestId> for BlobstoreBytes {
    fn from(root_skeleton_manifest_id: RootSkeletonManifestId) -> Self {
        BlobstoreBytes::from_bytes(Bytes::copy_from_slice(
            root_skeleton_manifest_id.0.blake2().as_ref(),
        ))
    }
}

pub fn format_key(derivation_ctx: &DerivationContext, changeset_id: ChangesetId) -> String {
    let root_prefix = "derived_root_skeletonmanifest.";
    let key_prefix = derivation_ctx.mapping_key_prefix::<RootSkeletonManifestId>();
    format!("{}{}{}", root_prefix, key_prefix, changeset_id)
}

#[async_trait]
impl BonsaiDerivable for RootSkeletonManifestId {
    const VARIANT: DerivableType = DerivableType::SkeletonManifests;

    type Dependencies = dependencies![];
    type PredecessorDependencies = dependencies![];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
        known: Option<&HashMap<ChangesetId, Self>>,
    ) -> Result<Self, Error> {
        let subtree_changes =
            get_skeleton_manifest_subtree_changes(ctx, derivation_ctx, known, &bonsai).await?;
        let id = derive_skeleton_manifest_with_subtree_changes(
            ctx,
            derivation_ctx,
            parents
                .into_iter()
                .map(RootSkeletonManifestId::into_skeleton_manifest_id)
                .collect(),
            get_file_changes(&bonsai),
            subtree_changes,
        )
        .await?;
        Ok(RootSkeletonManifestId(id))
    }

    async fn derive_batch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsais: Vec<BonsaiChangeset>,
    ) -> Result<HashMap<ChangesetId, Self>> {
        derive_skeleton_manifests_in_batch(
            ctx,
            derivation_ctx,
            bonsais.into_iter().map(|b| b.get_changeset_id()).collect(),
        )
        .await
    }

    async fn store_mapping(
        self,
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<()> {
        let key = format_key(derivation_ctx, changeset_id);
        derivation_ctx.blobstore().put(ctx, key, self.into()).await
    }

    async fn fetch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        changeset_id: ChangesetId,
    ) -> Result<Option<Self>> {
        let key = format_key(derivation_ctx, changeset_id);
        Ok(derivation_ctx
            .blobstore()
            .get(ctx, &key)
            .await?
            .map(TryInto::try_into)
            .transpose()?)
    }

    fn from_thrift(data: thrift::DerivedData) -> Result<Self> {
        if let thrift::DerivedData::skeleton_manifest(
            thrift::DerivedDataSkeletonManifest::root_skeleton_manifest_id(id),
        ) = data
        {
            SkeletonManifestId::from_thrift(id).map(Self)
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME.to_string(),
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::skeleton_manifest(
            thrift::DerivedDataSkeletonManifest::root_skeleton_manifest_id(
                data.skeleton_manifest_id().into_thrift(),
            ),
        ))
    }
}

pub fn get_file_changes(
    bcs: &BonsaiChangeset,
) -> Vec<(NonRootMPath, Option<(ContentId, FileType)>)> {
    bcs.file_changes()
        .map(|(mpath, file_change)| {
            (
                mpath.clone(),
                file_change
                    .simplify()
                    .map(|bc| (bc.content_id(), bc.file_type())),
            )
        })
        .collect()
}

pub async fn get_skeleton_manifest_subtree_changes(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    known: Option<&HashMap<ChangesetId, RootSkeletonManifestId>>,
    bcs: &BonsaiChangeset,
) -> Result<Vec<ManifestParentReplacement<SkeletonManifestId, ()>>> {
    let copy_sources = bcs
        .subtree_changes()
        .iter()
        .filter_map(|(path, change)| {
            let (from_cs_id, from_path) = change.copy_source()?;
            Some((path, from_cs_id, from_path))
        })
        .collect::<Vec<_>>();
    stream::iter(copy_sources)
        .map(|(path, from_cs_id, from_path)| {
            cloned!(ctx);
            let blobstore = derivation_ctx.blobstore().clone();
            async move {
                let root = derivation_ctx
                    .fetch_unknown_dependency::<RootSkeletonManifestId>(&ctx, known, from_cs_id)
                    .await?
                    .into_skeleton_manifest_id();
                let entry = root
                    .find_entry(ctx, blobstore, from_path.clone())
                    .await?
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Subtree copy source {} does not exist in {}",
                            from_path,
                            from_cs_id
                        )
                    })?;
                Ok(ManifestParentReplacement {
                    path: path.clone(),
                    replacements: vec![entry],
                })
            }
        })
        .buffered(100)
        .try_collect()
        .boxed()
        .await
}

#[cfg(test)]
mod test {
    use blobstore::Loadable;
    use bonsai_hg_mapping::BonsaiHgMapping;
    use bookmarks::BookmarkKey;
    use bookmarks::Bookmarks;
    use bookmarks::BookmarksRef;
    use borrowed::borrowed;
    use commit_graph::CommitGraph;
    use commit_graph::CommitGraphRef;
    use commit_graph::CommitGraphWriter;
    use derived_data_test_utils::iterate_all_manifest_entries;
    use fbinit::FacebookInit;
    use filestore::FilestoreConfig;
    use fixtures::BranchEven;
    use fixtures::BranchUneven;
    use fixtures::BranchWide;
    use fixtures::Linear;
    use fixtures::ManyDiamonds;
    use fixtures::ManyFilesDirs;
    use fixtures::MergeEven;
    use fixtures::MergeUneven;
    use fixtures::TestRepoFixture;
    use fixtures::UnsharedMergeEven;
    use fixtures::UnsharedMergeUneven;
    use futures::future::Future;
    use futures::stream::Stream;
    use futures::stream::TryStreamExt;
    use futures::try_join;
    use manifest::Entry;
    use mercurial_derivation::DeriveHgChangeset;
    use mercurial_types::HgChangesetId;
    use mercurial_types::HgManifestId;
    use mononoke_macros::mononoke;
    use mononoke_types::ChangesetId;
    use repo_blobstore::RepoBlobstore;
    use repo_blobstore::RepoBlobstoreRef;
    use repo_derived_data::RepoDerivedData;
    use repo_derived_data::RepoDerivedDataRef;
    use repo_identity::RepoIdentity;
    use tokio::runtime::Runtime;

    use super::*;

    #[facet::container]
    struct TestRepo(
        dyn BonsaiHgMapping,
        dyn Bookmarks,
        CommitGraph,
        dyn CommitGraphWriter,
        RepoDerivedData,
        RepoBlobstore,
        FilestoreConfig,
        RepoIdentity,
    );

    async fn fetch_manifest_by_cs_id(
        ctx: &CoreContext,
        repo: &impl RepoBlobstoreRef,
        hg_cs_id: HgChangesetId,
    ) -> Result<HgManifestId> {
        Ok(hg_cs_id
            .load(ctx, repo.repo_blobstore())
            .await?
            .manifestid())
    }

    async fn verify_skeleton_manifest(
        ctx: &CoreContext,
        repo: &(impl RepoDerivedDataRef + RepoBlobstoreRef + Send + Sync),
        bcs_id: ChangesetId,
        hg_cs_id: HgChangesetId,
    ) -> Result<()> {
        let manager = repo.repo_derived_data().manager();
        let root_skeleton_manifest_id = manager
            .derive::<RootSkeletonManifestId>(ctx, bcs_id, None)
            .await?
            .into_skeleton_manifest_id();

        let skeleton_manifest_entries =
            iterate_all_manifest_entries(ctx, repo, Entry::Tree(root_skeleton_manifest_id))
                .map_ok(|(path, _)| path)
                .try_collect::<Vec<_>>();

        let root_mf_id = fetch_manifest_by_cs_id(ctx, repo, hg_cs_id).await?;

        let filenode_entries = iterate_all_manifest_entries(ctx, repo, Entry::Tree(root_mf_id))
            .map_ok(|(path, _)| path)
            .try_collect::<Vec<_>>();

        let (mut skeleton_manifest_entries, mut filenode_entries) =
            try_join!(skeleton_manifest_entries, filenode_entries)?;
        skeleton_manifest_entries.sort();
        filenode_entries.sort();
        assert_eq!(skeleton_manifest_entries, filenode_entries);
        Ok(())
    }

    async fn all_commits<'a>(
        ctx: &'a CoreContext,
        repo: &'a (impl BookmarksRef + CommitGraphRef + RepoDerivedDataRef + Send + Sync),
    ) -> Result<impl Stream<Item = Result<(ChangesetId, HgChangesetId)>> + 'a> {
        let master_book = BookmarkKey::new("master").unwrap();
        let bcs_id = repo
            .bookmarks()
            .get(ctx.clone(), &master_book, bookmarks::Freshness::MostRecent)
            .await?
            .unwrap();

        Ok(repo
            .commit_graph()
            .ancestors_difference_stream(ctx, vec![bcs_id], vec![])
            .await?
            .and_then(move |new_bcs_id| async move {
                let hg_cs_id = repo.derive_hg_changeset(ctx, new_bcs_id).await?;
                Ok((new_bcs_id, hg_cs_id))
            }))
    }

    fn verify_repo<F>(fb: FacebookInit, repo: F, runtime: &Runtime)
    where
        F: Future<Output = TestRepo>,
    {
        let ctx = CoreContext::test_mock(fb);
        let repo = runtime.block_on(repo);
        borrowed!(ctx, repo);

        runtime
            .block_on(async move {
                all_commits(ctx, repo)
                    .await
                    .unwrap()
                    .try_for_each(move |(bcs_id, hg_cs_id)| async move {
                        verify_skeleton_manifest(ctx, repo, bcs_id, hg_cs_id).await
                    })
                    .await
            })
            .unwrap();
    }

    #[mononoke::fbinit_test]
    fn test_derive_data(fb: FacebookInit) {
        let runtime = Runtime::new().unwrap();
        verify_repo(fb, Linear::get_repo(fb), &runtime);
        verify_repo(fb, BranchEven::get_repo(fb), &runtime);
        verify_repo(fb, BranchUneven::get_repo(fb), &runtime);
        verify_repo(fb, BranchWide::get_repo(fb), &runtime);
        verify_repo(fb, ManyDiamonds::get_repo(fb), &runtime);
        verify_repo(fb, ManyFilesDirs::get_repo(fb), &runtime);
        verify_repo(fb, MergeEven::get_repo(fb), &runtime);
        verify_repo(fb, MergeUneven::get_repo(fb), &runtime);
        verify_repo(fb, UnsharedMergeEven::get_repo(fb), &runtime);
        verify_repo(fb, UnsharedMergeUneven::get_repo(fb), &runtime);
    }
}
