/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::hash::Hash;
use std::hash::Hasher;
use std::str::FromStr;
use std::sync::OnceLock;

use ahash::RandomState;
use anyhow::Error;
use anyhow::format_err;
use bitflags::bitflags;
use blame::RootBlameV2;
use blobstore_factory::SqlTierInfo;
use bookmarks::BookmarkKey;
use changeset_info::ChangesetInfo;
use context::CoreContext;
use deleted_manifest::RootDeletedManifestV2Id;
use derived_data_manager::BonsaiDerivable as NewBonsaiDerivable;
use fastlog::RootFastlog;
use fastlog::unode_entry_to_fastlog_batch_key;
use filenodes::FilenodeInfo;
use filenodes_derivation::FilenodesOnlyPublic;
use filestore::Alias;
use fsnodes::RootFsnodeId;
use futures::FutureExt;
use futures::StreamExt;
use futures::future::BoxFuture;
use futures::stream;
use futures::stream::BoxStream;
use hash_memo::EagerHashMemoizer;
use internment::ArcIntern;
use manifest::Entry;
use mercurial_derivation::MappedHgChangesetId;
use mercurial_types::FileBytes;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileEnvelope;
use mercurial_types::HgFileEnvelopeMut;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mercurial_types::HgParents;
use mercurial_types::blobs::HgBlobChangeset;
use mercurial_types::blobs::HgBlobManifest;
use mercurial_types::calculate_hg_node_id_stream;
use mononoke_types::BlameV2Id;
use mononoke_types::BlobstoreKey;
use mononoke_types::BlobstoreValue;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::ContentMetadataV2;
use mononoke_types::DeletedManifestV2Id;
use mononoke_types::DerivableType;
use mononoke_types::FastlogBatchId;
use mononoke_types::FileUnodeId;
use mononoke_types::FsnodeId;
use mononoke_types::MPathHash;
use mononoke_types::ManifestUnodeId;
use mononoke_types::MononokeId;
use mononoke_types::NonRootMPath;
use mononoke_types::RepoPath;
use mononoke_types::SkeletonManifestId;
use mononoke_types::blame_v2::BlameV2;
use mononoke_types::deleted_manifest_v2::DeletedManifestV2;
use mononoke_types::fastlog_batch::FastlogBatch;
use mononoke_types::fsnode::Fsnode;
use mononoke_types::path::MPath;
use mononoke_types::skeleton_manifest::SkeletonManifest;
use mononoke_types::unode::FileUnode;
use mononoke_types::unode::ManifestUnode;
use newfilenodes::PathHash;
use phases::Phase;
use repo_blobstore::RepoBlobstoreRef;
use skeleton_manifest::RootSkeletonManifestId;
use thiserror::Error;
use unodes::RootUnodeManifestId;

use crate::detail::repo::Repo;
use crate::detail::walk::OutgoingEdge;

#[derive(Error, Debug)]
pub enum HashValidationError {
    #[error("Error while computing hash validation")]
    Error(#[from] Error),
    #[error("failed to validate filenode hash: expected {expected_hash} actual {actual_hash}")]
    HashMismatch {
        actual_hash: String,
        expected_hash: String,
    },
    #[error("hash validation for {0} is not supported")]
    NotSupported(String),
}

// Helper to save repetition for the type enums
macro_rules! define_type_enum {
     (enum $enum_name:ident {
         $($variant:ident),* $(,)?
     }) => {
         #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, strum::AsRefStr,
         strum::EnumCount, strum::EnumIter, strum::EnumString,
         strum::VariantNames, strum::IntoStaticStr)]
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

/// Used for both Bonsai and HgChangesets to track if filenode data is present
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChangesetKey<T> {
    pub inner: T,
    pub filenode_known_derived: bool,
}

impl ChangesetKey<ChangesetId> {
    fn blobstore_key(&self) -> String {
        self.inner.blobstore_key()
    }

