/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};

use anyhow::{Error, Result};
use async_trait::async_trait;
use blobrepo::BlobRepo;
use blobstore::{Blobstore, BlobstoreGetData};
use borrowed::borrowed;
use bytes::Bytes;
use context::CoreContext;
use derived_data::{BonsaiDerived, BonsaiDerivedMapping};
use futures::stream::{FuturesUnordered, TryStreamExt};
use mononoke_types::{
    BlobstoreBytes, BonsaiChangeset, ChangesetId, ContentId, FileType, MPath, SkeletonManifestId,
};
use repo_blobstore::RepoBlobstore;

use crate::derive::derive_skeleton_manifest;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RootSkeletonManifestId(SkeletonManifestId);

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
        SkeletonManifestId::from_bytes(&blob_bytes.into_bytes()).map(RootSkeletonManifestId)
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

#[async_trait]
impl BonsaiDerived for RootSkeletonManifestId {
    const NAME: &'static str = "skeleton_manifests";
    type Mapping = RootSkeletonManifestMapping;

    fn mapping(_ctx: &CoreContext, repo: &BlobRepo) -> Self::Mapping {
        RootSkeletonManifestMapping::new(repo.blobstore().clone())
    }

    async fn derive_from_parents(
        ctx: CoreContext,
        repo: BlobRepo,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
    ) -> Result<Self, Error> {
        let skeleton_manifest_id = derive_skeleton_manifest(
            &ctx,
            &repo,
            parents
                .into_iter()
                .map(RootSkeletonManifestId::into_skeleton_manifest_id)
                .collect(),
            get_file_changes(&bonsai),
        )
        .await?;
        Ok(RootSkeletonManifestId(skeleton_manifest_id))
    }
}

#[derive(Clone)]
pub struct RootSkeletonManifestMapping {
    blobstore: RepoBlobstore,
}

impl RootSkeletonManifestMapping {
    pub fn new(blobstore: RepoBlobstore) -> Self {
        Self { blobstore }
    }

    fn format_key(&self, cs_id: ChangesetId) -> String {
        format!("derived_root_skeletonmanifest.{}", cs_id)
    }

    async fn fetch_skeleton_manifest(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Option<RootSkeletonManifestId>> {
        match self.blobstore.get(ctx, &self.format_key(cs_id)).await? {
            Some(blob) => Ok(Some(blob.try_into()?)),
            None => Ok(None),
        }
    }
}

#[async_trait]
impl BonsaiDerivedMapping for RootSkeletonManifestMapping {
    type Value = RootSkeletonManifestId;

    async fn get(
        &self,
        ctx: CoreContext,
        csids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Self::Value>, Error> {
        borrowed!(ctx);
        csids
            .into_iter()
            .map(|cs_id| async move {
                match self.fetch_skeleton_manifest(ctx, cs_id).await? {
                    Some(root_skeleton_manifest_id) => Ok(Some((cs_id, root_skeleton_manifest_id))),
                    None => Ok(None),
                }
            })
            .collect::<FuturesUnordered<_>>()
            .try_filter_map(|maybe_skeleton_manifest_mapping| async move {
                Ok(maybe_skeleton_manifest_mapping)
            })
            .try_collect()
            .await
    }

    async fn put(&self, ctx: CoreContext, csid: ChangesetId, id: Self::Value) -> Result<(), Error> {
        self.blobstore
            .put(&ctx, self.format_key(csid), id.into())
            .await
    }
}

pub(crate) fn get_file_changes(
    bcs: &BonsaiChangeset,
) -> Vec<(MPath, Option<(ContentId, FileType)>)> {
    bcs.file_changes()
        .map(|(mpath, file_change)| {
            (
                mpath.clone(),
                file_change.map(|file_change| (file_change.content_id(), file_change.file_type())),
            )
        })
        .collect()
}

#[cfg(test)]
mod test {
    use super::*;
    use blobrepo_hg::BlobRepoHg;
    use blobstore::Loadable;
    use bookmarks::BookmarkName;
    use derived_data_test_utils::iterate_all_manifest_entries;
    use fbinit::FacebookInit;
    use fixtures::{
        branch_even, branch_uneven, branch_wide, linear, many_diamonds, many_files_dirs,
        merge_even, merge_uneven, unshared_merge_even, unshared_merge_uneven,
    };
    use futures::compat::{Future01CompatExt, Stream01CompatExt};
    use futures::future::Future;
    use futures::stream::Stream;
    use futures::try_join;
    use manifest::Entry;
    use mercurial_types::{HgChangesetId, HgManifestId};
    use revset::AncestorsNodeStream;
    use tokio_compat::runtime::Runtime;

    async fn fetch_manifest_by_cs_id(
        ctx: &CoreContext,
        repo: &BlobRepo,
        hg_cs_id: HgChangesetId,
    ) -> Result<HgManifestId> {
        Ok(hg_cs_id.load(ctx, repo.blobstore()).await?.manifestid())
    }

    async fn verify_skeleton_manifest(
        ctx: &CoreContext,
        repo: &BlobRepo,
        bcs_id: ChangesetId,
        hg_cs_id: HgChangesetId,
    ) -> Result<()> {
        let root_skeleton_manifest_id = RootSkeletonManifestId::derive(ctx, repo, bcs_id)
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
        repo: &'a BlobRepo,
    ) -> Result<impl Stream<Item = Result<(ChangesetId, HgChangesetId)>> + 'a> {
        let master_book = BookmarkName::new("master").unwrap();
        let bcs_id = repo
            .get_bonsai_bookmark(ctx.clone(), &master_book)
            .await?
            .unwrap();

        Ok(
            AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), bcs_id.clone())
                .compat()
                .and_then(move |new_bcs_id| async move {
                    let hg_cs_id = repo
                        .get_hg_from_bonsai_changeset(ctx.clone(), new_bcs_id)
                        .compat()
                        .await?;
                    Ok((new_bcs_id, hg_cs_id))
                }),
        )
    }

    fn verify_repo<F>(fb: FacebookInit, repo: F, runtime: &mut Runtime)
    where
        F: Future<Output = BlobRepo>,
    {
        let ctx = CoreContext::test_mock(fb);
        let repo = runtime.block_on_std(repo);
        borrowed!(ctx, repo);

        runtime
            .block_on_std(async move {
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

    #[fbinit::test]
    fn test_derive_data(fb: FacebookInit) {
        let mut runtime = Runtime::new().unwrap();
        verify_repo(fb, linear::getrepo(fb), &mut runtime);
        verify_repo(fb, branch_even::getrepo(fb), &mut runtime);
        verify_repo(fb, branch_uneven::getrepo(fb), &mut runtime);
        verify_repo(fb, branch_wide::getrepo(fb), &mut runtime);
        verify_repo(fb, many_diamonds::getrepo(fb), &mut runtime);
        verify_repo(fb, many_files_dirs::getrepo(fb), &mut runtime);
        verify_repo(fb, merge_even::getrepo(fb), &mut runtime);
        verify_repo(fb, merge_uneven::getrepo(fb), &mut runtime);
        verify_repo(fb, unshared_merge_even::getrepo(fb), &mut runtime);
        verify_repo(fb, unshared_merge_uneven::getrepo(fb), &mut runtime);
    }
}
