/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

mod bonsai_generation;
mod create_changeset;
pub mod derive_hg_changeset;
pub mod derive_hg_manifest;
pub mod file_history;
pub mod repo_commit;

pub use crate::repo_commit::ChangesetHandle;
pub use changeset_fetcher::ChangesetFetcher;
// TODO: This is exported for testing - is this the right place for it?
pub use crate::repo_commit::{check_case_conflicts, compute_changed_files, UploadEntries};
pub mod errors {
    pub use blobrepo_errors::*;
}
pub use create_changeset::CreateChangeset;

use anyhow::{Context, Error};
use blobrepo::BlobRepo;
use blobrepo_errors::ErrorKind;
use blobstore::Loadable;
use bonsai_hg_mapping::{BonsaiHgMapping, BonsaiOrHgChangesetIds};
use bookmarks::{
    Bookmark, BookmarkKind, BookmarkName, BookmarkPagination, BookmarkPrefix, Freshness,
};
use cloned::cloned;
use context::CoreContext;
use failure_ext::FutureFailureExt;
use filenodes::{FilenodeInfo, FilenodeResult, Filenodes};
use futures::future::TryFutureExt;
use futures::stream::TryStreamExt;
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use futures_old::{
    future,
    stream::{self, futures_unordered},
    Future, IntoFuture, Stream,
};
use mercurial_mutation::HgMutationStore;
use mercurial_types::{blobs::HgBlobEnvelope, HgChangesetId, HgFileNodeId};
use mononoke_types::{ChangesetId, RepoPath};
use stats::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// `BlobRepoHg` is an extension trait for `BlobRepo` which contains
/// mercurial specific methods.
pub trait BlobRepoHg {
    fn get_bonsai_hg_mapping(&self) -> &Arc<dyn BonsaiHgMapping>;

    fn get_filenodes(&self) -> &Arc<dyn Filenodes>;

    fn hg_mutation_store(&self) -> &Arc<dyn HgMutationStore>;

    fn get_bonsai_from_hg(
        &self,
        ctx: CoreContext,
        hg_cs_id: HgChangesetId,
    ) -> BoxFuture<Option<ChangesetId>, Error>;

    fn get_hg_bonsai_mapping(
        &self,
        ctx: CoreContext,
        bonsai_or_hg_cs_ids: impl Into<BonsaiOrHgChangesetIds>,
    ) -> BoxFuture<Vec<(HgChangesetId, ChangesetId)>, Error>;

    fn get_heads_maybe_stale(&self, ctx: CoreContext) -> BoxStream<HgChangesetId, Error>;

    fn changeset_exists(
        &self,
        ctx: CoreContext,
        changesetid: HgChangesetId,
    ) -> BoxFuture<bool, Error>;

    fn get_changeset_parents(
        &self,
        ctx: CoreContext,
        changesetid: HgChangesetId,
    ) -> BoxFuture<Vec<HgChangesetId>, Error>;

    fn get_bookmark(
        &self,
        ctx: CoreContext,
        name: &BookmarkName,
    ) -> BoxFuture<Option<HgChangesetId>, Error>;

    fn get_pull_default_bookmarks_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> BoxStream<(Bookmark, HgChangesetId), Error>;

    fn get_publishing_bookmarks_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> BoxStream<(Bookmark, HgChangesetId), Error>;

    fn get_bookmarks_by_prefix_maybe_stale(
        &self,
        ctx: CoreContext,
        prefix: &BookmarkPrefix,
        max: u64,
    ) -> BoxStream<(Bookmark, HgChangesetId), Error>;

    fn get_filenode_opt(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        node: HgFileNodeId,
    ) -> BoxFuture<FilenodeResult<Option<FilenodeInfo>>, Error>;

    fn get_filenode(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        node: HgFileNodeId,
    ) -> BoxFuture<FilenodeResult<FilenodeInfo>, Error>;

    fn get_all_filenodes_maybe_stale(
        &self,
        ctx: CoreContext,
        path: RepoPath,
    ) -> BoxFuture<FilenodeResult<Vec<FilenodeInfo>>, Error>;

    fn get_hg_from_bonsai_changeset(
        &self,
        ctx: CoreContext,
        bcs_id: ChangesetId,
    ) -> BoxFuture<HgChangesetId, Error>;

    fn get_filenode_from_envelope(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        node: HgFileNodeId,
        linknode: HgChangesetId,
    ) -> BoxFuture<FilenodeInfo, Error>;
}