    fn sampling_fingerprint(&self) -> u64 {
        self.inner.sampling_fingerprint()
    }
}

impl ChangesetKey<HgChangesetId> {
    fn blobstore_key(&self) -> String {
        self.inner.blobstore_key()
    }

    fn sampling_fingerprint(&self) -> u64 {
        self.inner.sampling_fingerprint()
    }
}

bitflags! {
    /// Some derived data needs unodes as precondition, flags represent what is available in a compact way
    #[derive(Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Clone, Copy)]
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

impl<T: MononokeId> UnodeKey<T>
where
    T::Value: BlobstoreValue<Key = T>,
{
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

impl<T: MononokeId> FastlogKey<T>
where
    T::Value: BlobstoreValue<Key = T>,
{
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
            HgChangesetViaBonsai,
            HgManifest,
            HgFileEnvelope,
            HgFileNode,
            HgManifestFileNode,
            // Content
            FileContent,
            FileContentMetadataV2,
            AliasContentMapping,
            // Derived
            Blame,
            ChangesetInfo,
            ChangesetInfoMapping,
            DeletedManifestV2,
            DeletedManifestV2Mapping,
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
    (Bookmark, BookmarkKey, [Changeset, BonsaiHgMapping]),
    (
        Changeset,
        ChangesetKey<ChangesetId>,
        [
            FileContent,
            BonsaiParent(Changeset),
            BonsaiHgMapping,
            PhaseMapping,
            ChangesetInfo,
            ChangesetInfoMapping,
            DeletedManifestV2Mapping,
            FsnodeMapping,
            SkeletonManifestMapping,
            UnodeMapping
        ]
    ),
    (BonsaiHgMapping, ChangesetKey<ChangesetId>, [HgBonsaiMapping, HgChangesetViaBonsai]),
    (PhaseMapping, ChangesetId, []),
    (
        PublishedBookmarks,
        UnitKey,
        [Changeset, BonsaiHgMapping]
    ),
    // Hg
    (HgBonsaiMapping, ChangesetKey<HgChangesetId>, [Changeset]),
    (
        HgChangeset,
        ChangesetKey<HgChangesetId>,
        [HgParent(HgChangesetViaBonsai), HgManifest, HgManifestFileNode]
    ),
    (HgChangesetViaBonsai, ChangesetKey<HgChangesetId>, [HgChangeset]),
    (
        HgManifest,
        PathKey<HgManifestId>,
        [HgFileEnvelope, HgFileNode, HgManifestFileNode, ChildHgManifest(HgManifest)]
    ),
    (HgFileEnvelope, HgFileNodeId, [FileContent]),
    (
        HgFileNode,
        PathKey<HgFileNodeId>,
        [
            LinkedHgBonsaiMapping(HgBonsaiMapping),
            LinkedHgChangeset(HgChangesetViaBonsai),
            HgParentFileNode(HgFileNode),
            HgCopyfromFileNode(HgFileNode)
        ]
    ),
    (
        HgManifestFileNode,
        PathKey<HgFileNodeId>,
        [
            LinkedHgBonsaiMapping(HgBonsaiMapping),
            LinkedHgChangeset(HgChangesetViaBonsai),
            HgParentFileNode(HgManifestFileNode),
            HgCopyfromFileNode(HgManifestFileNode)
        ]
    ),
    // Content
    (FileContent, ContentId, [FileContentMetadataV2]),
    (
        FileContentMetadataV2,
        ContentId,
        [
            Sha1Alias(AliasContentMapping),
            Sha256Alias(AliasContentMapping),
            GitSha1Alias(AliasContentMapping),
            SeededBlake3Alias(AliasContentMapping)
        ]
    ),
    (AliasContentMapping, AliasKey, [FileContent]),
    // Derived data
    (
        Blame,
        BlameV2Id,
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
        DeletedManifestV2,
        DeletedManifestV2Id,
        [DeletedManifestV2Child(DeletedManifestV2), LinkedChangeset(Changeset)]
    ),
    (DeletedManifestV2Mapping, ChangesetId, [RootDeletedManifestV2(DeletedManifestV2)]),
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
    pub fn derived_data_type(&self) -> Option<DerivableType> {
        match self {
            NodeType::Root => None,
            // Bonsai
            NodeType::Bookmark => None,
            NodeType::Changeset => None,
            // from filenodes/lib.rs: If hg changeset is not generated, then root filenode can't possible be generated
            // therefore this is the same as MappedHgChangesetId + FilenodesOnlyPublic
            NodeType::BonsaiHgMapping => Some(FilenodesOnlyPublic::VARIANT),
            NodeType::PhaseMapping => None,
            NodeType::PublishedBookmarks => None,
            // Hg
            NodeType::HgBonsaiMapping => Some(MappedHgChangesetId::VARIANT),
            NodeType::HgChangeset => Some(MappedHgChangesetId::VARIANT),
            NodeType::HgChangesetViaBonsai => Some(MappedHgChangesetId::VARIANT),
            NodeType::HgManifest => Some(MappedHgChangesetId::VARIANT),
            NodeType::HgFileEnvelope => Some(MappedHgChangesetId::VARIANT),
            NodeType::HgFileNode => Some(FilenodesOnlyPublic::VARIANT),
            NodeType::HgManifestFileNode => Some(FilenodesOnlyPublic::VARIANT),
            // Content
            NodeType::FileContent => None,
            NodeType::FileContentMetadataV2 => None,
            NodeType::AliasContentMapping => None,
            // Derived data
            NodeType::Blame => Some(RootBlameV2::VARIANT),
            NodeType::ChangesetInfo => Some(ChangesetInfo::VARIANT),
            NodeType::ChangesetInfoMapping => Some(ChangesetInfo::VARIANT),
            NodeType::DeletedManifestV2 => Some(RootDeletedManifestV2Id::VARIANT),
            NodeType::DeletedManifestV2Mapping => Some(RootDeletedManifestV2Id::VARIANT),
            NodeType::FastlogBatch => Some(RootFastlog::VARIANT),
            NodeType::FastlogDir => Some(RootFastlog::VARIANT),
            NodeType::FastlogFile => Some(RootFastlog::VARIANT),
            NodeType::Fsnode => Some(RootFsnodeId::VARIANT),
            NodeType::FsnodeMapping => Some(RootFsnodeId::VARIANT),
            NodeType::SkeletonManifest => Some(RootSkeletonManifestId::VARIANT),
            NodeType::SkeletonManifestMapping => Some(RootSkeletonManifestId::VARIANT),
            NodeType::UnodeFile => Some(RootUnodeManifestId::VARIANT),
            NodeType::UnodeManifest => Some(RootUnodeManifestId::VARIANT),
            NodeType::UnodeMapping => Some(RootUnodeManifestId::VARIANT),
        }
    }

    // Only certain node types can have repo paths associated
    pub fn allow_repo_path(&self) -> bool {
        match self {
            NodeType::Root => false,
            // Bonsai
            NodeType::Bookmark => false,
            NodeType::Changeset => false,
            NodeType::BonsaiHgMapping => false,
            NodeType::PhaseMapping => false,
            NodeType::PublishedBookmarks => false,
            // Hg
            NodeType::HgBonsaiMapping => false,
            NodeType::HgChangeset => false,
            NodeType::HgChangesetViaBonsai => false,
            NodeType::HgManifest => true,
            NodeType::HgFileEnvelope => true,
            NodeType::HgFileNode => true,
            NodeType::HgManifestFileNode => true,
            // Content
            NodeType::FileContent => true,
            NodeType::FileContentMetadataV2 => true,
            NodeType::AliasContentMapping => true,
            // Derived Data
            NodeType::Blame => false,
            NodeType::ChangesetInfo => false,
            NodeType::ChangesetInfoMapping => false,
            NodeType::DeletedManifestV2 => true,
            NodeType::DeletedManifestV2Mapping => false,
            NodeType::FastlogBatch => true,
            NodeType::FastlogDir => true,
            NodeType::FastlogFile => true,
            NodeType::Fsnode => true,
            NodeType::FsnodeMapping => false,
            NodeType::SkeletonManifest => true,
            NodeType::SkeletonManifestMapping => false,
            NodeType::UnodeFile => true,
            NodeType::UnodeManifest => true,
            NodeType::UnodeMapping => false,
        }
    }
}

const ROOT_FINGERPRINT: u64 = 0;

// Can represent Path and PathHash
pub trait WrappedPathLike {
    fn sampling_fingerprint(&self) -> u64;
    fn evolve_path<'a>(
        from_route: Option<&'a Self>,
        walk_item: &'a OutgoingEdge,
    ) -> Option<&'a Self>;
}

/// Represent root or non root path hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WrappedPathHash {
    Root,
    NonRoot(MPathHash),
}

impl WrappedPathHash {
    pub fn as_ref(&self) -> Option<&MPathHash> {
        match self {
            Self::Root => None,
            Self::NonRoot(mpath_hash) => Some(mpath_hash),
        }
    }
}

impl WrappedPathLike for WrappedPathHash {
    fn sampling_fingerprint(&self) -> u64 {
        match self {
            WrappedPathHash::Root => ROOT_FINGERPRINT,
            WrappedPathHash::NonRoot(path_hash) => path_hash.sampling_fingerprint(),
        }
    }
    fn evolve_path<'a>(
        from_route: Option<&'a Self>,
        walk_item: &'a OutgoingEdge,
    ) -> Option<&'a Self> {
        match walk_item.path.as_ref() {
            // Step has set explicit path, e.g. bonsai file
            Some(from_step) => Some(from_step.get_path_hash()),
            None => match walk_item.target.stats_path() {
                // Path is part of node identity
                Some(from_node) => Some(from_node.get_path_hash()),
                // No per-node path, so use the route, filtering out nodes that can't have repo paths
                None => {
                    if walk_item.target.get_type().allow_repo_path() {
                        from_route
                    } else {
                        None
                    }
                }
            },
        }
    }
}

