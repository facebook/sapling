/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod bonsai_generation;
mod create_changeset;
pub mod repo_commit;
pub use crate::bonsai_generation::create_bonsai_changeset_object;
pub use crate::bonsai_generation::save_bonsai_changeset_object;
pub use crate::repo_commit::ChangesetHandle;
pub use changeset_fetcher::ChangesetFetcher;
// TODO: This is exported for testing - is this the right place for it?
pub use crate::repo_commit::compute_changed_files;
pub use crate::repo_commit::UploadEntries;
pub mod errors {
    pub use blobrepo_errors::*;
}
pub use create_changeset::create_bonsai_changeset_hook;
pub use create_changeset::CreateChangeset;
pub mod file_history {
    pub use blobrepo_common::file_history::*;
}

use anyhow::Error;
use async_trait::async_trait;
use blobrepo_errors::ErrorKind;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bonsai_hg_mapping::BonsaiOrHgChangesetIds;
use bookmarks::Bookmark;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkName;
use bookmarks::BookmarkPagination;
use bookmarks::BookmarkPrefix;
use bookmarks::BookmarksRef;
use bookmarks::Freshness;
use changesets::ChangesetsRef;
use cloned::cloned;
use context::CoreContext;
use filenodes::FilenodeInfo;
use filenodes::FilenodeRangeResult;
use filenodes::FilenodeResult;
use filenodes::FilenodesRef;
use futures::future;
use futures::stream;
use futures::stream::BoxStream;
use futures::Stream;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use mercurial_derived_data::DeriveHgChangeset;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mononoke_types::ChangesetId;
use mononoke_types::RepoPath;
use repo_derived_data::RepoDerivedDataRef;
use stats::prelude::*;
use std::collections::HashMap;
use std::collections::HashSet;

/// `BlobRepoHg` is an extension trait for repo facet containers which contains
/// mercurial specific methods.
#[async_trait]
pub trait BlobRepoHg: Send + Sync {
    async fn get_hg_bonsai_mapping<'a>(
        &'a self,
        ctx: CoreContext,
        bonsai_or_hg_cs_ids: impl Into<BonsaiOrHgChangesetIds> + 'a + Send,
    ) -> Result<Vec<(HgChangesetId, ChangesetId)>, Error>
    where
        Self: ChangesetsRef + RepoDerivedDataRef + BonsaiHgMappingRef;

    fn get_heads_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> BoxStream<'_, Result<HgChangesetId, Error>>
    where
        Self: BookmarksRef + RepoDerivedDataRef + Send + Sync;

    async fn changeset_exists(
        &self,
        ctx: CoreContext,
        changesetid: HgChangesetId,
    ) -> Result<bool, Error>
    where
        Self: BonsaiHgMappingRef + ChangesetsRef;

    async fn get_changeset_parents(
        &self,
        ctx: CoreContext,
        changesetid: HgChangesetId,
    ) -> Result<Vec<HgChangesetId>, Error>
    where
        Self: BonsaiHgMappingRef + ChangesetsRef + RepoDerivedDataRef;

    async fn get_bookmark(
        &self,
        ctx: CoreContext,
        name: &BookmarkName,
    ) -> Result<Option<HgChangesetId>, Error>
    where
        Self: BookmarksRef + RepoDerivedDataRef;

    fn get_pull_default_bookmarks_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> BoxStream<'_, Result<(Bookmark, HgChangesetId), Error>>
    where
        Self: ChangesetsRef
            + BonsaiHgMappingRef
            + RepoDerivedDataRef
            + BlobRepoHg
            + BookmarksRef
            + Clone
            + Send
            + Sync;

    fn get_publishing_bookmarks_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> BoxStream<'_, Result<(Bookmark, HgChangesetId), Error>>
    where
        Self: ChangesetsRef
            + BonsaiHgMappingRef
            + RepoDerivedDataRef
            + BookmarksRef
            + Clone
            + Send
            + Sync;

    fn get_bookmarks_by_prefix_maybe_stale(
        &self,
        ctx: CoreContext,
        prefix: &BookmarkPrefix,
        max: u64,
    ) -> BoxStream<'_, Result<(Bookmark, HgChangesetId), Error>>
    where
        Self: ChangesetsRef
            + BonsaiHgMappingRef
            + RepoDerivedDataRef
            + BlobRepoHg
            + BookmarksRef
            + Clone
            + Send
            + Sync;

    async fn get_filenode_opt(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        node: HgFileNodeId,
    ) -> Result<FilenodeResult<Option<FilenodeInfo>>, Error>
    where
        Self: FilenodesRef;

    async fn get_filenode(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        node: HgFileNodeId,
    ) -> Result<FilenodeResult<FilenodeInfo>, Error>
    where
        Self: FilenodesRef;

    async fn get_all_filenodes_maybe_stale(
        &self,
        ctx: CoreContext,
        path: RepoPath,
        limit: Option<u64>,
    ) -> Result<FilenodeRangeResult<Vec<FilenodeInfo>>, Error>
    where
        Self: FilenodesRef;
}

