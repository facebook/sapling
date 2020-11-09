/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ahash::RandomState;
use anyhow::Error;
use bookmarks::BookmarkName;
use changeset_info::ChangesetInfo;
use derived_data::BonsaiDerived;
use derived_data_filenodes::FilenodesOnlyPublic;
use filenodes::FilenodeInfo;
use filestore::Alias;
use fsnodes::RootFsnodeId;
use futures::stream::BoxStream;
use hash_memo::EagerHashMemoizer;
use internment::ArcIntern;
use mercurial_derived_data::MappedHgChangesetId;
use mercurial_types::{
    blobs::{BlobManifest, HgBlobChangeset},
    FileBytes, HgChangesetId, HgFileEnvelope, HgFileNodeId, HgManifestId,
};
use mononoke_types::{fsnode::Fsnode, FsnodeId};
use mononoke_types::{
    BonsaiChangeset, ChangesetId, ContentId, ContentMetadata, MPath, MPathHash, MononokeId,
};
use once_cell::sync::OnceCell;
use phases::Phase;
use std::{
    fmt,
    hash::{Hash, Hasher},
};

// Helper to save repetition for the type enums
macro_rules! define_type_enum {
     (enum $enum_name:ident {
         $($variant:ident),*,
     }) => {
         #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, strum_macros::EnumCount,
	     strum_macros::EnumIter, strum_macros::EnumString,  strum_macros::IntoStaticStr)]
         pub enum $enum_name {
             $($variant),*
         }
    }
}

#[doc(hidden)]
macro_rules! create_graph_impl {
    ($nodetypeenum:ident, $nodekeyenum:ident, $edgetypeenum:ident, {$($nodetype:tt)*} {$($nodekeys:tt)*})=> {
        define_type_enum!{
            enum $nodetypeenum {$($nodetype)*}
        }

        #[derive(Clone, Debug, PartialEq, Eq, Hash)]
        pub enum $nodekeyenum {$($nodekeys)*}
    };
    ($nodetypeenum:ident, $nodekeyenum:ident, $edgetypeenum:ident, {$($nodetype:tt)*} {$($nodekeys:tt)*} ($source:ident, $sourcekey:ty) $($rest:tt)*) => {
        create_graph_impl! {
            $nodetypeenum, $nodekeyenum, $edgetypeenum,
            {$($nodetype)* $source,}
            {$($nodekeys)* $source($sourcekey),}
            $($rest)*
        }
    };
}

macro_rules! root_edge_type {
    ($edgetypeenum:ident, Root) => {
        None
    };
    ($edgetypeenum:ident, $target:ident) => {
        Some(paste::item! {$edgetypeenum::[<RootTo $target>]})
    };
}

macro_rules! create_graph {
    ($nodetypeenum:ident, $nodekeyenum:ident, $edgetypeenum:ident, $(($source:ident, $sourcekey:tt)),* $(,)?) => {
        create_graph_impl! {
            $nodetypeenum, $nodekeyenum, $edgetypeenum,
            {}
            {}
            $(($source, $sourcekey))*
        }
        impl $nodetypeenum {
            pub fn root_edge_type(&self) -> Option<EdgeType> {
                match self {
                    $($nodetypeenum::$source => root_edge_type!($edgetypeenum, $source)),*
                }
            }
        }
        impl $nodekeyenum {
            pub fn get_type(&self) -> NodeType {
                match self {
                    $($nodekeyenum::$source(_) => $nodetypeenum::$source),*
                }
            }
        }
    }
}

create_graph!(
    NodeType,
    Node,
    EdgeType,
    (Root, ()),
    // Bonsai
    (Bookmark, BookmarkName),
    (BonsaiChangeset, ChangesetId),
    (BonsaiHgMapping, ChangesetId),
    (BonsaiPhaseMapping, ChangesetId),
    (PublishedBookmarks, ()),
    // Hg
    (HgBonsaiMapping, HgChangesetId),
    (HgChangeset, HgChangesetId),
    (HgManifest, (WrappedPath, HgManifestId)),
    (HgFileEnvelope, HgFileNodeId),
    (HgFileNode, (WrappedPath, HgFileNodeId)),
    // Content
    (FileContent, ContentId),
    (FileContentMetadata, ContentId),
    (AliasContentMapping, Alias),
    // Derived data
    (BonsaiFsnodeMapping, ChangesetId),
    (ChangesetInfo, ChangesetId),
    (Fsnode, (WrappedPath, FsnodeId)),
);

