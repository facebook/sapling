/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::Blake3;
use crate::FileType;
use crate::HgId;
use crate::Id20;
use crate::RepoPathBuf;

#[derive(Clone, Debug)]
pub struct AugmentedFileNode {
    pub file_type: FileType,
    pub filenode: HgId,
    pub content_blake3: Blake3,
    pub content_sha1: Id20,
    pub total_size: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AugmentedDirectoryNode {
    pub treenode: HgId,
    pub augmented_manifest_id: Blake3,
    pub augmented_manifest_size: u64,
}

#[derive(Clone, Debug)]
pub enum AugmentedTreeChildEntry {
    FileNode(AugmentedFileNode),
    DirectoryNode(AugmentedDirectoryNode),
}

#[derive(Debug, Clone)]
pub struct AugmentedTreeEntry {
    pub hg_node_id: HgId,
    pub p1: Option<HgId>,
    pub p2: Option<HgId>,
    pub subentries: Vec<(RepoPathBuf, AugmentedTreeChildEntry)>,
}
