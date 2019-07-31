// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::errors::ErrorKind;
use blobrepo::BlobRepo;
use cloned::cloned;
use context::CoreContext;
use failure_ext::Error;
use futures::{future, stream, sync::mpsc, Future, Sink, Stream};
use futures_ext::{spawn_future, FutureExt};
use mercurial_types::HgChangesetId;
use mononoke_types::{
    blob::BlobstoreValue, ChangesetId, ContentId, FileChange, FileContents, MPath,
};
use std::collections::HashSet;
use std::convert::TryFrom;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

fn check_bonsai_cs(
    cs_id: ChangesetId,
    ctx: CoreContext,
    repo: BlobRepo,
    cs_queue: mpsc::Sender<ChangesetId>,
    hg_cs_queue: mpsc::Sender<HgChangesetId>,
    file_queue: mpsc::Sender<FileInformation>,
) -> impl Future<Item = (), Error = Error> {
    let changeset = repo.get_bonsai_changeset(ctx.clone(), cs_id);
    let repo_parents = repo
        .get_changeset_parents_by_bonsai(ctx.clone(), cs_id)
        .and_then(move |parents| {
            // Add parents to the check queue ASAP - we'll validate them later
            stream::iter_ok(parents.clone())
                .forward(cs_queue)
                .map(move |_| parents)
        });

    changeset.join(repo_parents).and_then({
        move |(bcs, repo_parents)| {
            // If hash verification fails, abort early
            let hash = *bcs.clone().into_blob().id();
            if hash != cs_id {
                return future::err(ErrorKind::BadChangesetHash(cs_id, hash).into()).left_future();
            }

            // Check parents match
            let parents: Vec<_> = bcs.parents().collect();
            let repo_parents_ok = if repo_parents == parents {
                future::ok(())
            } else {
                future::err(ErrorKind::DbParentsMismatch(cs_id).into())
            };

            // Queue check on Mercurial equivalent
            let hg_cs = repo
                .get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
                .and_then(move |hg_cs| {
                    repo.get_bonsai_from_hg(ctx, hg_cs)
                        .and_then(move |new_id| {
                            // Verify symmetry of the mapping, too
                            match new_id {
                                Some(new_id) if cs_id == new_id => future::ok(()),
                                Some(new_id) => future::err(
                                    ErrorKind::HgMappingBroken(cs_id, hg_cs, new_id).into(),
                                ),
                                None => {
                                    future::err(ErrorKind::HgMappingNotPresent(cs_id, hg_cs).into())
                                }
                            }
                        })
                        .map(move |_| hg_cs)
                })
                .and_then(|hg_cs| hg_cs_queue.send(hg_cs).map(|_| ()).from_err());

            // Queue checks on files
            let file_changes: Vec<_> = bcs
                .file_changes()
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

            bcs_verifier
                .join4(queue_file_changes, repo_parents_ok, hg_cs)
                .map(|_| ())
                .right_future()
        }
    })
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

// Cheating for Eq and Hash - just compare cs_id
impl Eq for FileInformation {}

impl PartialEq for FileInformation {
    fn eq(&self, other: &Self) -> bool {
        self.cs_id == other.cs_id
    }
}

impl Hash for FileInformation {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.cs_id.hash(state)
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
    // TODO (T47717165): stream!
    let bytes = repo
        .get_file_content_by_content_id(ctx.clone(), file_info.id)
        .concat2();

    let file_checks = bytes.and_then({
        cloned!(file_info);
        move |file_bytes| {
            let bytes = file_bytes.into_bytes();

            let size = u64::try_from(bytes.len())?;
            if file_info.size != size {
                return Err(ErrorKind::BadContentSize(file_info, size).into());
            }

            let id = FileContents::new_bytes(bytes).content_id();
            if id != file_info.id {
                return Err(ErrorKind::BadContentId(file_info, id).into());
            }

            Ok(())
        }
    });

    let sha256_check = repo
        .get_file_sha256(ctx.clone(), file_info.id)
        .and_then(move |sha256| {
            repo.get_file_content_id_by_sha256(ctx, sha256)
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

fn check_hg_cs(
    cs: HgChangesetId,
    ctx: CoreContext,
    repo: BlobRepo,
    cs_queue: mpsc::Sender<ChangesetId>,
) -> impl Future<Item = (), Error = Error> {
    // Fetch the changeset and check its hash
    let changeset = repo
        .get_changeset_by_changesetid(ctx.clone(), cs)
        .and_then(move |changeset| {
            if changeset.get_changeset_id() == cs {
                future::ok(changeset)
            } else {
                future::err(
                    ErrorKind::HgChangesetIdMismatch(cs, changeset.get_changeset_id()).into(),
                )
            }
        });
    // And fetch its parents via the Bonsai route - this gets parents via Bonsai rules
    let bcs_parents = repo.get_changeset_parents(ctx.clone(), cs);

    changeset
        .join(bcs_parents)
        .and_then(move |(hg_cs, bcs_parents)| {
            // Queue its Mercurial parents for checking, in Bonsai form.
            // We do not need to do a symmetry check, as Bonsai <-> HG is 1:1, and the Bonsai
            // mapping will do a symmetry check.
            // While here, validate that we have the same parents in Bonsai form
            let parents: Vec<_> = hg_cs
                .p1()
                .into_iter()
                .chain(hg_cs.p2().into_iter())
                .map(HgChangesetId::new)
                .collect();

            if parents != bcs_parents {
                return future::err(ErrorKind::ParentsMismatch(cs).into()).left_future();
            }

            let queue_parents = stream::iter_ok(parents.into_iter())
                .and_then({
                    cloned!(repo, ctx);
                    move |hg_cs| {
                        repo.get_bonsai_from_hg(ctx.clone(), hg_cs)
                            .map(move |opt_cs| (hg_cs, opt_cs))
                    }
                })
                .and_then(move |(hg_cs, opt_cs)| {
                    if let Some(cs_id) = opt_cs {
                        future::ok(cs_id)
                    } else {
                        future::err(ErrorKind::HgDangling(hg_cs).into())
                    }
                })
                .forward(cs_queue.clone())
                .map(|_| ());

            // Queue the Bonsai of this CS for rechecking, too. Also a 1:1 mapping, but will
            // break if the mapping is bad and this CS is found via (e.g.) a linknode
            // The skipping of already checked CSes will avoid an infinite loop
            let queue_bonsai = repo
                .get_bonsai_from_hg(ctx, cs)
                .and_then(move |opt_cs| {
                    if let Some(cs_id) = opt_cs {
                        future::ok(cs_id)
                    } else {
                        future::err(ErrorKind::HgDangling(cs).into())
                    }
                })
                .and_then(move |cs| cs_queue.send(cs).map(|_| ()).from_err());
            queue_parents.join(queue_bonsai).map(|_| ()).right_future()
        })
}

fn checker_task<InHash, Spawner, F>(
    mut spawner: Spawner,
    input: mpsc::Receiver<InHash>,
    error: mpsc::Sender<Error>,
    queue_length: usize,
) -> impl Future<Item = (), Error = ()>
where
    InHash: Hash + Eq + Clone,
    Spawner: FnMut(InHash) -> F,
    F: Future<Item = (), Error = Error> + Send + 'static,
{
    let already_seen = Arc::new(Mutex::new(HashSet::new()));

    input
        .map({
            cloned!(already_seen, error);
            move |cs| {
                {
                    let mut already_seen = already_seen.lock().expect("lock poisoned");
                    if !already_seen.insert(cs.clone()) {
                        // Don't retry a known-good item
                        return future::ok(()).left_future();
                    }
                }

                spawn_future(spawner(cs).or_else({
                    cloned!(error);
                    move |err| error.send(err).map(|_| ()).map_err(|e| e.into_inner())
                }))
                .map_err(|e| panic!("Could not queue error: {:#?}", e))
                .right_future()
            }
        })
        .buffer_unordered(queue_length)
        .for_each(|id| Ok(id))
}

pub struct Checker {
    bonsai_to_check_sender: mpsc::Sender<ChangesetId>,
    bonsai_to_check_receiver: mpsc::Receiver<ChangesetId>,
    content_to_check_sender: mpsc::Sender<FileInformation>,
    content_to_check_receiver: mpsc::Receiver<FileInformation>,
    hg_changeset_to_check_sender: mpsc::Sender<HgChangesetId>,
    hg_changeset_to_check_receiver: mpsc::Receiver<HgChangesetId>,
}

impl Checker {
    pub fn new() -> Self {
        // This allows two parents to be sent by each changeset before blocking
        let (bonsai_to_check_sender, bonsai_to_check_receiver) = mpsc::channel(1);
        // Backpressure if files aren't being checked fast enough
        let (content_to_check_sender, content_to_check_receiver) = mpsc::channel(0);
        // Again with the two parents
        let (hg_changeset_to_check_sender, hg_changeset_to_check_receiver) = mpsc::channel(1);

        Self {
            bonsai_to_check_sender,
            bonsai_to_check_receiver,
            content_to_check_sender,
            content_to_check_receiver,
            hg_changeset_to_check_sender,
            hg_changeset_to_check_receiver,
        }
    }

    pub fn queue_root_commits<S, E>(&self, initial: S) -> impl Future<Item = (), Error = E>
    where
        S: Stream<Item = ChangesetId, Error = E>,
    {
        initial
            .forward(
                self.bonsai_to_check_sender
                    .clone()
                    .sink_map_err(|_| panic!("Checker failed")),
            )
            .map(|_| ())
    }

    pub fn spawn_tasks(self, ctx: CoreContext, repo: BlobRepo, error_sender: mpsc::Sender<Error>) {
        tokio::spawn(checker_task(
            {
                let (bcs, hgcs, ccs) = (
                    self.bonsai_to_check_sender.clone(),
                    self.hg_changeset_to_check_sender.clone(),
                    self.content_to_check_sender.clone(),
                );
                {
                    cloned!(ctx, repo);
                    move |hash| {
                        check_bonsai_cs(
                            hash,
                            ctx.clone(),
                            repo.clone(),
                            bcs.clone(),
                            hgcs.clone(),
                            ccs.clone(),
                        )
                    }
                }
            },
            self.bonsai_to_check_receiver,
            error_sender.clone(),
            1000,
        ));

        tokio::spawn(checker_task(
            {
                cloned!(ctx, repo);
                move |hash| check_one_file(hash, ctx.clone(), repo.clone())
            },
            self.content_to_check_receiver,
            error_sender.clone(),
            10000,
        ));

        tokio::spawn(checker_task(
            {
                cloned!(ctx, repo, self.bonsai_to_check_sender);
                move |hash| {
                    check_hg_cs(
                        hash,
                        ctx.clone(),
                        repo.clone(),
                        bonsai_to_check_sender.clone(),
                    )
                }
            },
            self.hg_changeset_to_check_receiver,
            error_sender,
            1000,
        ));
    }
}