define_stats! {
    prefix = "mononoke.blobrepo";
    changeset_exists: timeseries(Rate, Sum),
    get_all_filenodes: timeseries(Rate, Sum),
    get_bonsai_from_hg: timeseries(Rate, Sum),
    get_bookmark: timeseries(Rate, Sum),
    get_bookmarks_by_prefix_maybe_stale: timeseries(Rate, Sum),
    get_changeset_parents: timeseries(Rate, Sum),
    get_heads_maybe_stale: timeseries(Rate, Sum),
    get_hg_bonsai_mapping: timeseries(Rate, Sum),
    get_publishing_bookmarks_maybe_stale: timeseries(Rate, Sum),
    get_pull_default_bookmarks_maybe_stale: timeseries(Rate, Sum),
}

impl BlobRepoHg for BlobRepo {
    fn get_bonsai_hg_mapping(&self) -> &Arc<dyn BonsaiHgMapping> {
        self.attribute_expected::<dyn BonsaiHgMapping>()
    }

    fn get_filenodes(&self) -> &Arc<dyn Filenodes> {
        self.attribute_expected::<dyn Filenodes>()
    }

    fn hg_mutation_store(&self) -> &Arc<dyn HgMutationStore> {
        self.attribute_expected::<dyn HgMutationStore>()
    }

    fn get_bonsai_from_hg(
        &self,
        ctx: CoreContext,
        hg_cs_id: HgChangesetId,
    ) -> BoxFuture<Option<ChangesetId>, Error> {
        STATS::get_bonsai_from_hg.add_value(1);
        self.get_bonsai_hg_mapping()
            .get_bonsai_from_hg(ctx, self.get_repoid(), hg_cs_id)
    }

