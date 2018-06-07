// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use common::blake2_path_hash;
use mercurial_types::{HgChangesetId, HgFileNodeId, RepositoryId};
use schema::{filenodes, fixedcopyinfo, paths};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[derive(Insertable, Queryable)]
#[table_name = "filenodes"]
pub(crate) struct FilenodeRow {
    // Diesel doesn't support unsigned types.
    // TODO (sid0) T26215455: use a custom type here
    pub repo_id: RepositoryId,
    pub path_hash: Vec<u8>,
    pub is_tree: i32,
    pub filenode: HgFileNodeId,
    // TODO(stash): shouldn't it be Mononoke changeset id?
    pub linknode: HgChangesetId,
    pub p1: Option<HgFileNodeId>,
    pub p2: Option<HgFileNodeId>,
    pub has_copyinfo: i32,
}

impl FilenodeRow {
    pub(crate) fn new(
        repo_id: &RepositoryId,
        path: &Vec<u8>,
        is_tree: i32,
        filenode: &HgFileNodeId,
        linknode: &HgChangesetId,
        p1: Option<HgFileNodeId>,
        p2: Option<HgFileNodeId>,
        has_copyinfo: bool,
    ) -> Self {
        let has_copyinfo = if has_copyinfo { 1 } else { 0 };
        FilenodeRow {
            repo_id: *repo_id,
            path_hash: blake2_path_hash(path),
            is_tree,
            filenode: *filenode,
            linknode: *linknode,
            p1: p1,
            p2: p2,
            has_copyinfo,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[derive(Insertable, Queryable)]
#[table_name = "paths"]
pub(crate) struct PathRow {
    pub repo_id: RepositoryId,
    pub path_hash: Vec<u8>,
    pub path: Vec<u8>,
}

impl PathRow {
    pub(crate) fn new(repo_id: &RepositoryId, path: Vec<u8>) -> Self {
        PathRow {
            repo_id: *repo_id,
            path_hash: blake2_path_hash(&path),
            path,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[derive(Insertable, Queryable)]
#[table_name = "fixedcopyinfo"]
pub(crate) struct FixedCopyInfoRow {
    pub repo_id: RepositoryId,
    pub frompath_hash: Vec<u8>,
    pub fromnode: HgFileNodeId,
    is_tree: i32,
    pub topath_hash: Vec<u8>,
    pub tonode: HgFileNodeId,
}

impl FixedCopyInfoRow {
    pub(crate) fn new(
        repo_id: &RepositoryId,
        frompath: &Vec<u8>,
        fromnode: &HgFileNodeId,
        is_tree: i32,
        topath: &Vec<u8>,
        tonode: &HgFileNodeId,
    ) -> Self {
        let frompath_hash = blake2_path_hash(frompath);
        let topath_hash = blake2_path_hash(topath);

        FixedCopyInfoRow {
            repo_id: *repo_id,
            frompath_hash,
            fromnode: *fromnode,
            is_tree,
            topath_hash,
            tonode: *tonode,
        }
    }
}
