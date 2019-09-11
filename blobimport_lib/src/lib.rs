// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

mod bookmark;
mod changeset;
mod concurrency;

use std::cmp;
use std::path::PathBuf;
use std::sync::Arc;

use ascii::AsciiString;
use failure_ext::{err_msg, Error};
use futures::{future, Future, Stream};
use futures_ext::{BoxFuture, FutureExt, StreamExt};
use slog::{debug, error, info, Logger};

use blobrepo::BlobRepo;
use context::CoreContext;
use mercurial_revlog::RevlogRepo;
use mercurial_types::HgNodeHash;
use phases::Phases;

use crate::changeset::UploadChangesets;

// What to do with bookmarks when blobimporting a repo
pub enum BookmarkImportPolicy {
    // Do not import bookmarks
    Ignore,
    // Prefix bookmark names when importing
    Prefix(AsciiString),
}

pub struct Blobimport {
    pub ctx: CoreContext,
    pub logger: Logger,
    pub blobrepo: BlobRepo,
    pub revlogrepo_path: PathBuf,
    pub changeset: Option<HgNodeHash>,
    pub skip: Option<usize>,
    pub commits_limit: Option<usize>,
    pub bookmark_import_policy: BookmarkImportPolicy,
    pub phases_store: Arc<dyn Phases>,
    pub lfs_helper: Option<String>,
    pub concurrent_changesets: usize,
    pub concurrent_blobs: usize,
    pub concurrent_lfs_imports: usize,
}

impl Blobimport {
    pub fn import(self) -> BoxFuture<(), Error> {
        let Self {
            ctx,
            logger,
            blobrepo,
            revlogrepo_path,
            changeset,
            skip,
            commits_limit,
            bookmark_import_policy,
            phases_store,
            lfs_helper,
            concurrent_changesets,
            concurrent_blobs,
            concurrent_lfs_imports,
        } = self;

        let stale_bookmarks = {
            let revlogrepo = RevlogRepo::open(&revlogrepo_path).expect("cannot open revlogrepo");
            bookmark::read_bookmarks(revlogrepo)
        };

        let revlogrepo = RevlogRepo::open(revlogrepo_path).expect("cannot open revlogrepo");

        let log_step = match commits_limit {
            Some(commits_limit) => cmp::max(1, commits_limit / 10),
            None => 5000,
        };

        let upload_changesets = UploadChangesets {
            ctx: ctx.clone(),
            blobrepo: blobrepo.clone(),
            revlogrepo: revlogrepo.clone(),
            changeset,
            skip,
            commits_limit,
            phases_store,
            lfs_helper,
            concurrent_changesets,
            concurrent_blobs,
            concurrent_lfs_imports,
        }
        .upload()
        .enumerate()
        .map({
            let logger = logger.clone();
            move |(cs_count, cs)| {
                debug!(logger, "{} inserted: {}", cs_count, cs.1.get_changeset_id());
                if cs_count % log_step == 0 {
                    info!(logger, "inserted commits # {}", cs_count);
                }
                ()
            }
        })
        .map_err({
            let logger = logger.clone();
            move |err| {
                error!(logger, "failed to blobimport: {}", err);

                for cause in err.iter_chain() {
                    info!(logger, "cause: {}", cause);
                }
                info!(logger, "root cause: {:?}", err.find_root_cause());

                let msg = format!("failed to blobimport: {}", err);
                err_msg(msg)
            }
        })
        .for_each(|()| Ok(()))
        .inspect({
            let logger = logger.clone();
            move |()| {
                info!(logger, "finished uploading changesets");
            }
        });

        // Blobimport does not see scratch bookmarks in Mercurial, so we use
        // PublishingOrPullDefault here, which is the non-scratch set in Mononoke.
        let mononoke_bookmarks = blobrepo
            .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
            .map(|(bookmark, changeset_id)| (bookmark.into_name(), changeset_id));

        stale_bookmarks
            .join(mononoke_bookmarks.collect())
            .and_then(move |(stale_bookmarks, mononoke_bookmarks)| {
                upload_changesets.map(move |()| (stale_bookmarks, mononoke_bookmarks))
            })
            .and_then(
                move |(stale_bookmarks, mononoke_bookmarks)| match bookmark_import_policy {
                    BookmarkImportPolicy::Ignore => {
                        info!(
                            logger,
                            "since --no-bookmark was provided, bookmarks won't be imported"
                        );
                        future::ok(()).boxify()
                    }
                    BookmarkImportPolicy::Prefix(prefix) => bookmark::upload_bookmarks(
                        ctx,
                        &logger,
                        revlogrepo,
                        blobrepo,
                        stale_bookmarks,
                        mononoke_bookmarks,
                        bookmark::get_bookmark_prefixer(prefix),
                    ),
                },
            )
            .boxify()
    }
}
