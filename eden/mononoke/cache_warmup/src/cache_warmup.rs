/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bookmarks::BookmarkName;
use cloned::cloned;
use context::{CoreContext, PerfCounterType};
use derived_data::BonsaiDerived;
use derived_data_filenodes::FilenodesOnlyPublic;
use futures::future::{FutureExt as _, TryFutureExt};
use futures_ext::{spawn_future, FutureExt};
use futures_old::{future, Future, IntoFuture, Stream};
use futures_stats::Timed;
use manifest::{Entry, ManifestOps};
use mercurial_types::{HgChangesetId, HgFileNodeId, RepoPath};
use metaconfig_types::CacheWarmupParams;
use microwave::{self, SnapshotLocation};
use mononoke_types::ChangesetId;
use revset::AncestorsNodeStream;
use scuba_ext::ScubaSampleBuilderExt;
use slog::{debug, info, warn};

mod errors {
    use bookmarks::BookmarkName;
    use mercurial_types::HgChangesetId;
    use thiserror::Error;

    #[derive(Debug, Error)]
    pub enum ErrorKind {
        #[error("Bookmark {0} does not exist")]
        BookmarkNotFound(BookmarkName),
        #[error("Bookmark value {0} not found")]
        BookmarkValueNotFound(HgChangesetId),
    }
}

// Fetches all the manifest entries and their linknodes. Do not fetching files because
// there can be too many of them.
fn blobstore_and_filenodes_warmup(
    ctx: CoreContext,
    repo: BlobRepo,
    bcs_id: ChangesetId,
    revision: HgChangesetId,
) -> impl Future<Item = (), Error = Error> {
    // TODO(stash): Arbitrary number. Tweak somehow?
    let buffer_size = 100;

    let derive_filenodes =
        FilenodesOnlyPublic::derive(ctx.clone(), repo.clone(), bcs_id).from_err();

    revision
        .load(ctx.clone(), repo.blobstore())
        .from_err()
        .join(derive_filenodes)
        .map({
            cloned!(ctx, repo);
            move |(cs, _)| {
                cs.manifestid()
                    .list_all_entries(ctx.clone(), repo.get_blobstore())
            }
        })
        .flatten_stream()
        .filter_map({
            cloned!(ctx, repo);
            move |(path, entry)| match entry {
                Entry::Leaf(_) => None,
                Entry::Tree(hash) => {
                    let hash = HgFileNodeId::new(hash.into_nodehash());

                    let path = match path {
                        Some(path) => RepoPath::DirectoryPath(path),
                        None => RepoPath::RootPath,
                    };

                    let fut = repo.get_linknode_opt(ctx.clone(), &path, hash);
                    Some(fut)
                }
            }
        })
        .buffered(buffer_size)
        .fold(0, {
            cloned!(ctx);
            let mut i = 0;
            move |mut null_linknodes, linknode| {
                if linknode.is_none() {
                    ctx.perf_counters()
                        .increment_counter(PerfCounterType::NullLinknode);
                    null_linknodes += 1;
                }
                i += 1;
                if i % 10000 == 0 {
                    debug!(ctx.logger(), "manifests warmup: fetched {}th entry", i);
                }
                let res: Result<_, Error> = Ok(null_linknodes);
                res
            }
        })
        .map({
            cloned!(ctx);
            move |null_linknodes| {
                if null_linknodes > 0 {
                    warn!(ctx.logger(), "{} linknodes are missing!", null_linknodes);
                }
                debug!(ctx.logger(), "finished manifests warmup");
            }
        })
}

