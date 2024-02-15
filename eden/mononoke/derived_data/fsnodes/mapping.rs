/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use bytes::Bytes;
use context::CoreContext;
use derived_data::impl_bonsai_derived_via_manager;
use derived_data_manager::dependencies;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivableType;
use derived_data_manager::DerivationContext;
use derived_data_service_if::types as thrift;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::FileType;
use mononoke_types::FsnodeId;
use mononoke_types::NonRootMPath;

use crate::batch::derive_fsnode_in_batch;
use crate::derive::derive_fsnode;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RootFsnodeId(pub(crate) FsnodeId);

impl RootFsnodeId {
    pub fn fsnode_id(&self) -> &FsnodeId {
        &self.0
    }
    pub fn into_fsnode_id(self) -> FsnodeId {
        self.0
    }
}

impl TryFrom<BlobstoreBytes> for RootFsnodeId {
    type Error = Error;

    fn try_from(blob_bytes: BlobstoreBytes) -> Result<Self> {
        FsnodeId::from_bytes(&blob_bytes.into_bytes()).map(RootFsnodeId)
    }
}

impl TryFrom<BlobstoreGetData> for RootFsnodeId {
    type Error = Error;

    fn try_from(blob_get_data: BlobstoreGetData) -> Result<Self> {
        blob_get_data.into_bytes().try_into()
    }
}

impl From<RootFsnodeId> for BlobstoreBytes {
    fn from(root_fsnode_id: RootFsnodeId) -> Self {
        BlobstoreBytes::from_bytes(Bytes::copy_from_slice(root_fsnode_id.0.blake2().as_ref()))
    }
}

pub fn format_key(derivation_ctx: &DerivationContext, changeset_id: ChangesetId) -> String {
    let root_prefix = "derived_root_fsnode.";
    let key_prefix = derivation_ctx.mapping_key_prefix::<RootFsnodeId>();
    format!("{}{}{}", root_prefix, key_prefix, changeset_id)
}

#[async_trait]
impl BonsaiDerivable for RootFsnodeId {
    const VARIANT: DerivableType = DerivableType::Fsnodes;

    type Dependencies = dependencies![];
    type PredecessorDependencies = dependencies![];

    async fn derive_single(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
    ) -> Result<Self, Error> {
        let fsnode_id = derive_fsnode(
            ctx,
            derivation_ctx,
            parents
                .into_iter()
                .map(|root_fsnode_id| root_fsnode_id.into_fsnode_id())
                .collect(),
            get_file_changes(&bonsai),
        )
        .await?;
        Ok(RootFsnodeId(fsnode_id))
    }

