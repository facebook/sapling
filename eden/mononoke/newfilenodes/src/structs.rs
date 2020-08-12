/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use abomonation_derive::Abomonation;
use anyhow::Error;
use filenodes::FilenodeInfo;
use mercurial_types::{HgChangesetId, HgFileNodeId};
use mononoke_types::{hash, RepoPath};
use sql::mysql_async::{
    prelude::{ConvIr, FromValue},
    FromValueError, Value,
};
use std::cmp::{Eq, Ord, PartialEq, PartialOrd};
use std::convert::TryInto;
use std::hash::Hash;

use crate::local_cache::{CachePool, Cacheable};

#[derive(Abomonation, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PathHashBytes(pub Vec<u8>);

impl ConvIr<PathHashBytes> for Vec<u8> {
    fn new(v: Value) -> Result<Self, FromValueError> {
        Ok(Self::from_value_opt(v)?)
    }

    fn commit(self) -> PathHashBytes {
        PathHashBytes(self)
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for PathHashBytes {
    type Intermediate = Vec<u8>;
}

impl From<PathHashBytes> for Value {
    fn from(b: PathHashBytes) -> Self {
        b.0.into()
    }
}

#[derive(Abomonation, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PathBytes(pub Vec<u8>);

impl Cacheable for PathBytes {
    const POOL: CachePool = CachePool::Filenodes;
}

impl ConvIr<PathBytes> for Vec<u8> {
    fn new(v: Value) -> Result<Self, FromValueError> {
        Ok(Self::from_value_opt(v)?)
    }

    fn commit(self) -> PathBytes {
        PathBytes(self)
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for PathBytes {
    type Intermediate = Vec<u8>;
}

impl From<PathBytes> for Value {
    fn from(b: PathBytes) -> Self {
        b.0.into()
    }
}

#[derive(Clone)]
pub struct PathWithHash<'a> {
    pub path: &'a RepoPath,
    pub path_bytes: PathBytes,
    pub is_tree: bool,
    pub hash: PathHashBytes,
}

impl<'a> PathWithHash<'a> {
    pub fn from_repo_path(path: &'a RepoPath) -> Self {
        let (path_bytes, is_tree) = convert_from_repo_path(path);

        let hash = {
            let mut hash_content = hash::Context::new("path".as_bytes());
            hash_content.update(&path_bytes);
            hash_content.finish()
        };

        Self {
            path,
            path_bytes: PathBytes(path_bytes),
            is_tree,
            hash: PathHashBytes(Vec::from(hash.as_ref())),
        }
    }

    pub fn shard_number_by_hash(hash: &PathHashBytes, shard_count: usize) -> usize {
        // We don't need crypto strength here - we're just turning a potentially large hash into
        // a shard number.
        let raw_shard_number = hash
            .0
            .iter()
            .fold(0usize, |hash, byte| hash.rotate_left(8) ^ (*byte as usize));

        raw_shard_number % shard_count
    }

    pub fn sql_is_tree(&self) -> &'static i8 {
        if self.is_tree {
            &1
        } else {
            &0
        }
    }
}

fn convert_from_repo_path(path: &RepoPath) -> (Vec<u8>, bool) {
    match path {
        &RepoPath::RootPath => (vec![], true),
        &RepoPath::DirectoryPath(ref dir) => (dir.to_vec(), true),
        &RepoPath::FilePath(ref file) => (file.to_vec(), false),
    }
}

pub struct PathHash {
    pub path_bytes: PathBytes,
    pub is_tree: bool,
    pub hash: PathHashBytes,
}

impl PathHash {
    pub fn from_repo_path(path: &RepoPath) -> Self {
        let PathWithHash {
            path_bytes,
            is_tree,
            hash,
            ..
        } = PathWithHash::from_repo_path(path);

        Self {
            path_bytes,
            is_tree,
            hash,
        }
    }

    pub fn shard_number(&self, shard_count: usize) -> usize {
        PathWithHash::shard_number_by_hash(&self.hash, shard_count)
    }

    pub fn sql_is_tree(&self) -> &'static i8 {
        if self.is_tree {
            &1
        } else {
            &0
        }
    }
}

#[derive(Abomonation, Clone)]
pub struct CachedFilenode {
    pub filenode: HgFileNodeId,
    pub p1: Option<HgFileNodeId>,
    pub p2: Option<HgFileNodeId>,
    pub copyfrom: Option<(bool /* is_tree */, PathBytes, HgFileNodeId)>,
    pub linknode: HgChangesetId,
}

impl TryInto<FilenodeInfo> for CachedFilenode {
    type Error = Error;

    fn try_into(self) -> Result<FilenodeInfo, Error> {
        let Self {
            filenode,
            p1,
            p2,
            copyfrom,
            linknode,
        } = self;

        let copyfrom = match copyfrom {
            Some((is_tree, from_path_bytes, from_id)) => {
                let from_path = if is_tree {
                    RepoPath::dir(&from_path_bytes.0[..])?
                } else {
                    RepoPath::file(&from_path_bytes.0[..])?
                };
                Some((from_path, from_id))
            }
            None => None,
        };

        Ok(FilenodeInfo {
            filenode,
            p1,
            p2,
            copyfrom,
            linknode,
        })
    }
}

impl From<&'_ FilenodeInfo> for CachedFilenode {
    fn from(info: &'_ FilenodeInfo) -> Self {
        let FilenodeInfo {
            filenode,
            p1,
            p2,
            copyfrom,
            linknode,
        } = info;

        let copyfrom = match copyfrom {
            Some((from_path, from_id)) => {
                let (from_path_bytes, is_tree) = convert_from_repo_path(from_path);
                Some((is_tree, PathBytes(from_path_bytes), *from_id))
            }
            None => None,
        };

        Self {
            filenode: *filenode,
            p1: *p1,
            p2: *p2,
            copyfrom,
            linknode: *linknode,
        }
    }
}

impl Cacheable for CachedFilenode {
    const POOL: CachePool = CachePool::Filenodes;
}

#[derive(Abomonation, Clone)]
pub struct CachedHistory {
    // NOTE: We could store this more efficiently by deduplicating filenode IDs.
    pub history: Vec<CachedFilenode>,
}

impl Cacheable for Option<CachedHistory> {
    const POOL: CachePool = CachePool::FilenodesHistory;
}

impl From<&'_ Vec<FilenodeInfo>> for CachedHistory {
    fn from(history: &'_ Vec<FilenodeInfo>) -> Self {
        let history = history.iter().map(CachedFilenode::from).collect();
        Self { history }
    }
}

impl TryInto<Vec<FilenodeInfo>> for CachedHistory {
    type Error = Error;

    fn try_into(self) -> Result<Vec<FilenodeInfo>, Error> {
        let Self { history } = self;
        history.into_iter().map(|e| e.try_into()).collect()
    }
}