impl fmt::Display for NodeType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl NodeType {
    /// Derived data types are keyed by their statically defined NAME
    pub fn derived_data_name(&self) -> Option<&'static str> {
        match self {
            NodeType::Root => None,
            // Bonsai
            NodeType::Bookmark => None,
            NodeType::BonsaiChangeset => None,
            // from filenodes/lib.rs: If hg changeset is not generated, then root filenode can't possible be generated
            // therefore this is the same as MappedHgChangesetId + FilenodesOnlyPublic
            NodeType::BonsaiHgMapping => Some(FilenodesOnlyPublic::NAME),
            NodeType::BonsaiPhaseMapping => None,
            NodeType::PublishedBookmarks => None,
            // Hg
            NodeType::HgBonsaiMapping => Some(MappedHgChangesetId::NAME),
            NodeType::HgChangeset => Some(MappedHgChangesetId::NAME),
            NodeType::HgManifest => Some(MappedHgChangesetId::NAME),
            NodeType::HgFileEnvelope => Some(MappedHgChangesetId::NAME),
            NodeType::HgFileNode => Some(FilenodesOnlyPublic::NAME),
            // Content
            NodeType::FileContent => None,
            NodeType::FileContentMetadata => None,
            NodeType::AliasContentMapping => None,
            // Derived data
            NodeType::BonsaiFsnodeMapping => Some(RootFsnodeId::NAME),
            NodeType::ChangesetInfo => Some(ChangesetInfo::NAME),
            NodeType::Fsnode => Some(RootFsnodeId::NAME),
        }
    }
}

// Memoize the hash of the path as it is used frequently

#[derive(Debug)]
pub struct MPathHashMemo {
    mpath: MPath,
    memoized_hash: OnceCell<MPathHash>,
}

impl MPathHashMemo {
    fn new(mpath: MPath) -> Self {
        Self {
            mpath,
            memoized_hash: OnceCell::new(),
        }
    }

    pub fn get_path_hash(&self) -> &MPathHash {
        self.memoized_hash
            .get_or_init(|| self.mpath.get_path_hash())
    }

    pub fn mpath(&self) -> &MPath {
        &self.mpath
    }
}

impl PartialEq for MPathHashMemo {
    fn eq(&self, other: &Self) -> bool {
        self.mpath == other.mpath
    }
}

impl Eq for MPathHashMemo {}

impl Hash for MPathHashMemo {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.mpath.hash(state);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum WrappedPath {
    Root,
    NonRoot(ArcIntern<EagerHashMemoizer<MPathHashMemo>>),
}

impl WrappedPath {
    pub fn as_ref(&self) -> Option<&MPath> {
        match self {
            WrappedPath::Root => None,
            WrappedPath::NonRoot(path) => Some(path.mpath()),
        }
    }

    pub fn get_path_hash(&self) -> Option<&MPathHash> {
        match self {
            WrappedPath::Root => None,
            WrappedPath::NonRoot(path) => Some(path.get_path_hash()),
        }
    }

    pub fn sampling_fingerprint(&self) -> Option<u64> {
        self.get_path_hash().map(|h| h.sampling_fingerprint())
    }
}

impl fmt::Display for WrappedPath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            WrappedPath::Root => write!(f, ""),
            WrappedPath::NonRoot(path) => write!(f, "{}", path.mpath()),
        }
    }
}

static PATH_HASHER_FACTORY: OnceCell<RandomState> = OnceCell::new();