// Iterate over first parents, and fetch them
fn changesets_warmup(
    ctx: CoreContext,
    start_rev: HgChangesetId,
    repo: BlobRepo,
    cs_limit: usize,
) -> impl Future<Item = (), Error = Error> {
    info!(ctx.logger(), "about to start warming up changesets cache");

    repo.get_bonsai_from_hg(ctx.clone(), start_rev)
        .and_then({
            let start_rev = start_rev.clone();
            move |maybe_node| {
                maybe_node.ok_or(errors::ErrorKind::BookmarkValueNotFound(start_rev).into())
            }
        })
        .and_then({
            cloned!(ctx);
            move |start_rev| {
                AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), start_rev)
                    .take(cs_limit as u64)
                    .collect()
                    .map(move |_| {
                        debug!(ctx.logger(), "finished changesets warmup");
                    })
            }
        })
}

fn do_cache_warmup(
    ctx: CoreContext,
    repo: BlobRepo,
    bookmark: BookmarkName,
    commit_limit: usize,
) -> impl Future<Item = (), Error = Error> {
    let ctx = ctx.clone_and_reset();

    repo.get_bonsai_bookmark(ctx.clone(), &bookmark)
        .and_then({
            cloned!(repo, ctx);
            move |maybe_bcs_id| match maybe_bcs_id {
                Some(bcs_id) => repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id)
                    .map(move |hg_cs_id| Some((bcs_id, hg_cs_id)))
                    .left_future(),
                None => future::ok(None).right_future(),
            }
        })
        .and_then({
            cloned!(repo, ctx);
            move |book_bcs_hg_cs_id| match book_bcs_hg_cs_id {
                Some((bcs_id, bookmark_rev)) => {
                    let blobstore_warmup = spawn_future(blobstore_and_filenodes_warmup(
                        ctx.clone(),
                        repo.clone(),
                        bcs_id,
                        bookmark_rev,
                    ));
                    let cs_warmup =
                        spawn_future(changesets_warmup(ctx, bookmark_rev, repo, commit_limit));
                    blobstore_warmup.join(cs_warmup).map(|_| ()).left_future()
                }
                None => {
                    info!(ctx.logger(), "{} bookmark not found!", bookmark);
                    Err(errors::ErrorKind::BookmarkNotFound(bookmark).into())
                        .into_future()
                        .right_future()
                }
            }
        })
        .map({
            cloned!(ctx);
            move |()| {
                info!(ctx.logger(), "finished initial warmup");
                ()
            }
        })
        .timed({
            cloned!(ctx);
            move |stats, _| {
                let mut scuba = ctx.scuba().clone();
                scuba.add_future_stats(&stats);
                ctx.perf_counters().insert_perf_counters(&mut scuba);
                scuba.log_with_msg("Cache warmup complete", None);
                Ok(())
            }
        })
}

fn microwave_preload(
    ctx: CoreContext,
    repo: BlobRepo,
    params: CacheWarmupParams,
) -> impl Future<Item = (CoreContext, BlobRepo, CacheWarmupParams), Error = Error> {
    async move {
        if params.microwave_preload {
            match microwave::prime_cache(&ctx, &repo, SnapshotLocation::Blobstore).await {
                Ok(_) => {
                    warn!(ctx.logger(), "microwave: successfully primed cached");
                }
                Err(e) => {
                    warn!(ctx.logger(), "microwave: cache warmup failed: {:#?}", e);
                }
            }
        }

        Ok((ctx, repo, params))
    }
    .boxed()
    .compat()
}

/// Fetch all manifest entries for a bookmark, and fetches up to `commit_warmup_limit`
/// ancestors of the bookmark.
pub fn cache_warmup(
    ctx: CoreContext,
    repo: BlobRepo,
    cache_warmup: Option<CacheWarmupParams>,
) -> impl Future<Item = (), Error = Error> {
    let repoid = repo.get_repoid();
    match cache_warmup {
        Some(params) => microwave_preload(ctx, repo, params)
            .and_then(move |(ctx, repo, params)| {
                do_cache_warmup(ctx, repo, params.bookmark, params.commit_limit)
            })
            .map_err(move |err| {
                err.context(format!("while warming up repo {}", repoid))
                    .into()
            })
            .left_future(),
        None => Ok(()).into_future().right_future(),
    }
}
