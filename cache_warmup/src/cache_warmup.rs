/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use cloned::cloned;
use context::CoreContext;
use failure::Error;
use futures::{future, Future, IntoFuture, Stream};
use futures_ext::{spawn_future, FutureExt};
use manifest::{Entry, ManifestOps};
use mercurial_types::{Changeset, HgChangesetId, HgFileNodeId, RepoPath};
use metaconfig_types::CacheWarmupParams;
use revset::AncestorsNodeStream;
use slog::{debug, info, Logger};
use tracing::{trace_args, Traced};
use upload_trace::{manifold_thrift::thrift::RequestContext, UploadTrace};

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
    logger: Logger,
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
            cloned!(logger);
            let mut i = 0;
            move |_| {
                i += 1;
                if i % 10000 == 0 {
                    debug!(logger, "manifests warmup: fetched {}th entry", i);
                }
                Ok(())
            }
        })
        .map(move |()| {
            debug!(logger, "finished manifests warmup");
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
    logger: Logger,
) -> impl Future<Item = (), Error = Error> {
    info!(logger, "about to start warming up changesets cache");

    repo.get_bonsai_from_hg(ctx.clone(), start_rev)
        .and_then({
            let start_rev = start_rev.clone();
            move |maybe_node| {
                maybe_node.ok_or(errors::ErrorKind::BookmarkValueNotFound(start_rev).into())
            }
        })
        .and_then({
            let ctx = ctx.clone();
            move |start_rev| {
                AncestorsNodeStream::new(ctx, &repo.get_changeset_fetcher(), start_rev)
                    .take(cs_limit as u64)
                    .collect()
                    .map(move |_| {
                        debug!(logger, "finished changesets warmup");
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
    logger: Logger,
) -> impl Future<Item = (), Error = Error> {
    repo.get_bookmark(ctx.clone(), &bookmark)
        .and_then({
            let logger = logger.clone();
            let repo = repo.clone();
            let ctx = ctx.clone();
            move |bookmark_rev| match bookmark_rev {
                Some(bookmark_rev) => {
                    let blobstore_warmup = spawn_future(blobstore_and_filenodes_warmup(
                        ctx.clone(),
                        repo.clone(),
                        bookmark_rev,
                        logger.clone(),
                    ));
                    let cs_warmup = spawn_future(changesets_warmup(
                        ctx,
                        bookmark_rev,
                        repo,
                        commit_limit,
                        logger,
                    ));
                    blobstore_warmup.join(cs_warmup).map(|_| ()).left_future()
                }
                None => {
                    info!(logger, "{} bookmark not found!", bookmark);
                    Err(errors::ErrorKind::BookmarkNotFound(bookmark).into())
                        .into_future()
                        .right_future()
                }
            }
        })
        .map(move |()| {
            info!(logger, "finished initial warmup");
            ()
        })
        .traced(&ctx.trace(), "cache_warmup", trace_args! {})
        .map({
            let ctx = ctx.clone();
            move |_| {
                tokio::spawn(future::lazy(move || {
                    ctx.trace()
                        .upload_to_manifold(RequestContext {
                            bucketName: "mononoke_prod".into(),
                            apiKey: "".into(),
                            ..Default::default()
                        })
                        .then(move |upload_res| {
                            match upload_res {
                                Err(err) => {
                                    info!(ctx.logger(), "warmup trace failed to upload: {:#?}", err)
                                }
                                Ok(()) => info!(
                                    ctx.logger(),
                                    "warmup trace uploaded: {}",
                                    ctx.trace().id()
                                ),
                            }
                            Ok(())
                        })
                }));
            }
        })
}

/// Fetch all manifest entries for a bookmark, and fetches up to `commit_warmup_limit`
/// ancestors of the bookmark.
pub fn cache_warmup(
    ctx: CoreContext,
    repo: BlobRepo,
    cache_warmup: Option<CacheWarmupParams>,
    logger: Logger,
) -> impl Future<Item = (), Error = Error> {
    match cache_warmup {
        Some(cache_warmup) => do_cache_warmup(
            ctx,
            repo,
            cache_warmup.bookmark,
            cache_warmup.commit_limit,
            logger.clone(),
        )
        .left_future(),
        None => Ok(()).into_future().right_future(),
    }
}