    async fn derive_batch(
        ctx: &CoreContext,
        derivation_ctx: &DerivationContext,
        bonsais: Vec<BonsaiChangeset>,
    ) -> Result<HashMap<ChangesetId, Self>> {
        derive_fsnode_in_batch(
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
        if let thrift::DerivedData::fsnode(thrift::DerivedDataFsnode::root_fsnode_id(id)) = data {
            FsnodeId::from_thrift(id).map(Self)
        } else {
            Err(anyhow!(
                "Can't convert {} from provided thrift::DerivedData",
                Self::NAME.to_string(),
            ))
        }
    }

    fn into_thrift(data: Self) -> Result<thrift::DerivedData> {
        Ok(thrift::DerivedData::fsnode(
            thrift::DerivedDataFsnode::root_fsnode_id(data.into_fsnode_id().into_thrift()),
        ))
    }
}

impl_bonsai_derived_via_manager!(RootFsnodeId);

pub(crate) fn get_file_changes(
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

#[cfg(test)]
mod test {
    use blobstore::Loadable;
    use bookmarks::BookmarkKey;
    use bookmarks::BookmarksRef;
    use borrowed::borrowed;
    use changeset_fetcher::ChangesetFetcherArc;
    use derived_data::BonsaiDerived;
    use derived_data_test_utils::iterate_all_manifest_entries;
    use fbinit::FacebookInit;
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
    use futures::compat::Stream01CompatExt;
    use futures::future::Future;
    use futures::stream::Stream;
    use futures::stream::TryStreamExt;
    use futures::try_join;
    use manifest::Entry;
    use mercurial_derivation::DeriveHgChangeset;
    use mercurial_types::HgChangesetId;
    use mercurial_types::HgManifestId;
    use repo_blobstore::RepoBlobstoreRef;
    use repo_derived_data::RepoDerivedDataRef;
    use revset::AncestorsNodeStream;
    use tokio::runtime::Runtime;

    use super::*;

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

    async fn verify_fsnode(
        ctx: &CoreContext,
        repo: &(impl RepoBlobstoreRef + RepoDerivedDataRef + Send + Sync),
        bcs_id: ChangesetId,
        hg_cs_id: HgChangesetId,
    ) -> Result<()> {
        let root_fsnode_id = RootFsnodeId::derive(ctx, repo, bcs_id)
            .await?
            .into_fsnode_id();

        let fsnode_entries = iterate_all_manifest_entries(ctx, repo, Entry::Tree(root_fsnode_id))
            .map_ok(|(path, _)| path)
            .try_collect::<Vec<_>>();

        let root_mf_id = fetch_manifest_by_cs_id(ctx, repo, hg_cs_id).await?;

        let filenode_entries = iterate_all_manifest_entries(ctx, repo, Entry::Tree(root_mf_id))
            .map_ok(|(path, _)| path)
            .try_collect::<Vec<_>>();

        let (mut fsnode_entries, mut filenode_entries) =
            try_join!(fsnode_entries, filenode_entries)?;
        fsnode_entries.sort();
        filenode_entries.sort();
        assert_eq!(fsnode_entries, filenode_entries);
        Ok(())
    }

    async fn all_commits<'a>(
        ctx: &'a CoreContext,
        repo: &'a (impl BookmarksRef + ChangesetFetcherArc + RepoDerivedDataRef + Send + Sync),
    ) -> Result<impl Stream<Item = Result<(ChangesetId, HgChangesetId)>> + 'a> {
        let master_book = BookmarkKey::new("master").unwrap();
        let bcs_id = repo
            .bookmarks()
            .get(ctx.clone(), &master_book)
            .await?
            .unwrap();

        Ok(
            AncestorsNodeStream::new(ctx.clone(), &repo.changeset_fetcher_arc(), bcs_id.clone())
                .compat()
                .and_then(move |new_bcs_id| async move {
                    let hg_cs_id = repo.derive_hg_changeset(ctx, new_bcs_id).await?;
                    Ok((new_bcs_id, hg_cs_id))
                }),
        )
    }

    fn verify_repo<F, R>(fb: FacebookInit, repo: F, runtime: &Runtime)
    where
        F: Future<Output = R>,
        R: BookmarksRef + RepoBlobstoreRef + RepoDerivedDataRef + ChangesetFetcherArc + Send + Sync,
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
                        verify_fsnode(ctx, repo, bcs_id, hg_cs_id).await
                    })
                    .await
            })
            .unwrap();
    }

    #[fbinit::test]
    fn test_derive_data(fb: FacebookInit) {
        let runtime = Runtime::new().unwrap();
        verify_repo(fb, Linear::getrepo(fb), &runtime);
        verify_repo(fb, BranchEven::getrepo(fb), &runtime);
        verify_repo(fb, BranchUneven::getrepo(fb), &runtime);
        verify_repo(fb, BranchWide::getrepo(fb), &runtime);
        verify_repo(fb, ManyDiamonds::getrepo(fb), &runtime);
        verify_repo(fb, ManyFilesDirs::getrepo(fb), &runtime);
        verify_repo(fb, MergeEven::getrepo(fb), &runtime);
        verify_repo(fb, MergeUneven::getrepo(fb), &runtime);
        verify_repo(fb, UnsharedMergeEven::getrepo(fb), &runtime);
        verify_repo(fb, UnsharedMergeUneven::getrepo(fb), &runtime);
    }
}
