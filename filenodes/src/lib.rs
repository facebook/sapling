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
use mercurial_types::{HgChangesetId, HgFileNodeId, RepoPath, RepositoryId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilenodeInfo {
    pub path: RepoPath,
    pub filenode: HgFileNodeId,
    pub p1: Option<HgFileNodeId>,
    pub p2: Option<HgFileNodeId>,
    pub copyfrom: Option<(RepoPath, HgFileNodeId)>,
    pub linknode: HgChangesetId,
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
        filenode: &HgFileNodeId,
        repo_id: &RepositoryId,
    ) -> BoxFuture<Option<FilenodeInfo>, Error>;
}
