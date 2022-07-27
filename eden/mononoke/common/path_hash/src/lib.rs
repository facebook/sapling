/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use abomonation_derive::Abomonation;
use mononoke_types::hash;
use mononoke_types::path_bytes_from_mpath;
use mononoke_types::MPath;
use mononoke_types::RepoPath;
use sql::mysql;
use sql::mysql_async::prelude::ConvIr;
use sql::mysql_async::prelude::FromValue;
use sql::mysql_async::FromValueError;
use sql::mysql_async::Value;
use std::borrow::Borrow;
use std::borrow::Cow;
use std::hash::Hash;
#[derive(Abomonation, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[derive(mysql::OptTryFromRowField)]
pub struct PathHashBytes(pub Vec<u8>);

impl PathHashBytes {
    pub fn new(path_bytes: &[u8]) -> Self {
        let hash = {
            let mut hash_content = hash::Context::new("path".as_bytes());
            hash_content.update(&path_bytes);
            hash_content.finish()
        };

        PathHashBytes(Vec::from(hash.as_ref()))
    }
}

impl ConvIr<PathHashBytes> for Vec<u8> {
    fn new(v: Value) -> Result<Self, FromValueError> {
        Self::from_value_opt(v)
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

impl std::fmt::Display for PathHashBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        for byte in &self.0 {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}

#[derive(Abomonation, Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[derive(mysql::OptTryFromRowField)]
pub struct PathBytes(pub Vec<u8>);

impl ConvIr<PathBytes> for Vec<u8> {
    fn new(v: Value) -> Result<Self, FromValueError> {
        Self::from_value_opt(v)
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
    pub path: Cow<'a, RepoPath>,
    pub path_bytes: PathBytes,
    pub is_tree: bool,
    pub hash: PathHashBytes,
}

impl<'a> PathWithHash<'a> {
    pub fn from_repo_path(path: &'a RepoPath) -> Self {
        Self::from_repo_path_cow(Cow::Borrowed(path))
    }
    pub fn from_repo_path_cow(path: Cow<'a, RepoPath>) -> Self {
        let (path_bytes, is_tree) = convert_from_repo_path(path.borrow());

        let hash = PathHashBytes::new(&path_bytes);

        Self {
            path,
            path_bytes: PathBytes(path_bytes),
            is_tree,
            hash,
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
        if self.is_tree { &1 } else { &0 }
    }
}

fn convert_from_repo_path(path: &RepoPath) -> (Vec<u8>, bool) {
    let is_tree = match path {
        &RepoPath::RootPath | &RepoPath::DirectoryPath(_) => true,
        &RepoPath::FilePath(_) => false,
    };
    let bytes = path_bytes_from_mpath(path.mpath());
    (bytes, is_tree)
}

#[derive(Abomonation, Clone, Debug, Eq, PartialEq)]
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

    pub fn from_path_and_is_tree(path: Option<&MPath>, is_tree: bool) -> Self {
        let path_bytes = path_bytes_from_mpath(path);
        let hash = PathHashBytes::new(&path_bytes);

        Self {
            path_bytes: PathBytes(path_bytes),
            is_tree,
            hash,
        }
    }

    pub fn shard_number(&self, shard_count: usize) -> usize {
        PathWithHash::shard_number_by_hash(&self.hash, shard_count)
    }

    pub fn sql_is_tree(&self) -> &'static i8 {
        if self.is_tree { &1 } else { &0 }
    }
}
