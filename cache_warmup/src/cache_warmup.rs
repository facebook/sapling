// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate bookmarks;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
#[macro_use]
extern crate slog;

extern crate blobrepo;
extern crate context;
extern crate mercurial_types;
extern crate metaconfig_types;
extern crate revset;

use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use context::CoreContext;
use futures::{Future, IntoFuture, Stream};
use futures_ext::{spawn_future, BoxFuture, FutureExt};
use mercurial_types::manifest::{Entry, Type};
use mercurial_types::manifest_utils::recursive_entry_stream;
use mercurial_types::{Changeset, HgChangesetId, HgFileNodeId, MPath, RepoPath};
use metaconfig_types::CacheWarmupParams;
use revset::AncestorsNodeStream;
use slog::Logger;

mod errors {
    use bookmarks::BookmarkName;
    use mercurial_types::HgChangesetId;

    #[derive(Debug, Fail)]
    pub enum ErrorKind {
        #[fail(display = "Bookmark {} does not exist", _0)]
        BookmarkNotFound(BookmarkName),
        #[fail(display = "Bookmark value {} not found", _0)]
        BookmarkValueNotFound(HgChangesetId),
    }
}

use failure::Error;

// Fetches all the manifest entries and their linknodes. Do not fetching files because
// there can be too many of them.
fn blobstore_and_filenodes_warmup(
    ctx: CoreContext,
    repo: BlobRepo,
    revision: HgChangesetId,
    logger: Logger,
) -> BoxFuture<(), Error> {
    // TODO(stash): Arbitrary number. Tweak somehow?
    let buffer_size = 100;
    repo.get_changeset_by_changesetid(ctx.clone(), revision)
        .map({
            let repo = repo.clone();
            move |cs| repo.get_root_entry(cs.manifestid())
        })
        .and_then({
            move |root_entry| {
                info!(logger, "starting precaching");
                let rootpath = None;
                let mut i = 0;
                recursive_entry_stream(ctx.clone(), rootpath, Box::new(root_entry))
                    .filter(|&(ref _path, ref entry)| entry.get_type() == Type::Tree)
                    .map(move |(path, entry)| {
                        let hash = HgFileNodeId::new(entry.get_hash().into_nodehash());
                        let path = MPath::join_element_opt(path.as_ref(), entry.get_name());
                        let path = match path {
                            Some(path) => RepoPath::DirectoryPath(path),
                            None => RepoPath::RootPath,
                        };
                        repo.get_linknode(ctx.clone(), &path, hash)
                    })
                    .buffered(buffer_size)
                    .for_each({
                        let logger = logger.clone();
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
            }
        })
        .boxify()
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
        .and_then(move |start_rev| {
            AncestorsNodeStream::new(ctx, &repo.get_changeset_fetcher(), start_rev)
                .take(cs_limit as u64)
                .collect()
                .map(move |_| {
                    debug!(logger, "finished changesets warmup");
                })
        })
}

fn do_cache_warmup(
    ctx: CoreContext,
    repo: BlobRepo,
    bookmark: BookmarkName,
    commit_limit: usize,
    logger: Logger,
) -> BoxFuture<(), Error> {
    repo.get_bookmark(ctx.clone(), &bookmark)
        .and_then({
            let logger = logger.clone();
            let repo = repo.clone();
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
                    blobstore_warmup.join(cs_warmup).map(|_| ()).boxify()
                }
                None => {
                    info!(logger, "{} bookmark not found!", bookmark);
                    Err(errors::ErrorKind::BookmarkNotFound(bookmark).into())
                        .into_future()
                        .boxify()
                }
            }
        })
        .map(move |()| {
            info!(logger, "finished initial warmup");
            ()
        })
        .boxify()
}

/// Fetch all manifest entries for a bookmark, and fetches up to `commit_warmup_limit`
/// ancestors of the bookmark.
pub fn cache_warmup(
    ctx: CoreContext,
    repo: BlobRepo,
    cache_warmup: Option<CacheWarmupParams>,
    logger: Logger,
) -> BoxFuture<(), Error> {
    match cache_warmup {
        Some(cache_warmup) => do_cache_warmup(
            ctx,
            repo,
            cache_warmup.bookmark,
            cache_warmup.commit_limit,
            logger.clone(),
        ),
        None => Ok(()).into_future().boxify(),
    }
}