impl From<Option<MPath>> for WrappedPath {
    fn from(mpath: Option<MPath>) -> Self {
        let hasher_fac = PATH_HASHER_FACTORY.get_or_init(|| RandomState::default());
        match mpath {
            Some(mpath) => WrappedPath::NonRoot(ArcIntern::new(EagerHashMemoizer::new(
                MPathHashMemo::new(mpath),
                hasher_fac,
            ))),
            None => WrappedPath::Root,
        }
    }
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
    RootToChangesetInfo,
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
    HgFileNodeToLinkedHgBonsaiMapping,
    HgFileNodeToLinkedHgChangeset,
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
    BonsaiChangesetToChangesetInfo,
    ChangesetInfoToChangesetInfoParent,
    FsnodeToChildFsnode,
    FsnodeToFileContent,
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
            EdgeType::RootToChangesetInfo => None,
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
            EdgeType::HgFileNodeToLinkedHgBonsaiMapping => Some(NodeType::HgFileNode),
            EdgeType::HgFileNodeToLinkedHgChangeset => Some(NodeType::HgFileNode),
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
            EdgeType::BonsaiChangesetToChangesetInfo => Some(NodeType::BonsaiChangeset),
            EdgeType::ChangesetInfoToChangesetInfoParent => Some(NodeType::ChangesetInfo),
            EdgeType::FsnodeToChildFsnode => Some(NodeType::Fsnode),
            EdgeType::FsnodeToFileContent => Some(NodeType::Fsnode),
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
            EdgeType::RootToChangesetInfo => NodeType::ChangesetInfo,
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
            EdgeType::HgFileNodeToLinkedHgBonsaiMapping => NodeType::HgBonsaiMapping,
            EdgeType::HgFileNodeToLinkedHgChangeset => NodeType::HgChangeset,
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
            EdgeType::BonsaiChangesetToChangesetInfo => NodeType::ChangesetInfo,
            EdgeType::ChangesetInfoToChangesetInfoParent => NodeType::ChangesetInfo,
            EdgeType::FsnodeToChildFsnode => NodeType::Fsnode,
            EdgeType::FsnodeToFileContent => NodeType::FileContent,
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
    NotRequired,
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
    ChangesetInfo(Option<ChangesetInfo>),
    Fsnode(Fsnode),
}

impl Node {
    pub fn stats_key(&self) -> String {
        match self {
            Node::Root(_) => "root".to_string(),
            // Bonsai
            Node::Bookmark(k) => k.to_string(),
            Node::BonsaiChangeset(k) => k.blobstore_key(),
            Node::BonsaiHgMapping(k) => k.blobstore_key(),
            Node::BonsaiPhaseMapping(k) => k.blobstore_key(),
            Node::PublishedBookmarks(_) => "published_bookmarks".to_string(),
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
            Node::ChangesetInfo(k) => k.blobstore_key(),
            Node::Fsnode((_, k)) => k.blobstore_key(),
        }
    }

    pub fn stats_path(&self) -> Option<&WrappedPath> {
        match self {
            Node::Root(_) => None,
            // Bonsai
            Node::Bookmark(_) => None,
            Node::BonsaiChangeset(_) => None,
            Node::BonsaiHgMapping(_) => None,
            Node::BonsaiPhaseMapping(_) => None,
            Node::PublishedBookmarks(_) => None,
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
            Node::ChangesetInfo(_) => None,
            Node::Fsnode((p, _)) => Some(&p),
        }
    }

    /// None means not hash based
    pub fn sampling_fingerprint(&self) -> Option<u64> {
        match self {
            Node::Root(_) => None,
            // Bonsai
            Node::Bookmark(_k) => None,
            Node::BonsaiChangeset(k) => Some(k.sampling_fingerprint()),
            Node::BonsaiHgMapping(k) => Some(k.sampling_fingerprint()),
            Node::BonsaiPhaseMapping(k) => Some(k.sampling_fingerprint()),
            Node::PublishedBookmarks(_) => None,
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
            Node::ChangesetInfo(k) => Some(k.sampling_fingerprint()),
            Node::Fsnode((_, k)) => Some(k.sampling_fingerprint()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blobrepo_factory::init_all_derived_data;
    use std::{collections::HashSet, mem::size_of};
    use strum::{EnumCount, IntoEnumIterator};

    #[test]
    fn test_node_size() {
        // Node size is important as we have lots of them, add a test to check for accidental changes
        assert_eq!(56, size_of::<Node>());
    }

    #[test]
    fn test_node_type_max_ordinal() {
        // Check the macros worked consistently
        for t in NodeType::iter() {
            assert!((t as usize) < NodeType::COUNT)
        }
    }

    #[test]
    fn test_all_derived_data_types_supported() {
        // All types blobrepo can support
        let a = init_all_derived_data().derived_data_types;

        // supported in graph
        let mut s = HashSet::new();
        for t in NodeType::iter() {
            if let Some(d) = t.derived_data_name() {
                assert!(
                    a.contains(d),
                    "graph derived data type {} for {} is not known by blobrepo::init_all_derived_data()",
                    d,
                    t
                );
                s.insert(d);
            }
        }

        // TODO(ahornby) implement all derived types in walker so can enable this check
        // for t in &a {
        //     assert!(
        //         s.contains(t.as_str()),
        //         "blobrepo derived data type {} is not supported by walker graph",
        //         t
        //     );
        // }
    }
}
