/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ahash::RandomState;
use anyhow::Error;
use bitflags::bitflags;
use blame::BlameRoot;
use bookmarks::BookmarkName;
use changeset_info::ChangesetInfo;
use deleted_files_manifest::RootDeletedManifestId;
use derived_data::BonsaiDerived;
use derived_data_filenodes::FilenodesOnlyPublic;
use fastlog::{unode_entry_to_fastlog_batch_key, RootFastlog};
use filenodes::FilenodeInfo;
use filestore::Alias;
use fsnodes::RootFsnodeId;
use futures::stream::BoxStream;
use hash_memo::EagerHashMemoizer;
use internment::ArcIntern;
use manifest::Entry;
use mercurial_derived_data::MappedHgChangesetId;
use mercurial_types::{
    blobs::{HgBlobChangeset, HgBlobManifest},
    FileBytes, HgChangesetId, HgFileEnvelope, HgFileNodeId, HgManifestId,
};
use mononoke_types::{
    blame::Blame,
    deleted_files_manifest::DeletedManifest,
    fastlog_batch::FastlogBatch,
    fsnode::Fsnode,
    skeleton_manifest::SkeletonManifest,
    unode::{FileUnode, ManifestUnode},
    BlameId, BonsaiChangeset, ChangesetId, ContentId, ContentMetadata, DeletedManifestId,
    FastlogBatchId, FileUnodeId, FsnodeId, MPath, MPathHash, ManifestUnodeId, MononokeId,
    SkeletonManifestId,
};
use once_cell::sync::OnceCell;
use phases::Phase;
use skeleton_manifest::RootSkeletonManifestId;
use std::{
    fmt,
    hash::{Hash, Hasher},
    str::FromStr,
};
use unodes::RootUnodeManifestId;

// Helper to save repetition for the type enums
macro_rules! define_type_enum {
     (enum $enum_name:ident {
         $($variant:ident),* $(,)?
     }) => {
         #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, strum_macros::EnumCount,
	     strum_macros::EnumIter, strum_macros::EnumString,  strum_macros::IntoStaticStr)]
         pub enum $enum_name {
             $($variant),*
         }
    }
}

macro_rules! incoming_type {
    ($nodetypeenum:ident, $edgetypeenum:tt, Root) => {
        None
    };
    ($nodetypeenum:ident, $edgetypeenum:tt, $sourcetype:tt) => {
        Some($nodetypeenum::$sourcetype)
    };
}

macro_rules! outgoing_type {
    // Edge isn't named exactly same as the target node type
    ($nodetypeenum:ident, $edgetypeenum:tt, $targetlabel:ident($targettype:ident)) => {
        $nodetypeenum::$targettype
    };
    // In most cases its the same
    ($nodetypeenum:ident, $edgetypeenum:tt, $targetlabel:ident) => {
        $nodetypeenum::$targetlabel
    };
}

