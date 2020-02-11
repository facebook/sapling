/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::mem;
use std::sync::{Arc, Mutex};

use anyhow::Error;
use cloned::cloned;
use futures::future::{self, Future};
use futures::stream;
use futures_ext::{FutureExt, StreamExt};
use itertools::{Either, Itertools};

use super::repo::BlobRepo;
use context::CoreContext;
use filenodes::FilenodeInfo;
use mercurial_types::{HgChangesetId, HgFileNodeId, RepoPath};

#[derive(Clone, Debug)]
pub struct IncompleteFilenodeInfo {
    pub path: RepoPath,
    pub filenode: HgFileNodeId,
    pub p1: Option<HgFileNodeId>,
    pub p2: Option<HgFileNodeId>,
    pub copyfrom: Option<(RepoPath, HgFileNodeId)>,
}

impl IncompleteFilenodeInfo {
    pub fn with_linknode(self, linknode: HgChangesetId) -> FilenodeInfo {
        let IncompleteFilenodeInfo {
            path,
            filenode,
            p1,
            p2,
            copyfrom,
        } = self;
        FilenodeInfo {
            path,
            filenode,
            p1,
            p2,
            copyfrom,
            linknode,
        }
    }
}

#[derive(Clone, Debug)]
pub struct IncompleteFilenodes {
    filenodes: Arc<Mutex<Vec<IncompleteFilenodeInfo>>>,
}

impl IncompleteFilenodes {
    pub fn new() -> Self {
        IncompleteFilenodes {
            filenodes: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn add(&self, filenode: IncompleteFilenodeInfo) {
        let mut filenodes = self.filenodes.lock().expect("lock poisoned");
        filenodes.push(filenode);
    }

    pub fn upload(
        &self,
        ctx: CoreContext,
        cs_id: HgChangesetId,
        repo: &BlobRepo,
    ) -> impl Future<Item = HgChangesetId, Error = Error> + Send {
        let filenodes = self.prepare_filenodes(cs_id);

        // upload() function does a few checks:
        // 1) it checks if root filenodes have their parents already uploaded and if not then
        //    it *does not* uploads any filenodes.
        // 2) If it decides to upload the filenodes, then it uploads root filenodes *after* all
        //    non-root filenodes are uploaded.
        //
        // The reason it's implemented this way is to make it possible to disentagle
        // filenode and hg changeset generation without causing breakages in production (see
        //  FilenodesOnlyPublic derived data).
        //
        // Let's assume we have two binaries running at the same time - first binary generates
        // filenodes while generating hg changesets and second does not (this situation is possible
        // because we update our binaries in stages).
        // Then we might end up in this situation
        //
        //      A
        //      |
        //      B <- This hg changeset was derived by the first binary, so it has filenodes
        //      |
        //      C <- hg changeset for this commit was derived by second binary, it might not yet have
        //      |    filenodes generated for it
        //     ...
        //
        // We ended up in an inconsistent state - root filenode for commit B is uploaded,
        // but we don't have any filenodes for commit C. This is a violation of the invariant.
        // The logic below doesn't allow this violation to occur
        // This logic should be temporary code that exists only to make migration smoother.

        let mut root_filenodes_parents_exist = vec![];
        let (root_filenodes, non_root_filenodes): (Vec<_>, Vec<_>) =
            filenodes.into_iter().partition_map(|filenode| {
                if filenode.path == RepoPath::RootPath {
                    let p1_exists = root_exists(&ctx, repo, &filenode.p1);
                    let p2_exists = root_exists(&ctx, repo, &filenode.p1);
                    root_filenodes_parents_exist.push(p1_exists);
                    root_filenodes_parents_exist.push(p2_exists);
                    Either::Left(filenode)
                } else {
                    Either::Right(filenode)
                }
            });

        let filenodes = repo.get_filenodes();
        let repoid = repo.get_repoid();
        future::join_all(root_filenodes_parents_exist).and_then(
            move |root_filenodes_parents_exist| {
                if root_filenodes_parents_exist.into_iter().all(|x| x) {
                    let s = stream::iter_ok(non_root_filenodes).boxify();
                    filenodes
                        .add_filenodes(ctx.clone(), s, repoid)
                        .and_then(move |_| {
                            let s = stream::iter_ok(root_filenodes).boxify();
                            filenodes.add_filenodes(ctx, s, repoid)
                        })
                        .map(move |_| cs_id)
                        .left_future()
                } else {
                    future::ok(cs_id).right_future()
                }
            },
        )
    }

    /// Filenodes shouldn't normally be replaced
    /// This function should only be used if we need to fix up filenodes
    pub fn replace_filenodes(
        &self,
        ctx: CoreContext,
        cs_id: HgChangesetId,
        repo: &BlobRepo,
    ) -> impl Future<Item = HgChangesetId, Error = Error> + Send {
        let filenodes = self.prepare_filenodes(cs_id);

        let s = stream::iter_ok(filenodes).boxify();
        repo.get_filenodes()
            .add_or_replace_filenodes(ctx, s, repo.get_repoid())
            .map(move |_| cs_id)
    }

    fn prepare_filenodes(&self, cs_id: HgChangesetId) -> Vec<FilenodeInfo> {
        let filenodes = {
            let mut filenodes = self.filenodes.lock().expect("lock poisoned");
            mem::replace(&mut *filenodes, Vec::new())
        }
        .into_iter()
        .map({
            cloned!(cs_id);
            move |node_info| node_info.with_linknode(cs_id)
        });

        filenodes.collect()
    }
}

fn root_exists(
    ctx: &CoreContext,
    repo: &BlobRepo,
    node: &Option<HgFileNodeId>,
) -> impl Future<Item = bool, Error = Error> {
    match node {
        Some(p) => repo
            .get_filenodes()
            .get_filenode(ctx.clone(), &RepoPath::RootPath, *p, repo.get_repoid())
            .map(|filenode| filenode.is_some())
            .left_future(),
        None => future::ok(true).right_future(),
    }
}

/// Create new instance of implementing object with overridden field of spcecified type.
///
/// This override can be very dangerous, it should only be used in unittest, or if you
/// really know what you are doing.
pub trait DangerousOverride<T> {
    fn dangerous_override<F>(&self, modify: F) -> Self
    where
        F: FnOnce(T) -> T;
}
