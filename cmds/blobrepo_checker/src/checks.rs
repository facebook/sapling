// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::BlobRepo;
use cloned::cloned;
use context::CoreContext;
use crate::errors::ErrorKind;
use failure_ext::Error;
use futures::{future, stream, Future, Sink, Stream, sync::mpsc};
use futures_ext::{spawn_future, FutureExt};
use mononoke_types::{ChangesetId, ContentId, FileChange, MPath, blob::BlobstoreValue};
use std::collections::HashSet;
use std::convert::TryFrom;
use std::fmt;
use std::sync::{Arc, Mutex};

fn check_bonsai_cs(
    cs_id: ChangesetId,
    ctx: CoreContext,
    repo: BlobRepo,
    cs_queue: mpsc::Sender<ChangesetId>,
    file_queue: mpsc::Sender<FileInformation>,
) -> impl Future<Item = (), Error = Error> {
    let changeset = repo.get_bonsai_changeset(ctx.clone(), cs_id);
    let repo_parents = repo.get_changeset_parents_by_bonsai(ctx.clone(), cs_id);

    changeset.join(repo_parents).and_then({
        move |(bcs, repo_parents)| {
            // If hash verification fails, abort early
            let hash = *bcs.clone().into_blob().id();
            if hash != cs_id {
                return future::err(ErrorKind::BadChangesetHash(cs_id, hash).into()).left_future();
            }

            // Queue checks on parents
            let parents: Vec<_> = bcs.parents().collect();
            let repo_parents_ok = if repo_parents == parents {
                future::ok(())
            } else {
                future::err(ErrorKind::DbParentsMismatch(cs_id).into())
            };
            let queue_parents = stream::iter_ok(parents.into_iter())
                .forward(cs_queue)
                .map(|_| ());

            // Queue checks on files
            let file_changes: Vec<_> = bcs.file_changes()
                .filter_map(|(mpath, opt_change)| {
                    FileInformation::maybe_from_change(cs_id, mpath, opt_change)
                })
                .collect();
            let queue_file_changes = stream::iter_ok(file_changes.into_iter())
                .forward(file_queue)
                .map(|_| ());

            // Check semantic correctness of changeset (copyinfo, files in right order)
            let bcs_verifier = future::result(
                bcs.into_mut()
                    .verify()
                    .map_err(|e| ErrorKind::InvalidChangeset(cs_id, e).into()),
            );

            queue_parents
                .join4(bcs_verifier, queue_file_changes, repo_parents_ok)
                .map(|_| ())
                .right_future()
        }
    })
}

pub fn bonsai_checker_task(
    ctx: CoreContext,
    repo: BlobRepo,
    cs_queue: mpsc::Sender<ChangesetId>,
    file_queue: mpsc::Sender<FileInformation>,
    input: mpsc::Receiver<ChangesetId>,
    error: mpsc::Sender<Error>,
) -> impl Future<Item = (), Error = ()> {
    let already_seen = Arc::new(Mutex::new(HashSet::new()));

    input
        .map({
            cloned!(already_seen, ctx, repo, cs_queue, error);
            move |cs| {
                {
                    let mut already_seen = already_seen.lock().expect("lock poisoned");
                    if already_seen.contains(&cs) {
                        return future::ok(()).left_future();
                    }

                    already_seen.insert(cs);
                }

                spawn_future(
                    check_bonsai_cs(
                        cs,
                        ctx.clone(),
                        repo.clone(),
                        cs_queue.clone(),
                        file_queue.clone(),
                    ).or_else({
                        cloned!(error);
                        move |err| error.send(err).map(|_| ()).map_err(|e| e.into_inner())
                    }),
                ).map_err(|e| panic!("Could not queue error: {:#?}", e))
                    .right_future()
            }
        })
        .buffer_unordered(1000)
        .for_each(|id| future::ok(id))
}

#[derive(Clone, Debug)]
pub struct FileInformation {
    cs_id: ChangesetId,
    mpath: MPath,
    id: ContentId,
    size: u64,
}

impl FileInformation {
    pub fn maybe_from_change(
        cs_id: ChangesetId,
        mpath: &MPath,
        change: Option<&FileChange>,
    ) -> Option<FileInformation> {
        change.map(|change| Self {
            cs_id,
            mpath: mpath.clone(),
            id: change.content_id(),
            size: change.size(),
        })
    }
}

impl fmt::Display for FileInformation {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "path {} from changeset {}, ContentId {}, size {}",
            self.mpath, self.cs_id, self.id, self.size
        )
    }
}

fn check_one_file(
    file_info: FileInformation,
    ctx: CoreContext,
    repo: BlobRepo,
) -> impl Future<Item = (), Error = Error> {
    // Fetch file.
    let file = repo.get_file_content_by_content_id(ctx.clone(), file_info.id);

    let file_checks = file.and_then({
        cloned!(file_info);
        move |file| {
            let size = u64::try_from(file.size());
            if Ok(file_info.size) != size {
                return Err(ErrorKind::BadContentSize(file_info, file.size()).into());
            }

            let id = *file.into_blob().id();
            if id != file_info.id {
                return Err(ErrorKind::BadContentId(file_info, id).into());
            }

            Ok(())
        }
    });

    let sha256_check = repo.get_file_sha256(ctx.clone(), file_info.id)
        .and_then(move |sha256| {
            repo.get_file_content_id_by_alias(ctx, sha256)
                .map(move |id| (sha256, id))
        })
        .and_then(move |(sha256, new_id)| {
            if new_id != file_info.id {
                return Err(ErrorKind::Sha256Mismatch(file_info, sha256, new_id).into());
            }

            Ok(())
        });

    sha256_check.join(file_checks).map(|_| ())
}

pub fn content_checker_task(
    ctx: CoreContext,
    repo: BlobRepo,
    input: mpsc::Receiver<FileInformation>,
    error: mpsc::Sender<Error>,
) -> impl Future<Item = (), Error = ()> {
    let already_seen = Arc::new(Mutex::new(HashSet::new()));

    input
        .map({
            cloned!(already_seen, ctx, repo, error);
            move |file| {
                {
                    let mut already_seen = already_seen.lock().expect("lock poisoned");
                    if already_seen.contains(&file.id) {
                        return future::ok(()).left_future();
                    }

                    already_seen.insert(file.id);
                }

                spawn_future(check_one_file(file, ctx.clone(), repo.clone()).or_else({
                    cloned!(error);
                    move |err| error.send(err).map(|_| ()).map_err(|e| e.into_inner())
                })).map_err(|e| panic!("Could not queue error: {:#?}", e))
                    .right_future()
            }
        })
        .buffer_unordered(1000)
        .for_each(|id| Ok(id))
}
