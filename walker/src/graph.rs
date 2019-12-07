/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::{format_err, Error};
use bookmarks::BookmarkName;
use filenodes::FilenodeInfo;
use futures_ext::BoxStream;
use mercurial_types::{
    blobs::HgBlobChangeset, FileBytes, HgChangesetId, HgFileEnvelope, HgFileNodeId, HgManifest,
    HgManifestId,
};
use mononoke_types::{BonsaiChangeset, ChangesetId, ContentId, ContentMetadata, MPath};
use std::fmt;
use std::str::FromStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeType {
    Bookmark,
    BonsaiChangeset,
    BonsaiChangesetFromHgChangeset,
    BonsaiParents,
    HgChangesetFromBonsaiChangeset,
    HgChangeset,
    HgManifest,
    HgFileEnvelope,
    HgFileNode,
    FileContent,
    FileContentMetadata,
}

impl fmt::Display for NodeType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl FromStr for NodeType {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Bookmark" => Ok(NodeType::Bookmark),
            "BonsaiChangeset" => Ok(NodeType::BonsaiChangeset),
            "BonsaiChangesetFromHgChangeset" => Ok(NodeType::BonsaiChangesetFromHgChangeset),
            "BonsaiParents" => Ok(NodeType::BonsaiParents),
            "HgChangesetFromBonsaiChangeset" => Ok(NodeType::HgChangesetFromBonsaiChangeset),
            "HgChangeset" => Ok(NodeType::HgChangeset),
            "HgManifest" => Ok(NodeType::HgManifest),
            "HgFileEnvelope" => Ok(NodeType::HgFileEnvelope),
            "HgFileNode" => Ok(NodeType::HgFileNode),
            "FileContent" => Ok(NodeType::FileContent),
            "FileContentMetadata" => Ok(NodeType::FileContentMetadata),
            _ => Err(format_err!("Unknown node type {}", s)),
        }
    }
}

// Set of keys to look up items by, name is the type of lookup, payload is the key used.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Node {
    Bookmark(BookmarkName),
    BonsaiChangeset(ChangesetId),
    BonsaiChangesetFromHgChangeset(HgChangesetId),
    HgChangesetFromBonsaiChangeset(ChangesetId),
    BonsaiParents(ChangesetId),
    HgChangeset(HgChangesetId),
    HgManifest((Option<MPath>, HgManifestId)),
    HgFileEnvelope(HgFileNodeId),
    HgFileNode((Option<MPath>, HgFileNodeId)),
    FileContent(ContentId),
    FileContentMetadata(ContentId),
}

/// File content gets a special two-state content so we can chose when to read the data
pub enum FileContentData {
    ContentStream(BoxStream<FileBytes, Error>),
    Consumed(usize),
}

/// The data from the walk - this is the "full" form but not necessarily fully loaded.
/// e.g. file content streams are passed to you to read, they aren't pre-loaded to bytes.
pub enum NodeData {
    Bookmark(ChangesetId),
    BonsaiChangeset(BonsaiChangeset),
    BonsaiChangesetFromHgChangeset(Option<ChangesetId>),
    HgChangesetFromBonsaiChangeset(HgChangesetId),
    BonsaiParents(Vec<ChangesetId>),
    HgChangeset(HgBlobChangeset),
    HgManifest(Box<dyn HgManifest + Sync>),
    HgFileEnvelope(HgFileEnvelope),
    HgFileNode(Option<FilenodeInfo>),
    FileContent(FileContentData),
    FileContentMetadata(ContentMetadata),
}

impl Node {
    pub fn get_type(self: &Self) -> NodeType {
        match self {
            Node::Bookmark(_) => NodeType::Bookmark,
            Node::BonsaiChangeset(_) => NodeType::BonsaiChangeset,
            Node::BonsaiChangesetFromHgChangeset(_) => NodeType::BonsaiChangesetFromHgChangeset,
            Node::BonsaiParents(_) => NodeType::BonsaiParents,
            Node::HgChangesetFromBonsaiChangeset(_) => NodeType::HgChangesetFromBonsaiChangeset,
            Node::HgChangeset(_) => NodeType::HgChangeset,
            Node::HgManifest(_) => NodeType::HgManifest,
            Node::HgFileEnvelope(_) => NodeType::HgFileEnvelope,
            Node::HgFileNode(_) => NodeType::HgFileNode,
            Node::FileContent(_) => NodeType::FileContent,
            Node::FileContentMetadata(_) => NodeType::FileContentMetadata,
        }
    }
}