    // Returns only the mapping for valid changests that are known to the server.
    // For Bonsai -> Hg conversion, missing Hg changesets will be derived (so all Bonsais will be
    // in the output).
    // For Hg -> Bonsai conversion, missing Bonsais will not be returned, since they cannot be
    // derived from Hg Changesets.
    fn get_hg_bonsai_mapping(
        &self,
        ctx: CoreContext,
        bonsai_or_hg_cs_ids: impl Into<BonsaiOrHgChangesetIds>,
    ) -> BoxFuture<Vec<(HgChangesetId, ChangesetId)>, Error> {
        STATS::get_hg_bonsai_mapping.add_value(1);
        let bonsai_or_hg_cs_ids = bonsai_or_hg_cs_ids.into();
        let fetched_from_mapping = self
            .get_bonsai_hg_mapping()
            .get(ctx.clone(), self.get_repoid(), bonsai_or_hg_cs_ids.clone())
            .map(|result| {
                result
                    .into_iter()
                    .map(|entry| (entry.hg_cs_id, entry.bcs_id))
                    .collect::<Vec<_>>()
            })
            .boxify();

        use BonsaiOrHgChangesetIds::*;
        match bonsai_or_hg_cs_ids {
            Bonsai(bonsais) => fetched_from_mapping
                .and_then({
                    let repo = self.clone();
                    move |hg_bonsai_list| {
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
                        repo.get_changesets_object()
                            .get_many(ctx.clone(), repo.get_repoid(), notfound.clone())
                            .and_then(move |existing| {
                                let existing: HashSet<_> =
                                    existing.into_iter().map(|entry| entry.cs_id).collect();

                                futures_unordered(
                                    notfound
                                        .into_iter()
                                        .filter(|cs_id| existing.contains(cs_id))
                                        .map(move |bcs_id| {
                                            repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id)
                                                .map(move |hg_cs_id| (hg_cs_id, bcs_id))
                                        }),
                                )
                                .collect()
                            })
                            .map(move |mut newmapping| {
                                newmapping.extend(hg_bonsai_list);
                                newmapping
                            })
                    }
                })
                .boxify(),
            Hg(_) => fetched_from_mapping,
        }
        // TODO(stash, luk): T37303879 also need to check that entries exist in changeset table
    }

    /// Get Mercurial heads, which we approximate as publishing Bonsai Bookmarks.
    fn get_heads_maybe_stale(&self, ctx: CoreContext) -> BoxStream<HgChangesetId, Error> {
        STATS::get_heads_maybe_stale.add_value(1);
        self.get_bonsai_heads_maybe_stale(ctx.clone())
            .and_then({
                let repo = self.clone();
                move |cs| repo.get_hg_from_bonsai_changeset(ctx.clone(), cs)
            })
            .boxify()
    }

    fn changeset_exists(
        &self,
        ctx: CoreContext,
        changesetid: HgChangesetId,
    ) -> BoxFuture<bool, Error> {
        STATS::changeset_exists.add_value(1);
        let changesetid = changesetid.clone();
        let repoid = self.get_repoid();
        let changesets = self.get_changesets_object();

        self.get_bonsai_from_hg(ctx.clone(), changesetid)
            .and_then(move |maybebonsai| match maybebonsai {
                Some(bonsai) => changesets
                    .get(ctx, repoid, bonsai)
                    .map(|res| res.is_some())
                    .left_future(),
                None => Ok(false).into_future().right_future(),
            })
            .boxify()
    }

    fn get_changeset_parents(
        &self,
        ctx: CoreContext,
        changesetid: HgChangesetId,
    ) -> BoxFuture<Vec<HgChangesetId>, Error> {
        STATS::get_changeset_parents.add_value(1);
        let repo = self.clone();
        let repoid = self.get_repoid();
        let changesets = self.get_changesets_object();

        self.get_bonsai_from_hg(ctx.clone(), changesetid)
            .and_then(move |maybebonsai| {
                maybebonsai.ok_or_else(|| ErrorKind::BonsaiMappingNotFound(changesetid).into())
            })
            .and_then({
                cloned!(ctx);
                move |bonsai| {
                    changesets
                        .get(ctx, repoid, bonsai)
                        .and_then(move |maybe_bonsai| {
                            maybe_bonsai.ok_or_else(|| ErrorKind::BonsaiNotFound(bonsai).into())
                        })
                }
            })
            .map(|bonsai| bonsai.parents)
            .and_then(move |bonsai_parents| {
                future::join_all(bonsai_parents.into_iter().map(move |bonsai_parent| {
                    repo.get_hg_from_bonsai_changeset(ctx.clone(), bonsai_parent)
                }))
            })
            .boxify()
    }

    fn get_bookmark(
        &self,
        ctx: CoreContext,
        name: &BookmarkName,
    ) -> BoxFuture<Option<HgChangesetId>, Error> {
        STATS::get_bookmark.add_value(1);
        self.bookmarks()
            .get(ctx.clone(), name)
            .compat()
            .and_then({
                let repo = self.clone();
                move |cs_opt| match cs_opt {
                    None => future::ok(None).left_future(),
                    Some(cs) => repo
                        .get_hg_from_bonsai_changeset(ctx, cs)
                        .map(Some)
                        .right_future(),
                }
            })
            .boxify()
    }

    /// Get Pull-Default (Pull-Default is a Mercurial concept) bookmarks by prefix, they will be
    /// read from cache or a replica, so they might be stale.
    fn get_pull_default_bookmarks_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> BoxStream<(Bookmark, HgChangesetId), Error> {
        STATS::get_pull_default_bookmarks_maybe_stale.add_value(1);
        let stream = self
            .bookmarks()
            .list(
                ctx.clone(),
                Freshness::MaybeStale,
                &BookmarkPrefix::empty(),
                &[BookmarkKind::PullDefaultPublishing][..],
                &BookmarkPagination::FromStart,
                std::u64::MAX,
            )
            .compat()
            .boxify();
        to_hg_bookmark_stream(&self, &ctx, stream)
    }

    /// Get Publishing (Publishing is a Mercurial concept) bookmarks by prefix, they will be read
    /// from cache or a replica, so they might be stale.
    fn get_publishing_bookmarks_maybe_stale(
        &self,
        ctx: CoreContext,
    ) -> BoxStream<(Bookmark, HgChangesetId), Error> {
        STATS::get_publishing_bookmarks_maybe_stale.add_value(1);
        let stream = self
            .bookmarks()
            .list(
                ctx.clone(),
                Freshness::MaybeStale,
                &BookmarkPrefix::empty(),
                BookmarkKind::ALL_PUBLISHING,
                &BookmarkPagination::FromStart,
                std::u64::MAX,
            )
            .compat()
            .boxify();
        to_hg_bookmark_stream(&self, &ctx, stream)
    }

    /// Get bookmarks by prefix, they will be read from replica, so they might be stale.
    fn get_bookmarks_by_prefix_maybe_stale(
        &self,
        ctx: CoreContext,
        prefix: &BookmarkPrefix,
        max: u64,
    ) -> BoxStream<(Bookmark, HgChangesetId), Error> {
        STATS::get_bookmarks_by_prefix_maybe_stale.add_value(1);
        let stream = self
            .bookmarks()
            .list(
                ctx.clone(),
                Freshness::MaybeStale,
                prefix,
                BookmarkKind::ALL,
                &BookmarkPagination::FromStart,
                max,
            )
            .compat()
            .boxify();
        to_hg_bookmark_stream(&self, &ctx, stream)
    }

    fn get_filenode_opt(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        node: HgFileNodeId,
    ) -> BoxFuture<FilenodeResult<Option<FilenodeInfo>>, Error> {
        let path = path.clone();
        self.get_filenodes()
            .get_filenode(ctx, &path, node, self.get_repoid())
            .boxify()
    }

    fn get_filenode(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        node: HgFileNodeId,
    ) -> BoxFuture<FilenodeResult<FilenodeInfo>, Error> {
        self.get_filenode_opt(ctx, path, node)
            .and_then({
                cloned!(path);
                move |filenode_res| match filenode_res {
                    FilenodeResult::Present(maybe_filenode) => {
                        let filenode = maybe_filenode
                            .ok_or_else(|| Error::from(ErrorKind::MissingFilenode(path, node)))?;
                        Ok(FilenodeResult::Present(filenode))
                    }
                    FilenodeResult::Disabled => Ok(FilenodeResult::Disabled),
                }
            })
            .boxify()
    }

    fn get_all_filenodes_maybe_stale(
        &self,
        ctx: CoreContext,
        path: RepoPath,
    ) -> BoxFuture<FilenodeResult<Vec<FilenodeInfo>>, Error> {
        STATS::get_all_filenodes.add_value(1);
        self.get_filenodes()
            .get_all_filenodes_maybe_stale(ctx, &path, self.get_repoid())
    }

    fn get_hg_from_bonsai_changeset(
        &self,
        ctx: CoreContext,
        bcs_id: ChangesetId,
    ) -> BoxFuture<HgChangesetId, Error> {
        derive_hg_changeset::get_hg_from_bonsai_changeset(self, ctx, bcs_id).boxify()
    }

    fn get_filenode_from_envelope(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        node: HgFileNodeId,
        linknode: HgChangesetId,
    ) -> BoxFuture<FilenodeInfo, Error> {
        node.load(ctx, &self.get_blobstore())
            .compat()
            .with_context({
                cloned!(path);
                move || format!("While fetching filenode for {} {}", path, node)
            })
            .from_err()
            .and_then({
                cloned!(path, linknode);
                move |envelope| {
                    let (p1, p2) = envelope.parents();
                    let copyfrom = envelope
                        .get_copy_info()
                        .with_context({
                            cloned!(path);
                            move || format!("While parsing copy information for {} {}", path, node)
                        })?
                        .map(|(path, node)| (RepoPath::FilePath(path), node));
                    Ok(FilenodeInfo {
                        filenode: node,
                        p1,
                        p2,
                        copyfrom,
                        linknode,
                    })
                }
            })
            .boxify()
    }
}

