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
use derived_data::{BonsaiDerived, BonsaiDerivedMapping};
use futures::{stream::FuturesUnordered, TryFutureExt, TryStreamExt};
use metaconfig_types::UnodeVersion;
use mononoke_types::{
    BlobstoreBytes, BonsaiChangeset, ChangesetId, ContentId, FileType, MPath, ManifestUnodeId,
};
use repo_blobstore::RepoBlobstore;
use std::{
    collections::HashMap,
    convert::{TryFrom, TryInto},
};

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
impl BonsaiDerived for RootUnodeManifestId {
    const NAME: &'static str = "unodes";
    type Mapping = RootUnodeManifestMapping;

    fn mapping(_ctx: &CoreContext, repo: &BlobRepo) -> Self::Mapping {
        RootUnodeManifestMapping::new(
            repo.blobstore().clone(),
            repo.get_derived_data_config().unode_version,
        )
    }

    async fn derive_from_parents(
        ctx: CoreContext,
        repo: BlobRepo,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
    ) -> Result<Self, Error> {
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
        )
        .map_ok(RootUnodeManifestId)
        .await
    }
}

// TODO(stash): have a generic version of blobstore derived data mapping?
#[derive(Clone)]
pub struct RootUnodeManifestMapping {
    blobstore: RepoBlobstore,
    unode_version: UnodeVersion,
}

impl RootUnodeManifestMapping {
    pub fn new(blobstore: RepoBlobstore, unode_version: UnodeVersion) -> Self {
        Self {
            blobstore,
            unode_version,
        }
    }

    fn format_key(&self, cs_id: ChangesetId) -> String {
        match self.unode_version {
            UnodeVersion::V1 => format!("derived_root_unode.{}", cs_id),
            UnodeVersion::V2 => format!("derived_root_unode_v2.{}", cs_id),
        }
    }

    async fn fetch_unode(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Option<(ChangesetId, RootUnodeManifestId)>, Error> {
        let bytes = self
            .blobstore
            .get(ctx.clone(), self.format_key(cs_id))
            .await?;
        match bytes {
            Some(bytes) => Ok(Some((cs_id, bytes.try_into()?))),
            None => Ok(None),
        }
    }
}

#[async_trait]
impl BonsaiDerivedMapping for RootUnodeManifestMapping {
    type Value = RootUnodeManifestId;

    async fn get(
        &self,
        ctx: CoreContext,
        csids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Self::Value>, Error> {
        csids
            .into_iter()
            .map(|cs_id| self.fetch_unode(ctx.clone(), cs_id))
            .collect::<FuturesUnordered<_>>()
            .try_filter_map(|x| async move { Ok(x) })
            .try_collect()
            .await
    }

    async fn put(&self, ctx: CoreContext, csid: ChangesetId, id: Self::Value) -> Result<(), Error> {
        self.blobstore
            .put(ctx, self.format_key(csid), id.into())
            .await
    }
}

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
    use cloned::cloned;
    use derived_data_test_utils::iterate_all_manifest_entries;
    use fbinit::FacebookInit;
    use fixtures::{
        branch_even, branch_uneven, branch_wide, linear, many_diamonds, many_files_dirs,
        merge_even, merge_uneven, unshared_merge_even, unshared_merge_uneven,
    };
    use futures::{compat::Stream01CompatExt, future, Future, Stream, TryStreamExt};
    use futures_old::{Future as _, Stream as _};
    use manifest::Entry;
    use mercurial_types::{HgChangesetId, HgManifestId};
    use revset::AncestorsNodeStream;

    async fn fetch_manifest_by_cs_id(
        ctx: CoreContext,
        repo: BlobRepo,
        hg_cs_id: HgChangesetId,
    ) -> Result<HgManifestId, Error> {
        Ok(hg_cs_id.load(ctx, repo.blobstore()).await?.manifestid())
    }

    async fn verify_unode(
        ctx: CoreContext,
        repo: BlobRepo,
        bcs_id: ChangesetId,
        hg_cs_id: HgChangesetId,
    ) -> Result<(), Error> {
        let unode_entries = {
            cloned!(ctx, repo);
            async move {
                let mf_unode_id = RootUnodeManifestId::derive(&ctx, &repo, bcs_id)
                    .await?
                    .manifest_unode_id()
                    .clone();
                let mut paths = iterate_all_manifest_entries(&ctx, &repo, Entry::Tree(mf_unode_id))
                    .map_ok(|(path, _)| path)
                    .try_collect::<Vec<_>>()
                    .await?;
                paths.sort();
                Ok(paths)
            }
        };

        let filenode_entries = async move {
            let root_mf_id = fetch_manifest_by_cs_id(ctx.clone(), repo.clone(), hg_cs_id).await?;
            let mut paths = iterate_all_manifest_entries(&ctx, &repo, Entry::Tree(root_mf_id))
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
            .map(move |maybe_bcs_id| {
                let bcs_id = maybe_bcs_id.unwrap();
                AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), bcs_id.clone())
                    .and_then(move |new_bcs_id| {
                        repo.get_hg_from_bonsai_changeset(ctx.clone(), new_bcs_id)
                            .map(move |hg_cs_id| (new_bcs_id, hg_cs_id))
                    })
            })
            .flatten_stream()
            .compat()
    }

    async fn verify_repo<F>(fb: FacebookInit, repo: F)
    where
        F: Future<Output = BlobRepo>,
    {
        let ctx = CoreContext::test_mock(fb);
        let repo = repo.await;

        all_commits(ctx.clone(), repo.clone())
            .and_then(move |(bcs_id, hg_cs_id)| {
                verify_unode(ctx.clone(), repo.clone(), bcs_id, hg_cs_id)
            })
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
