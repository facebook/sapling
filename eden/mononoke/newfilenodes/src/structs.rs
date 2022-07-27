/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use abomonation_derive::Abomonation;
use anyhow::Error;
use filenodes::FilenodeInfo;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mononoke_types::RepoPath;
use path_hash::PathBytes;

use crate::local_cache::CachePool;
use crate::local_cache::Cacheable;

impl Cacheable for PathBytes {
    const POOL: CachePool = CachePool::Filenodes;
}

fn convert_from_repo_path(path: &RepoPath) -> (Vec<u8>, bool) {
    match *path {
        RepoPath::RootPath => (vec![], true),
        RepoPath::DirectoryPath(ref dir) => (dir.to_vec(), true),
        RepoPath::FilePath(ref file) => (file.to_vec(), false),
    }
}

#[derive(Abomonation, Clone, Debug)]
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
