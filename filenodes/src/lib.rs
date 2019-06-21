// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

pub use crate::caching::CachingFilenodes;
use abomonation_derive::Abomonation;
use context::CoreContext;
use failure_ext::{Error, Result};
use futures_ext::{BoxFuture, BoxStream};
use mercurial_types::{HgChangesetId, HgFileNodeId, HgNodeHash, RepoPath};
use mononoke_types::{hash, RepoPathCached, RepositoryId};
use quickcheck::{Arbitrary, Gen};
use std::convert::TryFrom;

mod caching;
mod thrift {
    pub use filenodes_if::*;
}

pub fn blake2_path_hash(data: &Vec<u8>) -> hash::Blake2 {
    let mut hash_content = hash::Context::new("path".as_bytes());
    hash_content.update(data);
    hash_content.finish()
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

#[derive(Abomonation, Clone)]
pub struct FilenodeInfoCached {
    pub path: RepoPathCached,
    pub filenode: HgFileNodeId,
    pub p1: Option<HgFileNodeId>,
    pub p2: Option<HgFileNodeId>,
    pub copyfrom: Option<(RepoPathCached, HgFileNodeId)>,
    pub linknode: HgChangesetId,
}

impl From<FilenodeInfo> for FilenodeInfoCached {
    fn from(info: FilenodeInfo) -> Self {
        let FilenodeInfo {
            path,
            filenode,
            p1,
            p2,
            copyfrom,
            linknode,
        } = info;
        let path = path.into();
        let copyfrom = copyfrom.map(|(path, filenode_id)| (path.into(), filenode_id));
        FilenodeInfoCached {
            path,
            filenode,
            p1,
            p2,
            copyfrom,
            linknode,
        }
    }
}

impl TryFrom<FilenodeInfoCached> for FilenodeInfo {
    type Error = Error;

    fn try_from(info: FilenodeInfoCached) -> Result<Self> {
        let FilenodeInfoCached {
            path,
            filenode,
            p1,
            p2,
            copyfrom,
            linknode,
        } = info;
        let path = RepoPath::try_from(&path)?;
        let copyfrom = match copyfrom {
            None => None,
            Some((path, filenode_id)) => Some((RepoPath::try_from(&path)?, filenode_id)),
        };
        Ok(FilenodeInfo {
            path,
            filenode,
            p1,
            p2,
            copyfrom,
            linknode,
        })
    }
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

pub trait Filenodes: Send + Sync {
    fn add_filenodes(
        &self,
        ctx: CoreContext,
        info: BoxStream<FilenodeInfo, Error>,
        repo_id: RepositoryId,
    ) -> BoxFuture<(), Error>;

    fn get_filenode(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        filenode: HgFileNodeId,
        repo_id: RepositoryId,
    ) -> BoxFuture<Option<FilenodeInfo>, Error>;

    fn get_all_filenodes_maybe_stale(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        repo_id: RepositoryId,
    ) -> BoxFuture<Vec<FilenodeInfo>, Error>;
}

#[cfg(test)]
mod test {
    use super::*;
    use quickcheck::quickcheck;

    quickcheck! {
        fn filenodes_info_thrift_roundtrip(obj: FilenodeInfo) -> bool {
            let thrift_struct = obj.clone().into_thrift();
            let obj2 = FilenodeInfo::from_thrift(thrift_struct)
                .expect("converting a valid Thrift structure should always work");
            obj == obj2
        }
    }
}