define_stats! {
    prefix = "mononoke.blobrepo";
    changeset_exists: timeseries(Rate, Sum),
    get_all_filenodes: timeseries(Rate, Sum),
    get_bookmark: timeseries(Rate, Sum),
    get_bookmarks_by_prefix_maybe_stale: timeseries(Rate, Sum),
    get_changeset_parents: timeseries(Rate, Sum),
    get_heads_maybe_stale: timeseries(Rate, Sum),
    get_hg_bonsai_mapping: timeseries(Rate, Sum),
    get_publishing_bookmarks_maybe_stale: timeseries(Rate, Sum),
    get_pull_default_bookmarks_maybe_stale: timeseries(Rate, Sum),
}

#[async_trait]
impl<T: ChangesetsRef + BonsaiHgMappingRef + Send + Sync> BlobRepoHg for T {
    // Returns only the mapping for valid changests that are known to the server.
    // For Bonsai -> Hg conversion, missing Hg changesets will be derived (so all Bonsais will be
    // in the output).
    // For Hg -> Bonsai conversion, missing Bonsais will not be returned, since they cannot be
    // derived from Hg Changesets.
    async fn get_hg_bonsai_mapping<'a>(
        &'a self,
        ctx: CoreContext,
        bonsai_or_hg_cs_ids: impl Into<BonsaiOrHgChangesetIds> + 'a + Send,
    ) -> Result<Vec<(HgChangesetId, ChangesetId)>, Error>
    where
        Self: ChangesetsRef + RepoDerivedDataRef + BonsaiHgMappingRef,
    {
        STATS::get_hg_bonsai_mapping.add_value(1);

        let bonsai_or_hg_cs_ids = bonsai_or_hg_cs_ids.into();
        let hg_bonsai_list = self
            .bonsai_hg_mapping()
            .get(&ctx, bonsai_or_hg_cs_ids.clone())
            .await?
            .into_iter()
            .map(|entry| (entry.hg_cs_id, entry.bcs_id))
            .collect::<Vec<_>>();

        use BonsaiOrHgChangesetIds::*;
        match bonsai_or_hg_cs_ids {
            Bonsai(bonsais) => {
                // If a bonsai commit doesn't exist in the bonsai_hg_mapping,
                // that might mean two things: 1) Bonsai commit just doesn't exist
                // 2) Bonsai commit exists but hg changesets weren't generated for it
                // Normally the callers of get_hg_bonsai_mapping would expect that hg
                // changesets will be lazily generated, so the
                // code below explicitly checks if a commit exists and if yes then
                // generates hg changeset for it.
                let mapping: HashMap<_, _> = hg_bonsai_list
                    .iter()
                    .map(|(hg_id, bcs_id)| (bcs_id, hg_id))
                    .collect();

                let mut notfound = vec![];
                for b in bonsais {
                    if !mapping.contains_key(&b) {
                        notfound.push(b);
                    }
                }

                let existing: HashSet<_> = self
                    .changesets()
                    .get_many(ctx.clone(), notfound.clone())
                    .await?
                    .into_iter()
                    .map(|entry| entry.cs_id)
                    .collect();

                let mut newmapping: Vec<_> = stream::iter(
                    notfound
                        .into_iter()
                        .filter(|csid| existing.contains(csid))
                        .map(Ok),
                )
                .map_ok(|csid| {
                    self.derive_hg_changeset(&ctx, csid)
                        .map_ok(move |hgcsid| (hgcsid, csid))
                })
                .try_buffer_unordered(100)
                .try_collect()
                .await?;

                newmapping.extend(hg_bonsai_list);
                Ok(newmapping)
            }
            Hg(_) => Ok(hg_bonsai_list),
        }
        // TODO(stash, luk): T37303879 also need to check that entries exist in changeset table
    }

    /// Get Mercurial heads, which we approximate as publishing Bonsai Bookmarks.
    fn get_heads_maybe_stale(&self, ctx: CoreContext) -> BoxStream<'_, Result<HgChangesetId, Error>>
    where
        Self: BookmarksRef + RepoDerivedDataRef + Send + Sync,
    {
        STATS::get_heads_maybe_stale.add_value(1);

        self.bookmarks()
            .list(
                ctx.clone(),
                Freshness::MaybeStale,
                &BookmarkPrefix::empty(),
                BookmarkKind::ALL_PUBLISHING,
                &BookmarkPagination::FromStart,
                std::u64::MAX,
            )
            .map_ok({
                let repo = self.clone();
                move |(_, cs)| {
                    cloned!(ctx);
                    async move { repo.derive_hg_changeset(&ctx, cs).await }
                }
            })
            .try_buffer_unordered(100)
            .boxed()
    }

    async fn changeset_exists(
        &self,
        ctx: CoreContext,
        changesetid: HgChangesetId,
    ) -> Result<bool, Error>
    where
        Self: BonsaiHgMappingRef + ChangesetsRef,
    {
        STATS::changeset_exists.add_value(1);
        let csid = self
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(&ctx, changesetid)
            .await?;
        match csid {
            Some(bonsai) => {
                let res = self.changesets().get(ctx, bonsai).await?;
                Ok(res.is_some())
            }
            None => Ok(false),
        }
    }

    async fn get_changeset_parents(
        &self,
        ctx: CoreContext,
        changesetid: HgChangesetId,
    ) -> Result<Vec<HgChangesetId>, Error>
    where
        Self: BonsaiHgMappingRef + ChangesetsRef + RepoDerivedDataRef,
    {
        STATS::get_changeset_parents.add_value(1);

        let csid = self
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(&ctx, changesetid)
            .await?
            .ok_or(ErrorKind::BonsaiMappingNotFound(changesetid))?;

        let parents = self
            .changesets()
            .get(ctx.clone(), csid)
            .await?
            .ok_or(ErrorKind::BonsaiNotFound(csid))?
            .parents
            .into_iter()
            .map(|parent| self.derive_hg_changeset(&ctx, parent));

        Ok(future::try_join_all(parents).await?)
    }

    async fn get_bookmark(
        &self,
        ctx: CoreContext,
        name: &BookmarkName,
    ) -> Result<Option<HgChangesetId>, Error>
    where
        Self: BookmarksRef + RepoDerivedDataRef,
    {
        STATS::get_bookmark.add_value(1);
        let cs_opt = self.bookmarks().get(ctx.clone(), name).await?;
        match cs_opt {
            None => Ok(None),
            Some(cs) => {
                let hg_csid = self.derive_hg_changeset(&ctx, cs).await?;
                Ok(Some(hg_csid))
            }
        }
    }

    /// Get Pull-Default (Pull-Default is a Mercurial concept) bookmarks by prefix, they will be
    /// read from cache or a replica, so they might be stale.
    fn get_pull_default_bookmarks_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> BoxStream<'_, Result<(Bookmark, HgChangesetId), Error>>
    where
        Self: ChangesetsRef
            + BonsaiHgMappingRef
            + RepoDerivedDataRef
            + BlobRepoHg
            + BookmarksRef
            + Clone
            + Send
            + Sync,
    {
        STATS::get_pull_default_bookmarks_maybe_stale.add_value(1);
        let stream = self.bookmarks().list(
            ctx.clone(),
            Freshness::MaybeStale,
            &BookmarkPrefix::empty(),
            &[BookmarkKind::PullDefaultPublishing][..],
            &BookmarkPagination::FromStart,
            std::u64::MAX,
        );
        to_hg_bookmark_stream(self, &ctx, stream)
    }

    /// Get Publishing (Publishing is a Mercurial concept) bookmarks by prefix, they will be read
    /// from cache or a replica, so they might be stale.
    fn get_publishing_bookmarks_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> BoxStream<'_, Result<(Bookmark, HgChangesetId), Error>>
    where
        Self: ChangesetsRef
            + BonsaiHgMappingRef
            + RepoDerivedDataRef
            + BlobRepoHg
            + BookmarksRef
            + Clone
            + Send
            + Sync,
    {
        STATS::get_publishing_bookmarks_maybe_stale.add_value(1);
        let stream = self.bookmarks().list(
            ctx.clone(),
            Freshness::MaybeStale,
            &BookmarkPrefix::empty(),
            BookmarkKind::ALL_PUBLISHING,
            &BookmarkPagination::FromStart,
            std::u64::MAX,
        );
        to_hg_bookmark_stream(self, &ctx, stream)
    }

    /// Get bookmarks by prefix, they will be read from replica, so they might be stale.
    fn get_bookmarks_by_prefix_maybe_stale(
        &self,
        ctx: CoreContext,
        prefix: &BookmarkPrefix,
        max: u64,
    ) -> BoxStream<'_, Result<(Bookmark, HgChangesetId), Error>>
    where
        Self: ChangesetsRef
            + BonsaiHgMappingRef
            + RepoDerivedDataRef
            + BlobRepoHg
            + BookmarksRef
            + Clone
            + Send
            + Sync,
    {
        STATS::get_bookmarks_by_prefix_maybe_stale.add_value(1);
        let stream = self.bookmarks().list(
            ctx.clone(),
            Freshness::MaybeStale,
            prefix,
            BookmarkKind::ALL,
            &BookmarkPagination::FromStart,
            max,
        );
        to_hg_bookmark_stream(self, &ctx, stream)
    }

    async fn get_filenode_opt(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        node: HgFileNodeId,
    ) -> Result<FilenodeResult<Option<FilenodeInfo>>, Error>
    where
        Self: FilenodesRef,
    {
        self.filenodes().get_filenode(&ctx, path, node).await
    }

    async fn get_filenode(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        node: HgFileNodeId,
    ) -> Result<FilenodeResult<FilenodeInfo>, Error>
    where
        Self: FilenodesRef,
    {
        match self.get_filenode_opt(ctx, path, node).await? {
            FilenodeResult::Present(maybe_filenode) => {
                let filenode = maybe_filenode
                    .ok_or_else(|| Error::from(ErrorKind::MissingFilenode(path.clone(), node)))?;
                Ok(FilenodeResult::Present(filenode))
            }
            FilenodeResult::Disabled => Ok(FilenodeResult::Disabled),
        }
    }

    async fn get_all_filenodes_maybe_stale(
        &self,
        ctx: CoreContext,
        path: RepoPath,
        limit: Option<u64>,
    ) -> Result<FilenodeRangeResult<Vec<FilenodeInfo>>, Error>
    where
        Self: FilenodesRef,
    {
        STATS::get_all_filenodes.add_value(1);
        self.filenodes()
            .get_all_filenodes_maybe_stale(&ctx, &path, limit)
            .await
    }
}

