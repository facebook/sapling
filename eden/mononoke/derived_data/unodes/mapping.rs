/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::derive::derive_unode_manifest;
use anyhow::{Error, Result};
use async_trait::async_trait;
use blobrepo::BlobRepo;
use blobstore::{Blobstore, BlobstoreGetData};
use bytes::Bytes;
use context::CoreContext;
use derived_data::{
    impl_bonsai_derived_mapping, BlobstoreRootIdMapping, BonsaiDerivable, DerivedDataTypesConfig,
};
use futures::TryFutureExt;
use metaconfig_types::UnodeVersion;
use mononoke_types::{
    BlobstoreBytes, BonsaiChangeset, ContentId, FileType, MPath, ManifestUnodeId,
};
use repo_blobstore::RepoBlobstore;
use std::convert::{TryFrom, TryInto};

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RootUnodeManifestId(ManifestUnodeId);

impl RootUnodeManifestId {
    pub fn manifest_unode_id(&self) -> &ManifestUnodeId {
        &self.0
    }
}

impl TryFrom<BlobstoreBytes> for RootUnodeManifestId {
    type Error = Error;

    fn try_from(blob_bytes: BlobstoreBytes) -> Result<Self> {
        ManifestUnodeId::from_bytes(&blob_bytes.into_bytes()).map(RootUnodeManifestId)
    }
}

impl TryFrom<BlobstoreGetData> for RootUnodeManifestId {
    type Error = Error;

    fn try_from(blob_val: BlobstoreGetData) -> Result<Self> {
        blob_val.into_bytes().try_into()
    }
}

impl From<RootUnodeManifestId> for BlobstoreBytes {
    fn from(root_mf_id: RootUnodeManifestId) -> Self {
        BlobstoreBytes::from_bytes(Bytes::copy_from_slice(root_mf_id.0.blake2().as_ref()))
    }
}

#[async_trait]
impl BonsaiDerivable for RootUnodeManifestId {
    const NAME: &'static str = "unodes";

    type Options = UnodeVersion;

    async fn derive_from_parents(
        ctx: CoreContext,
        repo: BlobRepo,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
        options: &Self::Options,
    ) -> Result<Self, Error> {
        let unode_version = *options;
        let bcs_id = bonsai.get_changeset_id();
        derive_unode_manifest(
            ctx,
            repo,
            bcs_id,
            parents
                .into_iter()
                .map(|root_mf_id| root_mf_id.manifest_unode_id().clone())
                .collect(),
            get_file_changes(&bonsai),
            unode_version,
        )
        .map_ok(RootUnodeManifestId)
        .await
    }
}

#[derive(Clone)]
pub struct RootUnodeManifestMapping {
    blobstore: RepoBlobstore,
    unode_version: UnodeVersion,
}

#[async_trait]
impl BlobstoreRootIdMapping for RootUnodeManifestMapping {
    type Value = RootUnodeManifestId;

    fn new(repo: &BlobRepo, config: &DerivedDataTypesConfig) -> Result<Self> {
        Ok(Self {
            blobstore: repo.get_blobstore(),
            unode_version: config.unode_version,
        })
    }

    fn blobstore(&self) -> &dyn Blobstore {
        &self.blobstore
    }

    fn prefix(&self) -> &'static str {
        match self.unode_version {
            UnodeVersion::V1 => "derived_root_unode.",
            UnodeVersion::V2 => "derived_root_unode_v2.",
        }
    }

    fn options(&self) -> UnodeVersion {
        self.unode_version
    }
}

impl_bonsai_derived_mapping!(
    RootUnodeManifestMapping,
    BlobstoreRootIdMapping,
    RootUnodeManifestId
);

pub(crate) fn get_file_changes(
    bcs: &BonsaiChangeset,
) -> Vec<(MPath, Option<(ContentId, FileType)>)> {
    bcs.file_changes()
        .map(|(mpath, maybe_file_change)| {
            let content_file_type = match maybe_file_change {
                Some(file_change) => Some((file_change.content_id(), file_change.file_type())),
                None => None,
            };
            (mpath.clone(), content_file_type)
        })
        .collect()
}

