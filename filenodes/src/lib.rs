// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate asyncmemo;
extern crate failure_ext as failure;
extern crate futures;

extern crate futures_ext;
extern crate mercurial_types;

use std::sync::Arc;

use asyncmemo::{Asyncmemo, Filler};
use failure::Error;
use futures::Future;
use futures_ext::{BoxFuture, BoxStream, FutureExt};
use mercurial_types::{DChangesetId, DFileNodeId, RepoPath, RepositoryId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilenodeInfo {
    pub path: RepoPath,
    pub filenode: DFileNodeId,
    pub p1: Option<DFileNodeId>,
    pub p2: Option<DFileNodeId>,
    pub copyfrom: Option<(RepoPath, DFileNodeId)>,
    pub linknode: DChangesetId,
}

impl asyncmemo::Weight for FilenodeInfo {
    fn get_weight(&self) -> usize {
        self.path.get_weight() + self.filenode.get_weight() + self.p1.get_weight()
            + self.p2.get_weight() + self.copyfrom.get_weight() + self.linknode.get_weight()
    }
}

pub struct CachingFilenodes {
    filenodes: Arc<Filenodes>,
    cache: asyncmemo::Asyncmemo<FilenodesFiller>,
}

impl CachingFilenodes {
    pub fn new(filenodes: Arc<Filenodes>, sizelimit: usize) -> Self {
        let cache = asyncmemo::Asyncmemo::with_limits(
            FilenodesFiller::new(filenodes.clone()),
            std::usize::MAX,
            sizelimit,
        );
        Self { filenodes, cache }
    }
}

impl Filenodes for CachingFilenodes {
    fn add_filenodes(
        &self,
        info: BoxStream<FilenodeInfo, Error>,
        repo_id: &RepositoryId,
    ) -> BoxFuture<(), Error> {
        self.filenodes.add_filenodes(info, repo_id)
    }

    fn get_filenode(
        &self,
        path: &RepoPath,
        filenode: &DFileNodeId,
        repo_id: &RepositoryId,
    ) -> BoxFuture<Option<FilenodeInfo>, Error> {
        self.cache
            .get((path.clone(), *filenode, *repo_id))
            .then(|val| match val {
                Ok(val) => Ok(Some(val)),
                Err(Some(err)) => Err(err),
                Err(None) => Ok(None),
            })
            .boxify()
    }
}

pub struct FilenodesFiller {
    filenodes: Arc<Filenodes>,
}

impl FilenodesFiller {
    fn new(filenodes: Arc<Filenodes>) -> Self {
        FilenodesFiller { filenodes }
    }
}

impl Filler for FilenodesFiller {
    type Key = (RepoPath, DFileNodeId, RepositoryId);
    type Value = Box<Future<Item = FilenodeInfo, Error = Option<Error>> + Send>;

    fn fill(
        &self,
        _cache: &Asyncmemo<Self>,
        &(ref path, ref filenode, ref repo_id): &Self::Key,
    ) -> Self::Value {
        self.filenodes
            .get_filenode(path, filenode, repo_id)
            .map_err(|err| Some(err))
            .and_then(|res| match res {
                Some(val) => Ok(val),
                None => Err(None),
            })
            .boxify()
    }
}

pub trait Filenodes: Send + Sync {
    fn add_filenodes(
        &self,
        info: BoxStream<FilenodeInfo, Error>,
        repo_id: &RepositoryId,
    ) -> BoxFuture<(), Error>;

    fn get_filenode(
        &self,
        path: &RepoPath,
        filenode: &DFileNodeId,
        repo_id: &RepositoryId,
    ) -> BoxFuture<Option<FilenodeInfo>, Error>;
}