impl fmt::Display for WrappedPathHash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Root => write!(f, ""),
            Self::NonRoot(mpath_hash) => write!(f, "{}", mpath_hash.to_hex()),
        }
    }
}

// Memoize the hash of the path as it is used frequently
#[derive(Debug)]
pub struct MPathWithHashMemo {
    mpath: NonRootMPath,
    memoized_hash: OnceLock<WrappedPathHash>,
}

impl MPathWithHashMemo {
    fn new(mpath: NonRootMPath) -> Self {
        Self {
            mpath,
            memoized_hash: OnceLock::new(),
        }
    }

    pub fn get_path_hash_memo(&self) -> &WrappedPathHash {
        self.memoized_hash
            .get_or_init(|| WrappedPathHash::NonRoot(self.mpath.get_path_hash()))
    }

    pub fn mpath(&self) -> &NonRootMPath {
        &self.mpath
    }
}

impl PartialEq for MPathWithHashMemo {
    fn eq(&self, other: &Self) -> bool {
        self.mpath == other.mpath
    }
}

impl Eq for MPathWithHashMemo {}

impl Hash for MPathWithHashMemo {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.mpath.hash(state);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum WrappedPath {
    Root,
    NonRoot(ArcIntern<EagerHashMemoizer<MPathWithHashMemo>>),
}

impl WrappedPath {
    pub fn as_ref(&self) -> Option<&NonRootMPath> {
        match self {
            WrappedPath::Root => None,
            WrappedPath::NonRoot(path) => Some(path.mpath()),
        }
    }

