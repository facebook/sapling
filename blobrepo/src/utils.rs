// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::mem;
use std::sync::{Arc, Mutex};

use cloned::cloned;
use failure_ext::Error;
use futures::future::Future;
use futures::stream;
use futures_ext::{BoxStream, StreamExt};

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
        repo.get_filenodes()
            .add_filenodes(ctx, self.prepare_filenodes(cs_id), repo.get_repoid())
            .map(move |_| cs_id)
    }

    /// Filenodes shouldn't normally be replaced
    /// This function should only be used if we need to fix up filenodes
    pub fn replace_filenodes(
        &self,
        ctx: CoreContext,
        cs_id: HgChangesetId,
        repo: &BlobRepo,
    ) -> impl Future<Item = HgChangesetId, Error = Error> + Send {
        repo.get_filenodes()
            .add_or_replace_filenodes(ctx, self.prepare_filenodes(cs_id), repo.get_repoid())
            .map(move |_| cs_id)
    }

    fn prepare_filenodes(&self, cs_id: HgChangesetId) -> BoxStream<FilenodeInfo, Error> {
        let filenodes = {
            let mut filenodes = self.filenodes.lock().expect("lock poisoned");
            mem::replace(&mut *filenodes, Vec::new())
        }
        .into_iter()
        .map({
            cloned!(cs_id);
            move |node_info| node_info.with_linknode(cs_id)
        });

        stream::iter_ok(filenodes).boxify()
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
