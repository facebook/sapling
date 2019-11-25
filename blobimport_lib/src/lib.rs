/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

mod bookmark;
mod changeset;
mod concurrency;

use std::cmp;
use std::collections::HashMap;
use std::error::Error as StdError;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;

use ascii::AsciiString;
use cloned::cloned;
use failure_ext::{err_msg, Error};
use futures::{future, Future, Stream};
use futures_ext::{BoxFuture, FutureExt, StreamExt};
use slog::{debug, error, info, Logger};

use blobrepo::BlobRepo;
use bonsai_globalrev_mapping::{upload_globalrevs, BonsaiGlobalrevMapping};
use context::CoreContext;
use mercurial_revlog::RevlogRepo;
use mercurial_types::{HgChangesetId, HgNodeHash};
use mononoke_types::RepositoryId;
use phases::Phases;
use synced_commit_mapping::{SyncedCommitMapping, SyncedCommitMappingEntry};

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
    pub globalrevs_store: Arc<dyn BonsaiGlobalrevMapping>,
    pub synced_commit_mapping: Arc<dyn SyncedCommitMapping>,
    pub lfs_helper: Option<String>,
    pub concurrent_changesets: usize,
    pub concurrent_blobs: usize,
    pub concurrent_lfs_imports: usize,
    pub fixed_parent_order: HashMap<HgChangesetId, Vec<HgChangesetId>>,
    pub has_globalrev: bool,
    pub small_repo_id: Option<RepositoryId>,
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
            globalrevs_store,
            synced_commit_mapping,
            lfs_helper,
            concurrent_changesets,
            concurrent_blobs,
            concurrent_lfs_imports,
            fixed_parent_order,
            has_globalrev,
            small_repo_id,
        } = self;

        let repo_id = blobrepo.get_repoid();

        let stale_bookmarks = {
            let revlogrepo = RevlogRepo::open(&revlogrepo_path).expect("cannot open revlogrepo");
            bookmark::read_bookmarks(revlogrepo)
        };

        let revlogrepo = RevlogRepo::open(revlogrepo_path).expect("cannot open revlogrepo");

        let log_step = match commits_limit {
            Some(commits_limit) => cmp::max(1, commits_limit / 10),
            None => 5000,
        };

        let chunk_size = 100;

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
            fixed_parent_order,
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
                cs.0.clone()
            }
        })
        .map_err({
            let logger = logger.clone();
            move |err| {
                let msg = format!("failed to blobimport: {}", err);
                error!(logger, "{}", msg);

                let mut err = err.deref() as &dyn StdError;
                while let Some(cause) = failure_ext::cause(err) {
                    info!(logger, "cause: {}", cause);
                    err = cause;
                }
                info!(logger, "root cause: {:?}", err);

                err_msg(msg)
            }
        });

        // Blobimport does not see scratch bookmarks in Mercurial, so we use
        // PublishingOrPullDefault here, which is the non-scratch set in Mononoke.
        let mononoke_bookmarks = blobrepo
            .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
            .map(|(bookmark, changeset_id)| (bookmark.into_name(), changeset_id));

        stale_bookmarks
            .join(mononoke_bookmarks.collect())
            .and_then({
                cloned!(ctx);
                move |(stale_bookmarks, mononoke_bookmarks)| {
                    upload_changesets
                        .chunks(chunk_size)
                        .and_then({
                            cloned!(ctx, globalrevs_store, synced_commit_mapping);
                            move |chunk| {
                                let synced_commit_mapping_work =
                                    if let Some(small_repo_id) = small_repo_id {
                                        let entries = chunk
                                            .iter()
                                            .map(|cs| SyncedCommitMappingEntry {
                                                large_repo_id: repo_id,
                                                large_bcs_id: cs.get_changeset_id(),
                                                small_repo_id,
                                                small_bcs_id: cs.get_changeset_id(),
                                            })
                                            .collect();
                                        synced_commit_mapping
                                            .add_bulk(ctx.clone(), entries)
                                            .map(|_| ())
                                            .left_future()
                                    } else {
                                        future::ok(()).right_future()
                                    };

                                let globalrevs_work = if has_globalrev {
                                    upload_globalrevs(
                                        ctx.clone(),
                                        repo_id,
                                        globalrevs_store.clone(),
                                        chunk,
                                    )
                                    .left_future()
                                } else {
                                    future::ok(()).right_future()
                                };

                                globalrevs_work.join(synced_commit_mapping_work)
                            }
                        })
                        .for_each(|_| Ok(()))
                        .map(move |()| (stale_bookmarks, mononoke_bookmarks))
                }
            })
            .and_then(move |(stale_bookmarks, mononoke_bookmarks)| {
                info!(logger, "finished uploading changesets and globalrevs");
                match bookmark_import_policy {
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
                }
            })
            .boxify()
    }
}
