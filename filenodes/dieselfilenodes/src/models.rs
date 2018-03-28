// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use common::blake2_path_hash;
use mercurial_types::{HgChangesetId, HgFileNodeId, RepositoryId};
use schema::{filenodes, paths};

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
}

impl FilenodeRow {
    pub fn new(
        repo_id: &RepositoryId,
        path: &Vec<u8>,
        is_tree: i32,
        filenode: &HgFileNodeId,
        linknode: &HgChangesetId,
        p1: Option<HgFileNodeId>,
        p2: Option<HgFileNodeId>,
    ) -> Self {
        FilenodeRow {
            repo_id: *repo_id,
            path_hash: blake2_path_hash(path),
            is_tree,
            filenode: *filenode,
            linknode: *linknode,
            p1: p1,
            p2: p2,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[derive(Insertable)]
#[table_name = "paths"]
pub(crate) struct PathInsertRow {
    pub repo_id: RepositoryId,
    pub path_hash: Vec<u8>,
    pub path: Vec<u8>,
}

impl PathInsertRow {
    pub fn new(repo_id: &RepositoryId, path: Vec<u8>) -> Self {
        PathInsertRow {
            repo_id: *repo_id,
            path_hash: blake2_path_hash(&path),
            path,
        }
    }
}
