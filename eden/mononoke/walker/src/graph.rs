/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use bookmarks::BookmarkName;
use filenodes::FilenodeInfo;
use filestore::Alias;
use futures::stream::BoxStream;
use mercurial_types::{
    blobs::{BlobManifest, HgBlobChangeset},
    FileBytes, HgChangesetId, HgFileEnvelope, HgFileNodeId, HgManifestId,
};
use mononoke_types::{fsnode::Fsnode, FsnodeId};
use mononoke_types::{BonsaiChangeset, ChangesetId, ContentId, ContentMetadata, MPath, MononokeId};
use phases::Phase;
use std::{fmt, str::FromStr, sync::Arc};

// Helper to save repetition for the type enums
macro_rules! define_type_enum {
     (enum $enum_name:ident {
         $($variant:ident),*,
     }) => {
         #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
         pub enum $enum_name {
             $($variant),*
         }

         impl FromStr for $enum_name {
             type Err = Error;
             fn from_str(s: &str) -> Result<Self, Self::Err> {
                 match s {
                     $(stringify!($variant)=>Ok($enum_name::$variant),)*
                     _ => Err(format_err!("Unknown {} {}",stringify!($enum_name), s)),
                 }
             }
         }

        impl $enum_name {
            pub const ALL_VARIANTS: &'static [$enum_name] = &[
                $($enum_name::$variant),*
            ];

            #[allow(dead_code)]
            pub fn to_str(&self) -> &'static str {
                match self {
                    $($enum_name::$variant=>stringify!($variant),)*
                }
            }
        }
    }
}

define_type_enum! {
enum NodeType {
    Root,
    // Bonsai
    Bookmark,
    BonsaiChangeset,
    BonsaiHgMapping,
    BonsaiPhaseMapping,
    PublishedBookmarks,
    // Hg
    HgBonsaiMapping,
    HgChangeset,
    HgManifest,
    HgFileEnvelope,
    HgFileNode,
    // Content
    FileContent,
    FileContentMetadata,
    AliasContentMapping,
    // Derived data
    BonsaiFsnodeMapping,
    Fsnode,
}
}