#[doc(hidden)]
macro_rules! create_graph_impl {
    ($nodetypeenum:ident, $nodekeyenum:ident, $edgetypeenum:ident,
        {$($nodetype:tt)*} {$($nodekeys:tt)*} {$(($edgetype:tt, $edgesourcetype:tt, $($edgetargetdef:tt)+)),+ $(,)?} ) => {
        define_type_enum!{
            enum $nodetypeenum {$($nodetype)*}
        }

        #[derive(Clone, Debug, PartialEq, Eq, Hash)]
        pub enum $nodekeyenum {$($nodekeys)*}

        define_type_enum!{
            enum $edgetypeenum {$($edgetype),*}
        }

        impl $edgetypeenum {
            pub fn incoming_type(&self) -> Option<$nodetypeenum> {
                match self {
                    $($edgetypeenum::$edgetype => incoming_type!($nodetypeenum, $edgetypeenum, $edgesourcetype)),*
                }
            }
        }

        impl $edgetypeenum {
            pub fn outgoing_type(&self) -> $nodetypeenum {
                match self {
                    $($edgetypeenum::$edgetype => outgoing_type!($nodetypeenum, $edgetypeenum, $($edgetargetdef)+)),*
                }
            }
        }
    };
    ($nodetypeenum:ident, $nodekeyenum:ident, $edgetypeenum:ident,
        {$($nodetype:tt)*} {$($nodekeys:tt)*} {$(($edgetype:tt, $edgesourcetype:tt, $($edgetargetdef:tt)+)),* $(,)?}
            ($source:ident, $sourcekey:ty, [$($target:ident$(($targettype:ident))?),*]) $($rest:tt)*) => {
        paste::item!{
            create_graph_impl! {
                $nodetypeenum, $nodekeyenum, $edgetypeenum,
                {$($nodetype)* $source,}
                {$($nodekeys)* $source($sourcekey),}
                {
                    $(($edgetype, $edgesourcetype, $($edgetargetdef)+),)*
                    $(([<$source To $target>], $source, $target$(($targettype))? ),)*
                }
                $($rest)*
            }
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
    ($nodetypeenum:ident, $nodekeyenum:ident, $edgetypeenum:ident,
        $(($source:ident, $sourcekey:ty, [$($target:ident$(($targettype:ident))?),*])),* $(,)?) => {
        create_graph_impl! {
            $nodetypeenum, $nodekeyenum, $edgetypeenum,
            {}
            {}
            {}
            $(($source, $sourcekey, [$($target$(($targettype))?),*]))*
        }

        impl $nodetypeenum {
            pub fn root_edge_type(&self) -> Option<$edgetypeenum> {
                match self {
                    $($nodetypeenum::$source => root_edge_type!($edgetypeenum, $source)),*
                }
            }
            pub fn parse_node(&self, s: &str) -> Result<$nodekeyenum, Error> {
                match self {
                    $($nodetypeenum::$source => Ok($nodekeyenum::$source(<$sourcekey>::from_str(s)?))),*
                }
            }
        }
        impl $nodekeyenum {
            pub fn get_type(&self) -> $nodetypeenum {
                match self {
                    $($nodekeyenum::$source(_) => $nodetypeenum::$source),*
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnitKey();

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PathKey<T: fmt::Debug + Clone + PartialEq + Eq + Hash> {
    pub id: T,
    pub path: WrappedPath,
}
impl<T: fmt::Debug + Clone + PartialEq + Eq + Hash> PathKey<T> {
    pub fn new(id: T, path: WrappedPath) -> Self {
        Self { id, path }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AliasKey(pub Alias);

bitflags! {
    /// Some derived data needs unodes as precondition, flags represent what is available in a compact way
    #[derive(Default)]
    pub struct UnodeFlags: u8 {
        const NONE = 0b00000000;
        const BLAME = 0b00000001;
        const FASTLOG = 0b00000010;
    }
}

/// Not all unodes should attempt to traverse blame or fastlog
/// e.g. a unode for non-public commit is not expected to have it
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UnodeKey<T> {
    pub inner: T,
    pub flags: UnodeFlags,
}

impl<T: MononokeId> UnodeKey<T> {
    fn blobstore_key(&self) -> String {
        self.inner.blobstore_key()
    }

    fn sampling_fingerprint(&self) -> u64 {
        self.inner.sampling_fingerprint()
    }
}

pub type UnodeManifestEntry = Entry<ManifestUnodeId, FileUnodeId>;

/// newtype so we can implement blobstore_key()
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FastlogKey<T> {
    pub inner: T,
}

impl<T: MononokeId> FastlogKey<T> {
    fn sampling_fingerprint(&self) -> u64 {
        self.inner.sampling_fingerprint()
    }

    pub fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl FastlogKey<FileUnodeId> {
    fn blobstore_key(&self) -> String {
        unode_entry_to_fastlog_batch_key(&UnodeManifestEntry::Leaf(self.inner))
    }
}

impl FastlogKey<ManifestUnodeId> {
    fn blobstore_key(&self) -> String {
        unode_entry_to_fastlog_batch_key(&UnodeManifestEntry::Tree(self.inner))
    }
}

create_graph!(
    NodeType,
    Node,
    EdgeType,
    (
        Root,
        UnitKey,
        [
            // Bonsai
            Bookmark,
            Changeset,
            BonsaiHgMapping,
            PhaseMapping,
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
            // Derived
            Blame,
            ChangesetInfo,
            ChangesetInfoMapping,
            DeletedManifest,
            DeletedManifestMapping,
            FastlogBatch,
            FastlogDir,
            FastlogFile,
            Fsnode,
            FsnodeMapping,
            SkeletonManifest,
            SkeletonManifestMapping,
            UnodeFile,
            UnodeManifest,
            UnodeMapping
        ]
    ),
    // Bonsai
    (Bookmark, BookmarkName, [Changeset, BonsaiHgMapping]),
    (
        Changeset,
        ChangesetId,
        [
            FileContent,
            BonsaiParent(Changeset),
            BonsaiHgMapping,
            PhaseMapping,
            ChangesetInfo,
            ChangesetInfoMapping,
            DeletedManifestMapping,
            FsnodeMapping,
            SkeletonManifestMapping,
            UnodeMapping
        ]
    ),
    (BonsaiHgMapping, ChangesetId, [HgChangeset]),
    (PhaseMapping, ChangesetId, []),
    (
        PublishedBookmarks,
        UnitKey,
        [Changeset, BonsaiHgMapping]
    ),
    // Hg
    (HgBonsaiMapping, HgChangesetId, [Changeset]),
    (
        HgChangeset,
        HgChangesetId,
        [HgParent(HgChangeset), HgManifest]
    ),
    (
        HgManifest,
        PathKey<HgManifestId>,
        [HgFileEnvelope, HgFileNode, ChildHgManifest(HgManifest)]
    ),
    (HgFileEnvelope, HgFileNodeId, [FileContent]),
    (
        HgFileNode,
        PathKey<HgFileNodeId>,
        [
            LinkedHgBonsaiMapping(HgBonsaiMapping),
            LinkedHgChangeset(HgChangeset),
            HgParentFileNode(HgFileNode),
            HgCopyfromFileNode(HgFileNode)
        ]
    ),
    // Content
    (FileContent, ContentId, [FileContentMetadata]),
    (
        FileContentMetadata,
        ContentId,
        [
            Sha1Alias(AliasContentMapping),
            Sha256Alias(AliasContentMapping),
            GitSha1Alias(AliasContentMapping)
        ]
    ),
    (AliasContentMapping, AliasKey, [FileContent]),
    // Derived data
    (
        Blame,
        BlameId,
        [Changeset]
    ),
    (
        ChangesetInfo,
        ChangesetId,
        [ChangesetInfoParent(ChangesetInfo)]
    ),
    (
        ChangesetInfoMapping,
        ChangesetId,
        [ChangesetInfo]
    ),
    (
        DeletedManifest,
        DeletedManifestId,
        [DeletedManifestChild(DeletedManifest), LinkedChangeset(Changeset)]
    ),
    (DeletedManifestMapping, ChangesetId, [RootDeletedManifest(DeletedManifest)]),
    (
        Fsnode,
        FsnodeId,
        [ChildFsnode(Fsnode), FileContent]
    ),
    (
        FastlogBatch,
        FastlogBatchId,
        [Changeset, PreviousBatch(FastlogBatch)]
    ),
    (
        FastlogDir,
        FastlogKey<ManifestUnodeId>,
        [Changeset, PreviousBatch(FastlogBatch)]
    ),
    (
        FastlogFile,
        FastlogKey<FileUnodeId>,
        [Changeset, PreviousBatch(FastlogBatch)]
    ),
    (FsnodeMapping, ChangesetId, [RootFsnode(Fsnode)]),
    (
        SkeletonManifest,
        SkeletonManifestId,
        [SkeletonManifestChild(SkeletonManifest)]
    ),
    (SkeletonManifestMapping, ChangesetId, [RootSkeletonManifest(SkeletonManifest)]),
    (
        UnodeFile,
        UnodeKey<FileUnodeId>,
        [Blame, FastlogFile, FileContent, LinkedChangeset(Changeset), UnodeFileParent(UnodeFile)]
    ),
    (
        UnodeManifest,
        UnodeKey<ManifestUnodeId>,
        [FastlogDir, UnodeFileChild(UnodeFile), UnodeManifestChild(UnodeManifest), UnodeManifestParent(UnodeManifest), LinkedChangeset(Changeset)]
    ),
    (UnodeMapping, ChangesetId, [RootUnodeManifest(UnodeManifest)]),
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
            NodeType::Changeset => None,
            // from filenodes/lib.rs: If hg changeset is not generated, then root filenode can't possible be generated
            // therefore this is the same as MappedHgChangesetId + FilenodesOnlyPublic
            NodeType::BonsaiHgMapping => Some(FilenodesOnlyPublic::NAME),
            NodeType::PhaseMapping => None,
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
            NodeType::Blame => Some(BlameRoot::NAME),
            NodeType::ChangesetInfo => Some(ChangesetInfo::NAME),
            NodeType::ChangesetInfoMapping => Some(ChangesetInfo::NAME),
            NodeType::DeletedManifest => Some(RootDeletedManifestId::NAME),
            NodeType::DeletedManifestMapping => Some(RootDeletedManifestId::NAME),
            NodeType::FastlogBatch => Some(RootFastlog::NAME),
            NodeType::FastlogDir => Some(RootFastlog::NAME),
            NodeType::FastlogFile => Some(RootFastlog::NAME),
            NodeType::Fsnode => Some(RootFsnodeId::NAME),
            NodeType::FsnodeMapping => Some(RootFsnodeId::NAME),
            NodeType::SkeletonManifest => Some(RootSkeletonManifestId::NAME),
            NodeType::SkeletonManifestMapping => Some(RootSkeletonManifestId::NAME),
            NodeType::UnodeFile => Some(RootUnodeManifestId::NAME),
            NodeType::UnodeManifest => Some(RootUnodeManifestId::NAME),
            NodeType::UnodeMapping => Some(RootUnodeManifestId::NAME),
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

define_type_enum! {
    enum AliasType {
        GitSha1,
        Sha1,
        Sha256,
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

impl fmt::Debug for FileContentData {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            FileContentData::ContentStream(_s) => write!(f, "FileContentData::ContentStream(_)"),
            FileContentData::Consumed(s) => write!(f, "FileContentData::Consumed({})", s),
        }
    }
}

/// The data from the walk - this is the "full" form but not necessarily fully loaded.
/// e.g. file content streams are passed to you to read, they aren't pre-loaded to bytes.
#[derive(Debug)]
pub enum NodeData {
    ErrorAsData(Node),
    NotRequired,
    // Bonsai
    Bookmark(ChangesetId),
    Changeset(BonsaiChangeset),
    BonsaiHgMapping(Option<HgChangesetId>),
    PhaseMapping(Option<Phase>),
    PublishedBookmarks,
    // Hg
    HgBonsaiMapping(Option<ChangesetId>),
    HgChangeset(HgBlobChangeset),
    HgManifest(HgBlobManifest),
    HgFileEnvelope(HgFileEnvelope),
    HgFileNode(Option<FilenodeInfo>),
    // Content
    FileContent(FileContentData),
    FileContentMetadata(Option<ContentMetadata>),
    AliasContentMapping(ContentId),
    // Derived data
    Blame(Option<Blame>),
    ChangesetInfo(Option<ChangesetInfo>),
    ChangesetInfoMapping(Option<ChangesetId>),
    DeletedManifest(Option<DeletedManifest>),
    DeletedManifestMapping(Option<DeletedManifestId>),
    FastlogBatch(Option<FastlogBatch>),
    FastlogDir(Option<FastlogBatch>),
    FastlogFile(Option<FastlogBatch>),
    Fsnode(Fsnode),
    FsnodeMapping(Option<FsnodeId>),
    SkeletonManifest(Option<SkeletonManifest>),
    SkeletonManifestMapping(Option<SkeletonManifestId>),
    UnodeFile(FileUnode),
    UnodeManifest(ManifestUnode),
    UnodeMapping(Option<ManifestUnodeId>),
}

impl Node {
    pub fn stats_key(&self) -> String {
        match self {
            Node::Root(_) => "root".to_string(),
            // Bonsai
            Node::Bookmark(k) => k.to_string(),
            Node::Changeset(k) => k.blobstore_key(),
            Node::BonsaiHgMapping(k) => k.blobstore_key(),
            Node::PhaseMapping(k) => k.blobstore_key(),
            Node::PublishedBookmarks(_) => "published_bookmarks".to_string(),
            // Hg
            Node::HgBonsaiMapping(k) => k.blobstore_key(),
            Node::HgChangeset(k) => k.blobstore_key(),
            Node::HgManifest(PathKey { id, path: _ }) => id.blobstore_key(),
            Node::HgFileEnvelope(k) => k.blobstore_key(),
            Node::HgFileNode(PathKey { id, path: _ }) => id.blobstore_key(),
            // Content
            Node::FileContent(k) => k.blobstore_key(),
            Node::FileContentMetadata(k) => k.blobstore_key(),
            Node::AliasContentMapping(k) => k.0.blobstore_key(),
            // Derived data
            Node::Blame(k) => k.blobstore_key(),
            Node::ChangesetInfo(k) => k.blobstore_key(),
            Node::ChangesetInfoMapping(k) => k.blobstore_key(),
            Node::DeletedManifest(k) => k.blobstore_key(),
            Node::DeletedManifestMapping(k) => k.blobstore_key(),
            Node::FastlogBatch(k) => k.blobstore_key(),
            Node::FastlogDir(k) => k.blobstore_key(),
            Node::FastlogFile(k) => k.blobstore_key(),
            Node::Fsnode(k) => k.blobstore_key(),
            Node::FsnodeMapping(k) => k.blobstore_key(),
            Node::SkeletonManifest(k) => k.blobstore_key(),
            Node::SkeletonManifestMapping(k) => k.blobstore_key(),
            Node::UnodeFile(k) => k.blobstore_key(),
            Node::UnodeManifest(k) => k.blobstore_key(),
            Node::UnodeMapping(k) => k.blobstore_key(),
        }
    }

    pub fn stats_path(&self) -> Option<&WrappedPath> {
        match self {
            Node::Root(_) => None,
            // Bonsai
            Node::Bookmark(_) => None,
            Node::Changeset(_) => None,
            Node::BonsaiHgMapping(_) => None,
            Node::PhaseMapping(_) => None,
            Node::PublishedBookmarks(_) => None,
            // Hg
            Node::HgBonsaiMapping(_) => None,
            Node::HgChangeset(_) => None,
            Node::HgManifest(PathKey { id: _, path }) => Some(&path),
            Node::HgFileEnvelope(_) => None,
            Node::HgFileNode(PathKey { id: _, path }) => Some(&path),
            // Content
            Node::FileContent(_) => None,
            Node::FileContentMetadata(_) => None,
            Node::AliasContentMapping(_) => None,
            // Derived data
            Node::Blame(_) => None,
            Node::ChangesetInfo(_) => None,
            Node::ChangesetInfoMapping(_) => None,
            Node::DeletedManifest(_) => None,
            Node::DeletedManifestMapping(_) => None,
            Node::FastlogBatch(_) => None,
            Node::FastlogDir(_) => None,
            Node::FastlogFile(_) => None,
            Node::Fsnode(_) => None,
            Node::FsnodeMapping(_) => None,
            Node::SkeletonManifest(_) => None,
            Node::SkeletonManifestMapping(_) => None,
            Node::UnodeFile(_) => None,
            Node::UnodeManifest(_) => None,
            Node::UnodeMapping(_) => None,
        }
    }

    /// None means not hash based
    pub fn sampling_fingerprint(&self) -> Option<u64> {
        match self {
            Node::Root(_) => None,
            // Bonsai
            Node::Bookmark(_k) => None,
            Node::Changeset(k) => Some(k.sampling_fingerprint()),
            Node::BonsaiHgMapping(k) => Some(k.sampling_fingerprint()),
            Node::PhaseMapping(k) => Some(k.sampling_fingerprint()),
            Node::PublishedBookmarks(_) => None,
            // Hg
            Node::HgBonsaiMapping(k) => Some(k.sampling_fingerprint()),
            Node::HgChangeset(k) => Some(k.sampling_fingerprint()),
            Node::HgManifest(PathKey { id, path: _ }) => Some(id.sampling_fingerprint()),
            Node::HgFileEnvelope(k) => Some(k.sampling_fingerprint()),
            Node::HgFileNode(PathKey { id, path: _ }) => Some(id.sampling_fingerprint()),
            // Content
            Node::FileContent(k) => Some(k.sampling_fingerprint()),
            Node::FileContentMetadata(k) => Some(k.sampling_fingerprint()),
            Node::AliasContentMapping(k) => Some(k.0.sampling_fingerprint()),
            // Derived data
            Node::Blame(k) => Some(k.sampling_fingerprint()),
            Node::ChangesetInfo(k) => Some(k.sampling_fingerprint()),
            Node::ChangesetInfoMapping(k) => Some(k.sampling_fingerprint()),
            Node::DeletedManifest(k) => Some(k.sampling_fingerprint()),
            Node::DeletedManifestMapping(k) => Some(k.sampling_fingerprint()),
            Node::FastlogBatch(k) => Some(k.sampling_fingerprint()),
            Node::FastlogDir(k) => Some(k.sampling_fingerprint()),
            Node::FastlogFile(k) => Some(k.sampling_fingerprint()),
            Node::Fsnode(k) => Some(k.sampling_fingerprint()),
            Node::FsnodeMapping(k) => Some(k.sampling_fingerprint()),
            Node::SkeletonManifest(k) => Some(k.sampling_fingerprint()),
            Node::SkeletonManifestMapping(k) => Some(k.sampling_fingerprint()),
            Node::UnodeFile(k) => Some(k.sampling_fingerprint()),
            Node::UnodeManifest(k) => Some(k.sampling_fingerprint()),
            Node::UnodeMapping(k) => Some(k.sampling_fingerprint()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blobrepo_factory::init_all_derived_data;
    use std::{collections::HashSet, iter::FromIterator, mem::size_of};
    use strum::{EnumCount, IntoEnumIterator};

    #[test]
    fn test_node_size() {
        // Node size is important as we have lots of them, add a test to check for accidental changes
        assert_eq!(48, size_of::<Node>());
    }

    #[test]
    fn test_node_type_max_ordinal() {
        // Check the macros worked consistently
        for t in NodeType::iter() {
            assert!((t as usize) < NodeType::COUNT)
        }
    }

    #[test]
    fn test_small_graphs() -> Result<(), Error> {
        create_graph!(
            Test1NodeType,
            Test1Node,
            Test1EdgeType,
            (Root, UnitKey, [Foo]),
            (Foo, u32, []),
        );
        assert_eq!(Test1NodeType::Root, Test1Node::Root(UnitKey()).get_type());
        assert_eq!(Test1NodeType::Foo, Test1Node::Foo(42).get_type());
        assert_eq!(Test1EdgeType::RootToFoo.incoming_type(), None);
        assert_eq!(Test1EdgeType::RootToFoo.outgoing_type(), Test1NodeType::Foo);
        assert_eq!(Test1NodeType::Foo.parse_node("123")?, Test1Node::Foo(123));

        // Make sure type names don't clash
        create_graph!(
            Test2NodeType,
            Test2Node,
            Test2EdgeType,
            (Root, UnitKey, [Foo, Bar]),
            (Foo, u32, [Bar]),
            (Bar, u32, []),
        );
        assert_eq!(Test2NodeType::Root, Test2Node::Root(UnitKey()).get_type());
        assert_eq!(Test2NodeType::Foo, Test2Node::Foo(42).get_type());
        assert_eq!(Test2NodeType::Bar, Test2Node::Bar(42).get_type());
        assert_eq!(Test2EdgeType::RootToFoo.incoming_type(), None);
        assert_eq!(Test2EdgeType::RootToFoo.outgoing_type(), Test2NodeType::Foo);
        assert_eq!(Test2EdgeType::RootToBar.incoming_type(), None);
        assert_eq!(Test2EdgeType::RootToBar.outgoing_type(), Test2NodeType::Bar);
        assert_eq!(
            Test2EdgeType::FooToBar.incoming_type(),
            Some(Test2NodeType::Foo)
        );
        assert_eq!(Test2EdgeType::FooToBar.outgoing_type(), Test2NodeType::Bar);
        assert_eq!(Test2NodeType::Bar.parse_node("123")?, Test2Node::Bar(123));
        Ok(())
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

        // If you are adding a new derived data type, please add it to the walker graph rather than to this
        // list, otherwise it won't get scrubbed and thus you would be unaware of different representation
        // in different stores
        let grandfathered: HashSet<&'static str> =
            HashSet::from_iter(vec!["git_trees"].into_iter());
        let mut missing = HashSet::new();
        for t in &a {
            if s.contains(t.as_str()) {
                assert!(
                    !grandfathered.contains(t.as_str()),
                    "You've added support for {}, please remove it from the grandfathered missing set",
                    t
                );
            } else if !grandfathered.contains(t.as_str()) {
                missing.insert(t);
            }
        }
        assert!(
            missing.is_empty(),
            "blobrepo derived data types {:?} not supported by walker graph",
            missing,
        );
    }
}