fn to_hg_bookmark_stream(
    repo: &BlobRepo,
    ctx: &CoreContext,
    stream: BoxStream<(Bookmark, ChangesetId), Error>,
) -> BoxStream<(Bookmark, HgChangesetId), Error> {
    stream
        .chunks(100)
        .map({
            cloned!(repo, ctx);
            move |chunk| {
                let cs_ids = chunk.iter().map(|(_, cs_id)| *cs_id).collect::<Vec<_>>();

                repo.get_hg_bonsai_mapping(ctx.clone(), cs_ids)
                    .map(move |mapping| {
                        let mapping = mapping
                            .into_iter()
                            .map(|(hg_cs_id, cs_id)| (cs_id, hg_cs_id))
                            .collect::<HashMap<_, _>>();

                        let res = chunk
                            .into_iter()
                            .map(|(bookmark, cs_id)| {
                                let hg_cs_id = mapping.get(&cs_id).ok_or_else(|| {
                                    anyhow::format_err!(
                                        "cs_id was missing from mapping: {:?}",
                                        cs_id
                                    )
                                })?;
                                Ok((bookmark, *hg_cs_id))
                            })
                            .collect::<Vec<_>>();

                        stream::iter_result(res)
                    })
                    .flatten_stream()
            }
        })
        .flatten()
        .boxify()
}