impl fmt::Display for NodeType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl NodeType {
    pub fn root_edge_type(&self) -> Option<EdgeType> {
        match self {
            NodeType::Root => None,
            // Bonsai
            NodeType::Bookmark => Some(EdgeType::RootToBookmark),
            NodeType::BonsaiChangeset => Some(EdgeType::RootToBonsaiChangeset),
            NodeType::BonsaiHgMapping => Some(EdgeType::RootToBonsaiHgMapping),
            NodeType::BonsaiPhaseMapping => Some(EdgeType::RootToBonsaiPhaseMapping),
            NodeType::PublishedBookmarks => Some(EdgeType::RootToPublishedBookmarks),
            // Hg
            NodeType::HgBonsaiMapping => Some(EdgeType::RootToHgBonsaiMapping),
            NodeType::HgChangeset => Some(EdgeType::RootToHgChangeset),
            NodeType::HgManifest => Some(EdgeType::RootToHgManifest),
            NodeType::HgFileEnvelope => Some(EdgeType::RootToHgFileEnvelope),
            NodeType::HgFileNode => Some(EdgeType::RootToHgFileNode),
            // Content
            NodeType::FileContent => Some(EdgeType::RootToFileContent),
            NodeType::FileContentMetadata => Some(EdgeType::RootToFileContentMetadata),
            NodeType::AliasContentMapping => Some(EdgeType::RootToAliasContentMapping),
            // Derived data
            NodeType::BonsaiFsnodeMapping => Some(EdgeType::RootToBonsaiFsnodeMapping),
            NodeType::Fsnode => Some(EdgeType::RootToFsnode),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum WrappedPath {
    Root,
    NonRoot(Arc<MPath>),
}

impl WrappedPath {
    pub fn as_ref(&self) -> Option<&MPath> {
        match self {
            WrappedPath::Root => None,
            WrappedPath::NonRoot(path) => Some(path.as_ref()),
        }
    }

    pub fn sampling_fingerprint(&self) -> Option<u64> {
        self.as_ref()
            .map(|o| o.get_path_hash().sampling_fingerprint())
    }
}

impl From<Option<MPath>> for WrappedPath {
    fn from(mpath: Option<MPath>) -> Self {
        match mpath {
            Some(mpath) => WrappedPath::NonRoot(Arc::new(mpath)),
            None => WrappedPath::Root,
        }
    }
}

// Set of keys to look up items by, name is the type of lookup, payload is the key used.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Node {
    Root,
    // Bonsai
    Bookmark(BookmarkName),
    BonsaiChangeset(ChangesetId),
    BonsaiHgMapping(ChangesetId),
    BonsaiPhaseMapping(ChangesetId),
    PublishedBookmarks,
    // Hg
    HgBonsaiMapping(HgChangesetId),
    HgChangeset(HgChangesetId),
    HgManifest((WrappedPath, HgManifestId)),
    HgFileEnvelope(HgFileNodeId),
    HgFileNode((WrappedPath, HgFileNodeId)),
    // Content
    FileContent(ContentId),
    FileContentMetadata(ContentId),
    AliasContentMapping(Alias),
    // Derived data
    BonsaiFsnodeMapping(ChangesetId),
    Fsnode((WrappedPath, FsnodeId)),
}

// Some Node types are accessible by more than one type of edge, this allows us to restrict the paths
// This is really a declaration of the steps a walker can take.
define_type_enum! {
enum EdgeType {
    // Bonsai Roots
    RootToBookmark,
    RootToBonsaiChangeset,
    RootToBonsaiHgMapping,
    RootToBonsaiPhaseMapping,
    RootToPublishedBookmarks,
    // Hg Roots
    RootToHgBonsaiMapping,
    RootToHgChangeset,
    RootToHgManifest,
    RootToHgFileEnvelope,
    RootToHgFileNode,
    // Content Roots
    RootToFileContent,
    RootToFileContentMetadata,
    RootToAliasContentMapping,
    // Derived data Roots
    RootToBonsaiFsnodeMapping,
    RootToFsnode,
    // Bonsai
    BookmarkToBonsaiChangeset,
    BookmarkToBonsaiHgMapping,
    BonsaiChangesetToFileContent,
    BonsaiChangesetToBonsaiParent,
    BonsaiChangesetToBonsaiHgMapping,
    BonsaiChangesetToBonsaiPhaseMapping,
    BonsaiHgMappingToHgChangeset,
    PublishedBookmarksToBonsaiChangeset,
    PublishedBookmarksToBonsaiHgMapping,
    BonsaiChangesetToBonsaiFsnodeMapping,
    // Hg
    HgBonsaiMappingToBonsaiChangeset,
    HgChangesetToHgParent,
    HgChangesetToHgManifest,
    HgManifestToHgFileEnvelope,
    HgManifestToHgFileNode,
    HgManifestToChildHgManifest,
    HgFileEnvelopeToFileContent,
    HgLinkNodeToHgBonsaiMapping,
    HgLinkNodeToHgChangeset,
    HgFileNodeToHgParentFileNode,
    HgFileNodeToHgCopyfromFileNode,
    // Content
    FileContentToFileContentMetadata,
    FileContentMetadataToSha1Alias,
    FileContentMetadataToSha256Alias,
    FileContentMetadataToGitSha1Alias,
    AliasContentMappingToFileContent,
    // Derived data
    BonsaiToRootFsnode,
    FsnodeToChildFsnode,
}
}

define_type_enum! {
    enum AliasType {
        GitSha1,
        Sha1,
        Sha256,
    }
}

impl EdgeType {
    pub fn incoming_type(&self) -> Option<NodeType> {
        match self {
            // Bonsai Roots
            EdgeType::RootToBookmark => None,
            EdgeType::RootToBonsaiChangeset => None,
            EdgeType::RootToBonsaiHgMapping => None,
            EdgeType::RootToBonsaiPhaseMapping => None,
            EdgeType::RootToPublishedBookmarks => None,
            // Hg Roots
            EdgeType::RootToHgBonsaiMapping => None,
            EdgeType::RootToHgChangeset => None,
            EdgeType::RootToHgManifest => None,
            EdgeType::RootToHgFileEnvelope => None,
            EdgeType::RootToHgFileNode => None,
            // Content Roots
            EdgeType::RootToFileContent => None,
            EdgeType::RootToFileContentMetadata => None,
            EdgeType::RootToAliasContentMapping => None,
            // Derived data Roots
            EdgeType::RootToBonsaiFsnodeMapping => None,
            EdgeType::RootToFsnode => None,
            // Bonsai
            EdgeType::BookmarkToBonsaiChangeset => Some(NodeType::Bookmark),
            EdgeType::BookmarkToBonsaiHgMapping => Some(NodeType::Bookmark),
            EdgeType::BonsaiChangesetToFileContent => Some(NodeType::BonsaiChangeset),
            EdgeType::BonsaiChangesetToBonsaiParent => Some(NodeType::BonsaiChangeset),
            EdgeType::BonsaiChangesetToBonsaiHgMapping => Some(NodeType::BonsaiChangeset),
            EdgeType::BonsaiChangesetToBonsaiPhaseMapping => Some(NodeType::BonsaiChangeset),
            EdgeType::BonsaiHgMappingToHgChangeset => Some(NodeType::BonsaiHgMapping),
            EdgeType::PublishedBookmarksToBonsaiChangeset => Some(NodeType::PublishedBookmarks),
            EdgeType::PublishedBookmarksToBonsaiHgMapping => Some(NodeType::PublishedBookmarks),
            EdgeType::BonsaiChangesetToBonsaiFsnodeMapping => Some(NodeType::BonsaiChangeset),
            // Hg
            EdgeType::HgBonsaiMappingToBonsaiChangeset => Some(NodeType::HgBonsaiMapping),
            EdgeType::HgChangesetToHgParent => Some(NodeType::HgChangeset),
            EdgeType::HgChangesetToHgManifest => Some(NodeType::HgChangeset),
            EdgeType::HgManifestToHgFileEnvelope => Some(NodeType::HgManifest),
            EdgeType::HgManifestToHgFileNode => Some(NodeType::HgManifest),
            EdgeType::HgManifestToChildHgManifest => Some(NodeType::HgManifest),
            EdgeType::HgFileEnvelopeToFileContent => Some(NodeType::HgFileEnvelope),
            EdgeType::HgLinkNodeToHgBonsaiMapping => Some(NodeType::HgFileNode),
            EdgeType::HgLinkNodeToHgChangeset => Some(NodeType::HgFileNode),
            EdgeType::HgFileNodeToHgParentFileNode => Some(NodeType::HgFileNode),
            EdgeType::HgFileNodeToHgCopyfromFileNode => Some(NodeType::HgFileNode),
            // Content
            EdgeType::FileContentToFileContentMetadata => Some(NodeType::FileContent),
            EdgeType::FileContentMetadataToSha1Alias => Some(NodeType::FileContentMetadata),
            EdgeType::FileContentMetadataToSha256Alias => Some(NodeType::FileContentMetadata),
            EdgeType::FileContentMetadataToGitSha1Alias => Some(NodeType::FileContentMetadata),
            EdgeType::AliasContentMappingToFileContent => Some(NodeType::AliasContentMapping),
            // Derived data
            EdgeType::BonsaiToRootFsnode => Some(NodeType::BonsaiFsnodeMapping),
            EdgeType::FsnodeToChildFsnode => Some(NodeType::Fsnode),
        }
    }
    pub fn outgoing_type(&self) -> NodeType {
        match self {
            // Bonsai Roots
            EdgeType::RootToBookmark => NodeType::Bookmark,
            EdgeType::RootToBonsaiChangeset => NodeType::BonsaiChangeset,
            EdgeType::RootToBonsaiHgMapping => NodeType::BonsaiHgMapping,
            EdgeType::RootToBonsaiPhaseMapping => NodeType::BonsaiPhaseMapping,
            EdgeType::RootToPublishedBookmarks => NodeType::PublishedBookmarks,
            // Hg Roots
            EdgeType::RootToHgBonsaiMapping => NodeType::HgBonsaiMapping,
            EdgeType::RootToHgChangeset => NodeType::HgChangeset,
            EdgeType::RootToHgManifest => NodeType::HgManifest,
            EdgeType::RootToHgFileEnvelope => NodeType::HgFileEnvelope,
            EdgeType::RootToHgFileNode => NodeType::HgFileNode,
            // Content Roots
            EdgeType::RootToFileContent => NodeType::FileContent,
            EdgeType::RootToFileContentMetadata => NodeType::FileContentMetadata,
            EdgeType::RootToAliasContentMapping => NodeType::AliasContentMapping,
            // Derived data Roots
            EdgeType::RootToBonsaiFsnodeMapping => NodeType::BonsaiFsnodeMapping,
            EdgeType::RootToFsnode => NodeType::Fsnode,
            // Bonsai
            EdgeType::BookmarkToBonsaiChangeset => NodeType::BonsaiChangeset,
            EdgeType::BookmarkToBonsaiHgMapping => NodeType::BonsaiHgMapping,
            EdgeType::BonsaiChangesetToFileContent => NodeType::FileContent,
            EdgeType::BonsaiChangesetToBonsaiParent => NodeType::BonsaiChangeset,
            EdgeType::BonsaiChangesetToBonsaiHgMapping => NodeType::BonsaiHgMapping,
            EdgeType::BonsaiChangesetToBonsaiPhaseMapping => NodeType::BonsaiPhaseMapping,
            EdgeType::BonsaiHgMappingToHgChangeset => NodeType::HgChangeset,
            EdgeType::PublishedBookmarksToBonsaiChangeset => NodeType::BonsaiChangeset,
            EdgeType::PublishedBookmarksToBonsaiHgMapping => NodeType::BonsaiHgMapping,
            EdgeType::BonsaiChangesetToBonsaiFsnodeMapping => NodeType::BonsaiFsnodeMapping,
            // Hg
            EdgeType::HgBonsaiMappingToBonsaiChangeset => NodeType::BonsaiChangeset,
            EdgeType::HgChangesetToHgParent => NodeType::HgChangeset,
            EdgeType::HgChangesetToHgManifest => NodeType::HgManifest,
            EdgeType::HgManifestToHgFileEnvelope => NodeType::HgFileEnvelope,
            EdgeType::HgManifestToHgFileNode => NodeType::HgFileNode,
            EdgeType::HgManifestToChildHgManifest => NodeType::HgManifest,
            EdgeType::HgFileEnvelopeToFileContent => NodeType::FileContent,
            EdgeType::HgLinkNodeToHgBonsaiMapping => NodeType::HgBonsaiMapping,
            EdgeType::HgLinkNodeToHgChangeset => NodeType::HgChangeset,
            EdgeType::HgFileNodeToHgParentFileNode => NodeType::HgFileNode,
            EdgeType::HgFileNodeToHgCopyfromFileNode => NodeType::HgFileNode,
            // Content
            EdgeType::FileContentToFileContentMetadata => NodeType::FileContentMetadata,
            EdgeType::FileContentMetadataToSha1Alias => NodeType::AliasContentMapping,
            EdgeType::FileContentMetadataToSha256Alias => NodeType::AliasContentMapping,
            EdgeType::FileContentMetadataToGitSha1Alias => NodeType::AliasContentMapping,
            EdgeType::AliasContentMappingToFileContent => NodeType::FileContent,
            // Derived data
            EdgeType::BonsaiToRootFsnode => NodeType::Fsnode,
            EdgeType::FsnodeToChildFsnode => NodeType::Fsnode,
        }
    }
}

impl fmt::Display for EdgeType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// File content gets a special two-state content so we can chose when to read the data
pub enum FileContentData {
    ContentStream(BoxStream<'static, Result<FileBytes, Error>>),
    Consumed(usize),
}

/// The data from the walk - this is the "full" form but not necessarily fully loaded.
/// e.g. file content streams are passed to you to read, they aren't pre-loaded to bytes.
pub enum NodeData {
    ErrorAsData(Node),
    // Bonsai
    Bookmark(ChangesetId),
    BonsaiChangeset(BonsaiChangeset),
    BonsaiHgMapping(Option<HgChangesetId>),
    BonsaiPhaseMapping(Option<Phase>),
    PublishedBookmarks,
    // Hg
    HgBonsaiMapping(Option<ChangesetId>),
    HgChangeset(HgBlobChangeset),
    HgManifest(BlobManifest),
    HgFileEnvelope(HgFileEnvelope),
    HgFileNode(Option<FilenodeInfo>),
    // Content
    FileContent(FileContentData),
    FileContentMetadata(Option<ContentMetadata>),
    AliasContentMapping(ContentId),
    // Derived data
    BonsaiFsnodeMapping(Option<FsnodeId>),
    Fsnode(Fsnode),
}

impl Node {
    pub fn get_type(&self) -> NodeType {
        match self {
            Node::Root => NodeType::Root,
            // Bonsai
            Node::Bookmark(_) => NodeType::Bookmark,
            Node::BonsaiChangeset(_) => NodeType::BonsaiChangeset,
            Node::BonsaiHgMapping(_) => NodeType::BonsaiHgMapping,
            Node::BonsaiPhaseMapping(_) => NodeType::BonsaiPhaseMapping,
            Node::PublishedBookmarks => NodeType::PublishedBookmarks,
            // Hg
            Node::HgBonsaiMapping(_) => NodeType::HgBonsaiMapping,
            Node::HgChangeset(_) => NodeType::HgChangeset,
            Node::HgManifest(_) => NodeType::HgManifest,
            Node::HgFileEnvelope(_) => NodeType::HgFileEnvelope,
            Node::HgFileNode(_) => NodeType::HgFileNode,
            // Content
            Node::FileContent(_) => NodeType::FileContent,
            Node::FileContentMetadata(_) => NodeType::FileContentMetadata,
            Node::AliasContentMapping(_) => NodeType::AliasContentMapping,
            // Derived data
            Node::BonsaiFsnodeMapping(_) => NodeType::BonsaiFsnodeMapping,
            Node::Fsnode(_) => NodeType::Fsnode,
        }
    }

    pub fn stats_key(&self) -> String {
        match self {
            Node::Root => "root".to_string(),
            // Bonsai
            Node::Bookmark(k) => k.to_string(),
            Node::BonsaiChangeset(k) => k.blobstore_key(),
            Node::BonsaiHgMapping(k) => k.blobstore_key(),
            Node::BonsaiPhaseMapping(k) => k.blobstore_key(),
            Node::PublishedBookmarks => "published_bookmarks".to_string(),
            // Hg
            Node::HgBonsaiMapping(k) => k.blobstore_key(),
            Node::HgChangeset(k) => k.blobstore_key(),
            Node::HgManifest((_, k)) => k.blobstore_key(),
            Node::HgFileEnvelope(k) => k.blobstore_key(),
            Node::HgFileNode((_, k)) => k.blobstore_key(),
            // Content
            Node::FileContent(k) => k.blobstore_key(),
            Node::FileContentMetadata(k) => k.blobstore_key(),
            Node::AliasContentMapping(k) => k.blobstore_key(),
            // Derived data
            Node::BonsaiFsnodeMapping(k) => k.blobstore_key(),
            Node::Fsnode((_, k)) => k.blobstore_key(),
        }
    }

    pub fn stats_path(&self) -> Option<&WrappedPath> {
        match self {
            Node::Root => None,
            // Bonsai
            Node::Bookmark(_) => None,
            Node::BonsaiChangeset(_) => None,
            Node::BonsaiHgMapping(_) => None,
            Node::BonsaiPhaseMapping(_) => None,
            Node::PublishedBookmarks => None,
            // Hg
            Node::HgBonsaiMapping(_) => None,
            Node::HgChangeset(_) => None,
            Node::HgManifest((p, _)) => Some(&p),
            Node::HgFileEnvelope(_) => None,
            Node::HgFileNode((p, _)) => Some(&p),
            // Content
            Node::FileContent(_) => None,
            Node::FileContentMetadata(_) => None,
            Node::AliasContentMapping(_) => None,
            // Derived data
            Node::BonsaiFsnodeMapping(_) => None,
            Node::Fsnode((p, _)) => Some(&p),
        }
    }

    /// None means not hash based
    pub fn sampling_fingerprint(&self) -> Option<u64> {
        match self {
            Node::Root => None,
            // Bonsai
            Node::Bookmark(_k) => None,
            Node::BonsaiChangeset(k) => Some(k.sampling_fingerprint()),
            Node::BonsaiHgMapping(k) => Some(k.sampling_fingerprint()),
            Node::BonsaiPhaseMapping(k) => Some(k.sampling_fingerprint()),
            Node::PublishedBookmarks => None,
            // Hg
            Node::HgBonsaiMapping(k) => Some(k.sampling_fingerprint()),
            Node::HgChangeset(k) => Some(k.sampling_fingerprint()),
            Node::HgManifest((_, k)) => Some(k.sampling_fingerprint()),
            Node::HgFileEnvelope(k) => Some(k.sampling_fingerprint()),
            Node::HgFileNode((_, k)) => Some(k.sampling_fingerprint()),
            // Content
            Node::FileContent(k) => Some(k.sampling_fingerprint()),
            Node::FileContentMetadata(k) => Some(k.sampling_fingerprint()),
            Node::AliasContentMapping(k) => Some(k.sampling_fingerprint()),
            // Derived data
            Node::BonsaiFsnodeMapping(k) => Some(k.sampling_fingerprint()),
            Node::Fsnode((_, k)) => Some(k.sampling_fingerprint()),
        }
    }
}
