/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use cloned::cloned;
use context::CoreContext;
use futures::{future, Future, IntoFuture, Stream};
use futures_ext::{spawn_future, FutureExt};
use futures_stats::Timed;
use manifest::{Entry, ManifestOps};
use mercurial_types::{HgChangesetId, HgFileNodeId, RepoPath};
use metaconfig_types::CacheWarmupParams;
use revset::AncestorsNodeStream;
use scuba_ext::ScubaSampleBuilderExt;
use slog::{debug, info};
use tracing::{trace_args, Traced};

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
    revision: HgChangesetId,
) -> impl Future<Item = (), Error = Error> {
    // TODO(stash): Arbitrary number. Tweak somehow?
    let buffer_size = 100;
    repo.get_changeset_by_changesetid(ctx.clone(), revision)
        .map({
            cloned!(ctx, repo);
            move |cs| {
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

                    let fut = repo.get_linknode(ctx.clone(), &path, hash);
                    Some(fut)
                }
            }
        })
        .buffered(buffer_size)
        .for_each({
            cloned!(ctx);
            let mut i = 0;
            move |_| {
                i += 1;
                if i % 10000 == 0 {
                    debug!(ctx.logger(), "manifests warmup: fetched {}th entry", i);
                }
                Ok(())
            }
        })
        .map({
            cloned!(ctx);
            move |()| {
                debug!(ctx.logger(), "finished manifests warmup");
            }
        })
        .traced(
            &ctx.trace(),
            "cache_warmup::blobstore_and_filenodes",
            trace_args! {},
        )
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
        .traced(&ctx.trace(), "cache_warmup::changesets", trace_args! {})
}

fn do_cache_warmup(
    ctx: CoreContext,
    repo: BlobRepo,
    bookmark: BookmarkName,
    commit_limit: usize,
) -> impl Future<Item = (), Error = Error> {
    let ctx = ctx.clone_and_reset();

    repo.get_bookmark(ctx.clone(), &bookmark)
        .and_then({
            cloned!(repo, ctx);
            move |bookmark_rev| match bookmark_rev {
                Some(bookmark_rev) => {
                    let blobstore_warmup = spawn_future(blobstore_and_filenodes_warmup(
                        ctx.clone(),
                        repo.clone(),
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
        .traced(&ctx.trace(), "cache_warmup", trace_args! {})
        .map({
            let ctx = ctx.clone();
            move |_| {
                tokio::spawn(future::lazy(move || ctx.trace_upload().then(|_| Ok(()))));
            }
        })
}

/// Fetch all manifest entries for a bookmark, and fetches up to `commit_warmup_limit`
/// ancestors of the bookmark.
pub fn cache_warmup(
    ctx: CoreContext,
    repo: BlobRepo,
    cache_warmup: Option<CacheWarmupParams>,
) -> impl Future<Item = (), Error = Error> {
    match cache_warmup {
        Some(cache_warmup) => {
            do_cache_warmup(ctx, repo, cache_warmup.bookmark, cache_warmup.commit_limit)
                .left_future()
        }
        None => Ok(()).into_future().right_future(),
    }
}