    pub fn get_path_hash(&self) -> &WrappedPathHash {
        match self {
            WrappedPath::Root => &WrappedPathHash::Root,
            WrappedPath::NonRoot(path) => path.get_path_hash_memo(),
        }
    }
}

impl WrappedPathLike for WrappedPath {
    fn sampling_fingerprint(&self) -> u64 {
        self.get_path_hash().sampling_fingerprint()
    }
    fn evolve_path<'a>(
        from_route: Option<&'a Self>,
        walk_item: &'a OutgoingEdge,
    ) -> Option<&'a Self> {
        match walk_item.path.as_ref() {
            // Step has set explicit path, e.g. bonsai file
            Some(from_step) => Some(from_step),
            None => match walk_item.target.stats_path() {
                // Path is part of node identity
                Some(from_node) => Some(from_node),
                // No per-node path, so use the route, filtering out nodes that can't have repo paths
                None => {
                    if walk_item.target.get_type().allow_repo_path() {
                        from_route
                    } else {
                        None
                    }
                }
            },
        }
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

static PATH_HASHER_FACTORY: OnceLock<RandomState> = OnceLock::new();

impl From<MPath> for WrappedPath {
    fn from(mpath: MPath) -> Self {
        let hasher_fac = PATH_HASHER_FACTORY.get_or_init(RandomState::default);
        match mpath.into_optional_non_root_path() {
            Some(mpath) => WrappedPath::NonRoot(ArcIntern::new(EagerHashMemoizer::new(
                MPathWithHashMemo::new(mpath),
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
        SeededBlake3,
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
    // Weren't able to find node
    MissingAsData(Node),
    // Node has an invalid hash
    HashValidationFailureAsData(Node),
    NotRequired,
    OutsideChunk,
    // Bonsai
    Bookmark(ChangesetId),
    Changeset(BonsaiChangeset),
    BonsaiHgMapping(Option<HgChangesetId>),
    PhaseMapping(Option<Phase>),
    PublishedBookmarks,
    // Hg
    HgBonsaiMapping(Option<ChangesetId>),
    HgChangeset(HgBlobChangeset),
    HgChangesetViaBonsai(HgChangesetId),
    HgManifest(HgBlobManifest),
    HgFileEnvelope(HgFileEnvelope),
    HgFileNode(Option<FilenodeInfo>),
    HgManifestFileNode(Option<FilenodeInfo>),
    // Content
    FileContent(FileContentData),
    FileContentMetadataV2(Option<ContentMetadataV2>),
    AliasContentMapping(ContentId),
    // Derived data
    Blame(Option<BlameV2>),
    ChangesetInfo(Option<ChangesetInfo>),
    ChangesetInfoMapping(Option<ChangesetId>),
    DeletedManifestV2(Option<DeletedManifestV2>),
    DeletedManifestV2Mapping(Option<DeletedManifestV2Id>),
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

#[derive(Clone)]
pub struct SqlShardInfo {
    pub filenodes: SqlTierInfo,
    pub active_keys_per_shard: Option<usize>,
}

// Which type of non-blobstore Mononoke sql shard this node needs access to
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SqlShard {
    Metadata,
    HgFileNode(usize),
}

impl Node {
    /// Map node to an SqlShard if any
    pub fn sql_shard(&self, shard_info: &SqlShardInfo) -> Option<SqlShard> {
        // Only report shards if there is a limit of keys per shard
        shard_info.active_keys_per_shard?;

        match self {
            Node::Root(_) => None,
            // Bonsai
            Node::Bookmark(_) => Some(SqlShard::Metadata),
            Node::Changeset(_) => None,
            Node::BonsaiHgMapping(_) => Some(SqlShard::Metadata),
            Node::PhaseMapping(_) => Some(SqlShard::Metadata),
            Node::PublishedBookmarks(_) => Some(SqlShard::Metadata),
            // Hg
            Node::HgBonsaiMapping(_) => Some(SqlShard::Metadata),
            Node::HgChangeset(_) => None,
            Node::HgChangesetViaBonsai(_) => Some(SqlShard::Metadata),
            Node::HgManifest(PathKey { id: _, path: _ }) => None,
            Node::HgFileEnvelope(_) => None,
            Node::HgFileNode(PathKey { id: _, path }) => {
                let path = path
                    .as_ref()
                    .map_or(RepoPath::RootPath, |p| RepoPath::FilePath(p.clone()));
                let path_hash = PathHash::from_repo_path(&path);
                let shard_num = path_hash.shard_number(shard_info.filenodes.shard_num.unwrap_or(1));
                Some(SqlShard::HgFileNode(shard_num))
            }
            Node::HgManifestFileNode(PathKey { id: _, path }) => {
                let path = path
                    .as_ref()
                    .map_or(RepoPath::RootPath, |p| RepoPath::DirectoryPath(p.clone()));
                let path_hash = PathHash::from_repo_path(&path);
                let shard_num = path_hash.shard_number(shard_info.filenodes.shard_num.unwrap_or(1));
                Some(SqlShard::HgFileNode(shard_num))
            }
            // Content
            Node::FileContent(_) => None,
            Node::FileContentMetadataV2(_) => None,
            Node::AliasContentMapping(_) => None,
            // Derived data
            Node::Blame(_) => None,
            Node::ChangesetInfo(_) => None,
            Node::ChangesetInfoMapping(_) => None,
            Node::DeletedManifestV2(_) => None,
            Node::DeletedManifestV2Mapping(_) => None,
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
            Node::HgChangesetViaBonsai(k) => k.blobstore_key(),
            Node::HgManifest(PathKey { id, path: _ }) => id.blobstore_key(),
            Node::HgFileEnvelope(k) => k.blobstore_key(),
            Node::HgFileNode(PathKey { id, path: _ }) => id.blobstore_key(),
            Node::HgManifestFileNode(PathKey { id, path: _ }) => id.blobstore_key(),
            // Content
            Node::FileContent(k) => k.blobstore_key(),
            Node::FileContentMetadataV2(k) => k.blobstore_key(),
            Node::AliasContentMapping(k) => k.0.blobstore_key(),
            // Derived data
            Node::Blame(k) => k.blobstore_key(),
            Node::ChangesetInfo(k) => k.blobstore_key(),
            Node::ChangesetInfoMapping(k) => k.blobstore_key(),
            Node::DeletedManifestV2(k) => k.blobstore_key(),
            Node::DeletedManifestV2Mapping(k) => k.blobstore_key(),
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
            Node::HgChangesetViaBonsai(_) => None,
            Node::HgManifest(PathKey { id: _, path }) => Some(path),
            Node::HgFileEnvelope(_) => None,
            Node::HgFileNode(PathKey { id: _, path }) => Some(path),
            Node::HgManifestFileNode(PathKey { id: _, path }) => Some(path),
            // Content
            Node::FileContent(_) => None,
            Node::FileContentMetadataV2(_) => None,
            Node::AliasContentMapping(_) => None,
            // Derived data
            Node::Blame(_) => None,
            Node::ChangesetInfo(_) => None,
            Node::ChangesetInfoMapping(_) => None,
            Node::DeletedManifestV2(_) => None,
            Node::DeletedManifestV2Mapping(_) => None,
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
            Node::HgChangesetViaBonsai(k) => Some(k.sampling_fingerprint()),
            Node::HgManifest(PathKey { id, path: _ }) => Some(id.sampling_fingerprint()),
            Node::HgFileEnvelope(k) => Some(k.sampling_fingerprint()),
            Node::HgFileNode(PathKey { id, path: _ }) => Some(id.sampling_fingerprint()),
            Node::HgManifestFileNode(PathKey { id, path: _ }) => Some(id.sampling_fingerprint()),
            // Content
            Node::FileContent(k) => Some(k.sampling_fingerprint()),
            Node::FileContentMetadataV2(k) => Some(k.sampling_fingerprint()),
            Node::AliasContentMapping(k) => Some(k.0.sampling_fingerprint()),
            // Derived data
            Node::Blame(k) => Some(k.sampling_fingerprint()),
            Node::ChangesetInfo(k) => Some(k.sampling_fingerprint()),
            Node::ChangesetInfoMapping(k) => Some(k.sampling_fingerprint()),
            Node::DeletedManifestV2(k) => Some(k.sampling_fingerprint()),
            Node::DeletedManifestV2Mapping(k) => Some(k.sampling_fingerprint()),
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

    pub fn validate_hash(
        &self,
        ctx: CoreContext,
        repo: Repo,
        node_data: &NodeData,
    ) -> BoxFuture<'_, Result<(), HashValidationError>> {
        match (&self, node_data) {
            (Node::HgFileEnvelope(hg_filenode_id), NodeData::HgFileEnvelope(envelope)) => {
                let hg_filenode_id = hg_filenode_id.clone();
                let envelope = envelope.clone();
                async move {
                    let content_id = envelope.content_id();
                    let file_bytes =
                        filestore::fetch(repo.repo_blobstore(), ctx, &envelope.content_id().into())
                            .await?;

                    let file_bytes = file_bytes.ok_or_else(|| {
                        format_err!(
                            "content {} not found for filenode {}",
                            content_id,
                            hg_filenode_id
                        )
                    })?;
                    let HgFileEnvelopeMut {
                        p1, p2, metadata, ..
                    } = envelope.into_mut();
                    let p1 = p1.map(|p| p.into_nodehash());
                    let p2 = p2.map(|p| p.into_nodehash());
                    let actual = calculate_hg_node_id_stream(
                        stream::once(async { Ok(metadata) }).chain(file_bytes),
                        &HgParents::new(p1, p2),
                    )
                    .await?;
                    let actual = HgFileNodeId::new(actual);

                    if actual != hg_filenode_id {
                        return Err(HashValidationError::HashMismatch {
                            actual_hash: format!("{}", actual),
                            expected_hash: format!("{}", hg_filenode_id),
                        });
                    }
                    Ok(())
                }
                .boxed()
            }
            _ => {
                let ty = self.get_type();
                let s: &str = ty.into();
                async move { Err(HashValidationError::NotSupported(s.to_string())) }.boxed()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::mem::size_of;

    use mononoke_macros::mononoke;
    use strum::EnumCount;
    use strum::IntoEnumIterator;

    use super::*;

    #[mononoke::test]
    fn test_node_size() {
        // Node size is important as we have lots of them, add a test to check for accidental changes
        assert_eq!(40, size_of::<Node>());
    }

    #[mononoke::test]
    fn test_node_type_max_ordinal() {
        // Check the macros worked consistently
        for t in NodeType::iter() {
            assert!((t as usize) < NodeType::COUNT)
        }
    }

    #[mononoke::test]
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

    #[mononoke::test]
    fn test_all_derived_data_types_supported() {
        // All enabled types for the repo
        let a = test_repo_factory::default_test_repo_config()
            .derived_data_config
            .get_active_config()
            .expect("No enabled derived data types config")
            .types
            .clone();

        // supported in graph
        let mut s = HashSet::new();
        for t in NodeType::iter() {
            if let Some(d) = t.derived_data_type() {
                assert!(
                    a.contains(&d),
                    "graph derived data type {} for {} is not known by default_test_repo_config()",
                    d,
                    t
                );
                s.insert(d);
            }
        }

        // If you are adding a new derived data type, please add it to the walker graph rather than to this
        // list, otherwise it won't get scrubbed and thus you would be unaware of different representation
        // in different stores
        let grandfathered: HashSet<DerivableType> = HashSet::from_iter(vec![
            DerivableType::GitCommits,
            DerivableType::GitDeltaManifestsV2,
            DerivableType::GitDeltaManifestsV3,
            DerivableType::TestManifests,
            DerivableType::TestShardedManifests,
            DerivableType::BssmV3,
            DerivableType::Ccsm,
            DerivableType::HgAugmentedManifests,
            DerivableType::SkeletonManifestsV2,
            DerivableType::ContentManifests,
            DerivableType::InferredCopyFrom,
        ]);
        let mut missing = HashSet::new();
        for t in a {
            if s.contains(&t) {
                assert!(
                    !grandfathered.contains(&t),
                    "You've added support for {}, please remove it from the grandfathered missing set",
                    t
                );
            } else if !grandfathered.contains(&t) {
                missing.insert(t);
            }
        }
        assert!(
            missing.is_empty(),
            "Derived data types {:?} not supported by walker graph",
            missing,
        );
    }
}