#[cfg(test)]
mod test {
    use super::*;
    use blobrepo_hg::BlobRepoHg;
    use blobstore::Loadable;
    use bookmarks::BookmarkName;
    use borrowed::borrowed;
    use derived_data::BonsaiDerived;
    use derived_data_test_utils::iterate_all_manifest_entries;
    use fbinit::FacebookInit;
    use fixtures::{
        branch_even, branch_uneven, branch_wide, linear, many_diamonds, many_files_dirs,
        merge_even, merge_uneven, unshared_merge_even, unshared_merge_uneven,
    };
    use futures::{
        compat::{Future01CompatExt, Stream01CompatExt},
        future, Future, Stream, TryStreamExt,
    };
    use manifest::Entry;
    use mercurial_types::{HgChangesetId, HgManifestId};
    use mononoke_types::ChangesetId;
    use revset::AncestorsNodeStream;

    async fn fetch_manifest_by_cs_id(
        ctx: &CoreContext,
        repo: &BlobRepo,
        hg_cs_id: HgChangesetId,
    ) -> Result<HgManifestId, Error> {
        Ok(hg_cs_id.load(ctx, repo.blobstore()).await?.manifestid())
    }

    async fn verify_unode(
        ctx: &CoreContext,
        repo: &BlobRepo,
        bcs_id: ChangesetId,
        hg_cs_id: HgChangesetId,
    ) -> Result<(), Error> {
        let unode_entries = {
            async move {
                let mf_unode_id = RootUnodeManifestId::derive(ctx, repo, bcs_id)
                    .await?
                    .manifest_unode_id()
                    .clone();
                let mut paths = iterate_all_manifest_entries(ctx, repo, Entry::Tree(mf_unode_id))
                    .map_ok(|(path, _)| path)
                    .try_collect::<Vec<_>>()
                    .await?;
                paths.sort();
                Ok(paths)
            }
        };

        let filenode_entries = async move {
            let root_mf_id = fetch_manifest_by_cs_id(ctx, repo, hg_cs_id).await?;
            let mut paths = iterate_all_manifest_entries(ctx, repo, Entry::Tree(root_mf_id))
                .map_ok(|(path, _)| path)
                .try_collect::<Vec<_>>()
                .await?;
            paths.sort();
            Ok(paths)
        };

        future::try_join(unode_entries, filenode_entries)
            .map_ok(|(unode_entries, filenode_entries)| {
                assert_eq!(unode_entries, filenode_entries);
            })
            .await
    }

    fn all_commits(
        ctx: CoreContext,
        repo: BlobRepo,
    ) -> impl Stream<Item = Result<(ChangesetId, HgChangesetId), Error>> {
        let master_book = BookmarkName::new("master").unwrap();
        repo.get_bonsai_bookmark(ctx.clone(), &master_book)
            .map_ok(move |maybe_bcs_id| {
                let bcs_id = maybe_bcs_id.unwrap();
                AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), bcs_id.clone())
                    .compat()
                    .and_then(move |new_bcs_id| {
                        repo.get_hg_from_bonsai_changeset(ctx.clone(), new_bcs_id)
                            .compat()
                            .map_ok(move |hg_cs_id| (new_bcs_id, hg_cs_id))
                    })
            })
            .try_flatten_stream()
    }

    async fn verify_repo<F>(fb: FacebookInit, repo: F)
    where
        F: Future<Output = BlobRepo>,
    {
        let ctx = CoreContext::test_mock(fb);
        let repo = repo.await;
        borrowed!(ctx, repo);

        all_commits(ctx.clone(), repo.clone())
            .and_then(move |(bcs_id, hg_cs_id)| verify_unode(&ctx, &repo, bcs_id, hg_cs_id))
            .try_collect::<Vec<_>>()
            .await
            .unwrap();
    }

    #[fbinit::test]
    async fn test_derive_data(fb: FacebookInit) {
        verify_repo(fb, linear::getrepo(fb)).await;
        verify_repo(fb, branch_even::getrepo(fb)).await;
        verify_repo(fb, branch_uneven::getrepo(fb)).await;
        verify_repo(fb, branch_wide::getrepo(fb)).await;
        verify_repo(fb, many_diamonds::getrepo(fb)).await;
        verify_repo(fb, many_files_dirs::getrepo(fb)).await;
        verify_repo(fb, merge_even::getrepo(fb)).await;
        verify_repo(fb, merge_uneven::getrepo(fb)).await;
        verify_repo(fb, unshared_merge_even::getrepo(fb)).await;
        verify_repo(fb, unshared_merge_uneven::getrepo(fb)).await;
    }
}