pub fn to_hg_bookmark_stream<'repo, BookmarkType>(
    repo: &(
         impl ChangesetsRef
         + BonsaiHgMappingRef
         + RepoDerivedDataRef
         + BlobRepoHg
         + Clone
         + Send
         + Sync
         + 'repo
     ),
    ctx: &CoreContext,
    stream: impl Stream<Item = Result<(BookmarkType, ChangesetId), Error>> + Send + 'repo,
) -> BoxStream<'repo, Result<(BookmarkType, HgChangesetId), Error>>
where
    BookmarkType: Send,
{
    stream
        .chunks(100)
        .map({
            cloned!(repo, ctx);
            move |chunk| {
                cloned!(repo, ctx);
                async move {
                    let chunk: Vec<_> = chunk.into_iter().collect::<Result<_, _>>()?;
                    let cs_ids = chunk.iter().map(|(_, cs_id)| *cs_id).collect::<Vec<_>>();
                    let mapping: HashMap<_, _> = repo
                        .get_hg_bonsai_mapping(ctx.clone(), cs_ids)
                        .await?
                        .into_iter()
                        .map(|(hg_cs_id, cs_id)| (cs_id, hg_cs_id))
                        .collect();
                    let res = chunk.into_iter().map(move |(bookmark, cs_id)| {
                        let hg_cs_id = mapping.get(&cs_id).ok_or_else(|| {
                            anyhow::format_err!("cs_id was missing from mapping: {:?}", cs_id)
                        })?;
                        Ok((bookmark, *hg_cs_id))
                    });
                    Ok(stream::iter(res))
                }
                .try_flatten_stream()
            }
        })
        .flatten()
        .boxed()
}
