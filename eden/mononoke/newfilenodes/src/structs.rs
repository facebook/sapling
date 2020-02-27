/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::local_cache::{CachePool, Cacheable};
use abomonation_derive::Abomonation;
use mercurial_types::{HgChangesetId, HgFileNodeId};
use mononoke_types::{hash, RepoPath};
use sql::mysql_async::{
    prelude::{ConvIr, FromValue},
    FromValueError, Value,
};
use std::cmp::{Eq, Ord, PartialEq, PartialOrd};
use std::hash::Hash;

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
    pub is_tree: i8,
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

    pub fn shard_number(&self, shard_count: usize) -> usize {
        Self::shard_number_by_hash(&self.hash, shard_count)
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
}

fn convert_from_repo_path(path: &RepoPath) -> (Vec<u8>, i8) {
    match path {
        &RepoPath::RootPath => (vec![], 1),
        &RepoPath::DirectoryPath(ref dir) => (dir.to_vec(), 1),
        &RepoPath::FilePath(ref file) => (file.to_vec(), 0),
    }
}

pub struct PathHash {
    pub path_bytes: PathBytes,
    pub is_tree: i8,
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
}

#[derive(Abomonation, Clone)]
pub struct PartialFilenode {
    pub filenode: HgFileNodeId,
    pub p1: Option<HgFileNodeId>,
    pub p2: Option<HgFileNodeId>,
    pub copyfrom: Option<(PathHashBytes, HgFileNodeId)>,
    pub linknode: HgChangesetId,
}

impl Cacheable for PartialFilenode {
    const POOL: CachePool = CachePool::Filenodes;
}

#[derive(Abomonation, Clone)]
pub struct PartialHistory {
    // NOTE: We could store this more efficiently by deduplicating filenode IDs.
    pub history: Vec<PartialFilenode>,
}

impl Cacheable for PartialHistory {
    const POOL: CachePool = CachePool::FilenodesHistory;
}
