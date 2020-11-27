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
use futures::stream::{self, FuturesUnordered, StreamExt, TryStreamExt};
use mononoke_types::{
    BlobstoreBytes, BonsaiChangeset, ChangesetId, ContentId, FileType, FsnodeId, MPath,
};
use repo_blobstore::RepoBlobstore;

use crate::batch::derive_fsnode_in_batch;
use crate::derive::derive_fsnode;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RootFsnodeId(FsnodeId);

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

#[async_trait]
impl BonsaiDerived for RootFsnodeId {
    const NAME: &'static str = "fsnodes";
    type Mapping = RootFsnodeMapping;

    fn mapping(_ctx: &CoreContext, repo: &BlobRepo) -> Self::Mapping {
        RootFsnodeMapping::new(repo.blobstore().clone())
    }

    async fn derive_from_parents(
        ctx: CoreContext,
        repo: BlobRepo,
        bonsai: BonsaiChangeset,
        parents: Vec<Self>,
    ) -> Result<Self, Error> {
        let fsnode_id = derive_fsnode(
            &ctx,
            &repo,
            parents
                .into_iter()
                .map(|root_fsnode_id| root_fsnode_id.into_fsnode_id())
                .collect(),
            get_file_changes(&bonsai),
        )
        .await?;
        Ok(RootFsnodeId(fsnode_id))
    }

    async fn batch_derive<'a, Iter>(
        ctx: &CoreContext,
        repo: &BlobRepo,
        csids: Iter,
    ) -> Result<HashMap<ChangesetId, Self>, Error>
    where
        Iter: IntoIterator<Item = ChangesetId> + Send,
        Iter::IntoIter: Send,
    {
        let csids = csids.into_iter().collect::<Vec<_>>();
        let derived = derive_fsnode_in_batch(ctx, repo, csids.clone()).await?;

        let mapping = Self::mapping(ctx, repo);

        stream::iter(derived.into_iter().map(|(cs_id, derived)| {
            let mapping = mapping.clone();
            async move {
                let derived = RootFsnodeId(derived);
                mapping
                    .put(ctx.clone(), cs_id.clone(), derived.clone())
                    .await?;
                Ok((cs_id, derived))
            }
        }))
        .buffered(100)
        .try_collect::<HashMap<_, _>>()
        .await
    }
}

#[derive(Clone)]
pub struct RootFsnodeMapping {
    blobstore: RepoBlobstore,
}

impl RootFsnodeMapping {
    pub fn new(blobstore: RepoBlobstore) -> Self {
        Self { blobstore }
    }

    fn format_key(&self, cs_id: ChangesetId) -> String {
        format!("derived_root_fsnode.{}", cs_id)
    }

    async fn fetch_fsnode(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Option<RootFsnodeId>> {
        match self.blobstore.get(ctx, &self.format_key(cs_id)).await? {
            Some(blob) => Ok(Some(blob.try_into()?)),
            None => Ok(None),
        }
    }
}

#[async_trait]
impl BonsaiDerivedMapping for RootFsnodeMapping {
    type Value = RootFsnodeId;

    async fn get(
        &self,
        ctx: CoreContext,
        csids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Self::Value>, Error> {
        borrowed!(ctx);
        csids
            .into_iter()
            .map(|cs_id| async move {
                match self.fetch_fsnode(ctx, cs_id).await? {
                    Some(root_fsnode_id) => Ok(Some((cs_id, root_fsnode_id))),
                    None => Ok(None),
                }
            })
            .collect::<FuturesUnordered<_>>()
            .try_filter_map(|maybe_fsnode_mapping| async move { Ok(maybe_fsnode_mapping) })
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

    async fn verify_fsnode(
        ctx: &CoreContext,
        repo: &BlobRepo,
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
                        verify_fsnode(ctx, repo, bcs_id, hg_cs_id).await
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
