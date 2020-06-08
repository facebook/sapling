/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{anyhow, Error, Result};
use context::CoreContext;
use futures_ext::BoxFuture;
use mercurial_types::{HgChangesetId, HgFileNodeId, HgNodeHash, RepoPath};
use mononoke_types::{hash, RepositoryId};
use quickcheck::{Arbitrary, Gen};

pub mod thrift {
    pub use filenodes_if::*;
}

pub fn blake2_path_hash(data: &Vec<u8>) -> hash::Blake2 {
    let mut hash_content = hash::Context::new("path".as_bytes());
    hash_content.update(data);
    hash_content.finish()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedFilenode {
    pub path: RepoPath,
    pub info: FilenodeInfo,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FilenodeInfo {
    pub filenode: HgFileNodeId,
    pub p1: Option<HgFileNodeId>,
    pub p2: Option<HgFileNodeId>,
    pub copyfrom: Option<(RepoPath, HgFileNodeId)>,
    pub linknode: HgChangesetId,
}

// The main purpose of FilenodeResult is to force callers to deal with situation
// when filenodes are disabled. This shouldn't happen normally, but it
// might happen in exceptional situation like e.g. filenodes db being
// unavailable.
//
// The guideline here is the following - if the code might affect the critical
// read path i.e. serving "hg pull"/"hg update" then it should not rely on
// filenodes being available and it needs to have a workaround.
#[derive(Debug)]
#[must_use]
pub enum FilenodeResult<T> {
    Present(T),
    Disabled,
}

impl<T> FilenodeResult<T> {
    pub fn map<U>(self, func: impl Fn(T) -> U) -> FilenodeResult<U> {
        match self {
            FilenodeResult::Present(t) => FilenodeResult::Present(func(t)),
            FilenodeResult::Disabled => FilenodeResult::Disabled,
        }
    }

    pub fn do_not_handle_disabled_filenodes(self) -> Result<T, Error> {
        match self {
            FilenodeResult::Present(t) => Ok(t),
            FilenodeResult::Disabled => Err(anyhow!("filenodes are disabled")),
        }
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
        info: Vec<PreparedFilenode>,
        repo_id: RepositoryId,
    ) -> BoxFuture<(), Error>;

    fn add_or_replace_filenodes(
        &self,
        ctx: CoreContext,
        info: Vec<PreparedFilenode>,
        repo_id: RepositoryId,
    ) -> BoxFuture<(), Error>;

    fn get_filenode(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        filenode: HgFileNodeId,
        repo_id: RepositoryId,
    ) -> BoxFuture<FilenodeResult<Option<FilenodeInfo>>, Error>;

    fn get_all_filenodes_maybe_stale(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        repo_id: RepositoryId,
    ) -> BoxFuture<FilenodeResult<Vec<FilenodeInfo>>, Error>;

    fn prime_cache(&self, ctx: &CoreContext, repo_id: RepositoryId, filenodes: &[PreparedFilenode]);
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
