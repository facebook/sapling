// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate failure_ext as failure;
extern crate futures;

extern crate futures_ext;
extern crate mercurial_types;

use failure::Error;
use futures_ext::{BoxFuture, BoxStream};
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
