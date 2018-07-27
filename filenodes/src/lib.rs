// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate asyncmemo;
extern crate failure_ext as failure;
extern crate futures;
#[cfg_attr(test, macro_use)]
extern crate quickcheck;

extern crate filenodes_if;
extern crate futures_ext;
extern crate mercurial_types;

use std::sync::Arc;

use asyncmemo::{Asyncmemo, Filler};
use failure::{Error, Result};
use futures::Future;
use futures_ext::{BoxFuture, BoxStream, FutureExt};
use mercurial_types::{HgChangesetId, HgFileNodeId, HgNodeHash, RepoPath, RepositoryId};
use quickcheck::{Arbitrary, Gen};

mod thrift {
    pub use filenodes_if::*;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilenodeInfo {
    pub path: RepoPath,
    pub filenode: HgFileNodeId,
    pub p1: Option<HgFileNodeId>,
    pub p2: Option<HgFileNodeId>,
    pub copyfrom: Option<(RepoPath, HgFileNodeId)>,
    pub linknode: HgChangesetId,
}

impl FilenodeInfo {
    pub fn from_thrift(info: thrift::FilenodeInfo) -> Result<Self> {
        let catch_block = || {
            let copyfrom = match info.copyfrom {
                None => None,
                Some(copyfrom) => Some((
                    RepoPath::from_thrift(copyfrom.path)?,
                    HgFileNodeId::new(HgNodeHash::from_thrift(copyfrom.filenode)?),
                )),
            };

            Ok(Self {
                path: RepoPath::from_thrift(info.path)?,
                filenode: HgFileNodeId::new(HgNodeHash::from_thrift(info.filenode)?),
                p1: HgNodeHash::from_thrift_opt(info.p1)?.map(HgFileNodeId::new),
                p2: HgNodeHash::from_thrift_opt(info.p2)?.map(HgFileNodeId::new),
                copyfrom,
                linknode: HgChangesetId::new(HgNodeHash::from_thrift(info.linknode)?),
            })
        };

        catch_block()
    }

    pub fn into_thrift(self) -> thrift::FilenodeInfo {
        thrift::FilenodeInfo {
            path: self.path.into_thrift(),
            filenode: self.filenode.into_nodehash().into_thrift(),
            p1: self.p1.map(|p| p.into_nodehash().into_thrift()),
            p2: self.p2.map(|p| p.into_nodehash().into_thrift()),
            copyfrom: self.copyfrom.map(|copyfrom| thrift::FilenodeCopyFrom {
                path: copyfrom.0.into_thrift(),
                filenode: copyfrom.1.into_nodehash().into_thrift(),
            }),
            linknode: self.linknode.into_nodehash().into_thrift(),
        }
    }
}

impl asyncmemo::Weight for FilenodeInfo {
    fn get_weight(&self) -> usize {
        self.path.get_weight() + self.filenode.get_weight() + self.p1.get_weight()
            + self.p2.get_weight() + self.copyfrom.get_weight() + self.linknode.get_weight()
    }
}

impl Arbitrary for FilenodeInfo {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        Self {
            path: RepoPath::arbitrary(g),
            filenode: HgFileNodeId::arbitrary(g),
            p1: <Option<HgFileNodeId>>::arbitrary(g),
            p2: <Option<HgFileNodeId>>::arbitrary(g),
            copyfrom: <Option<(RepoPath, HgFileNodeId)>>::arbitrary(g),
            linknode: HgChangesetId::arbitrary(g),
        }
    }
}

pub struct CachingFilenodes {
    filenodes: Arc<Filenodes>,
    cache: asyncmemo::Asyncmemo<FilenodesFiller>,
}

impl CachingFilenodes {
    pub fn new(filenodes: Arc<Filenodes>, sizelimit: usize) -> Self {
        let cache = asyncmemo::Asyncmemo::with_limits(
            "filenodes",
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
        filenode: &HgFileNodeId,
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

    fn get_all_filenodes(
        &self,
        path: &RepoPath,
        repo_id: &RepositoryId,
    ) -> BoxFuture<Vec<FilenodeInfo>, Error> {
        self.filenodes.get_all_filenodes(path, repo_id)
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
    type Key = (RepoPath, HgFileNodeId, RepositoryId);
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
        filenode: &HgFileNodeId,
        repo_id: &RepositoryId,
    ) -> BoxFuture<Option<FilenodeInfo>, Error>;

    fn get_all_filenodes(
        &self,
        path: &RepoPath,
        repo_id: &RepositoryId,
    ) -> BoxFuture<Vec<FilenodeInfo>, Error>;
}

#[cfg(test)]
mod test {
    use super::*;

    quickcheck! {
        fn filenodes_info_thrift_roundtrip(obj: FilenodeInfo) -> bool {
            let thrift_struct = obj.clone().into_thrift();
            let obj2 = FilenodeInfo::from_thrift(thrift_struct)
                .expect("converting a valid Thrift structure should always work");
            obj == obj2
        }
    }
}
