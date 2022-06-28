/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::detail::graph::EdgeType;
use crate::detail::graph::NodeType;
use anyhow::format_err;
use anyhow::Context as _;
use anyhow::Error;
use derived_data_filenodes::FilenodesOnlyPublic;
use derived_data_manager::derivable::BonsaiDerivable;
use mercurial_derived_data::MappedHgChangesetId;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::Hash;
use std::str::FromStr;
use strum::IntoEnumIterator;

const ALL: &str = "all";
const BONSAI: &str = "bonsai";
pub const DEFAULT: &str = "default";
const DERIVED: &str = "derived";
const HG: &str = "hg";
pub const DEEP: &str = "deep";
const SHALLOW: &str = "shallow";
const CONTENTMETA: &str = "contentmeta";
const MARKER: &str = "marker";

/* NodeType */

const DEFAULT_INCLUDE_NODE_TYPES: &[NodeType] = &[
    NodeType::Bookmark,
    NodeType::Changeset,
    NodeType::BonsaiHgMapping,
    NodeType::PhaseMapping,
    NodeType::PublishedBookmarks,
    NodeType::HgBonsaiMapping,
    NodeType::HgChangeset,
    NodeType::HgChangesetViaBonsai,
    NodeType::HgManifest,
    NodeType::HgFileEnvelope,
    NodeType::HgFileNode,
    NodeType::FileContent,
    NodeType::FileContentMetadata,
    NodeType::AliasContentMapping,
];

const BONSAI_NODE_TYPES: &[NodeType] = &[NodeType::Bookmark, NodeType::Changeset];
const HG_DERIVED_TYPES: &[&str] = &[MappedHgChangesetId::NAME, FilenodesOnlyPublic::NAME];

const DERIVED_PREFIX: &str = "derived_";

static DERIVED_DATA_NODE_TYPES: Lazy<HashMap<String, Vec<NodeType>>> = Lazy::new(|| {
    let mut m: HashMap<String, Vec<NodeType>> = HashMap::new();
    for t in NodeType::iter() {
        if let Some(name) = t.derived_data_name() {
            m.entry(format!("{}{}", DERIVED_PREFIX, name))
                .or_default()
                .push(t);
        }
    }
    m
});

pub type NodeTypeArg = GraphTypeArg<NodeType>;

impl FromStr for NodeTypeArg {
    type Err = Error;

    fn from_str(arg: &str) -> Result<NodeTypeArg, Error> {
        Ok(match arg {
            ALL => GraphTypeArg(NodeType::iter().collect()),
            BONSAI => NodeTypeArg::new(BONSAI_NODE_TYPES.iter()),
            DEFAULT => NodeTypeArg::new(DEFAULT_INCLUDE_NODE_TYPES.iter()),
            DERIVED => NodeTypeArg::new(DERIVED_DATA_NODE_TYPES.values().flatten()),
            HG => {
                let mut node_types = vec![];
                for hg_derived in HG_DERIVED_TYPES {
                    let hg_derived = format!("{}{}", DERIVED_PREFIX, hg_derived);
                    let nodes_derived = DERIVED_DATA_NODE_TYPES.get(&hg_derived);
                    if let Some(nd) = nodes_derived {
                        nd.iter().for_each(|node| node_types.push(node.clone()));
                    }
                }
                GraphTypeArg(node_types)
            }
            _ => {
                if let Some(node_types) = DERIVED_DATA_NODE_TYPES.get(arg) {
                    GraphTypeArg(node_types.clone())
                } else {
                    NodeType::from_str(arg)
                        .map(|e| GraphTypeArg(vec![e]))
                        .with_context(|| format_err!("Unknown NodeType {}", arg))?
                }
            }
        })
    }
}

/* EdgeType */

// Goes as far into history as it can
const DEEP_INCLUDE_EDGE_TYPES: &[EdgeType] = &[
    // Bonsai
    EdgeType::BookmarkToChangeset,
    EdgeType::ChangesetToFileContent,
    EdgeType::ChangesetToBonsaiParent,
    EdgeType::ChangesetToBonsaiHgMapping,
    EdgeType::BonsaiHgMappingToHgChangesetViaBonsai,
    EdgeType::PublishedBookmarksToChangeset,
    EdgeType::PublishedBookmarksToBonsaiHgMapping,
    EdgeType::ChangesetToChangesetInfoMapping,
    EdgeType::ChangesetToDeletedManifestV2Mapping,
    EdgeType::ChangesetToFsnodeMapping,
    EdgeType::ChangesetToSkeletonManifestMapping,
    EdgeType::ChangesetToUnodeMapping,
    // Hg
    EdgeType::HgBonsaiMappingToChangeset,
    EdgeType::HgChangesetToHgParent,
    EdgeType::HgChangesetToHgManifest,
    EdgeType::HgChangesetToHgManifestFileNode,
    EdgeType::HgChangesetViaBonsaiToHgChangeset,
    EdgeType::HgManifestToHgFileEnvelope,
    EdgeType::HgManifestToHgFileNode,
    EdgeType::HgManifestToChildHgManifest,
    EdgeType::HgFileEnvelopeToFileContent,
    EdgeType::HgFileNodeToLinkedHgBonsaiMapping,
    EdgeType::HgFileNodeToLinkedHgChangeset,
    EdgeType::HgFileNodeToHgParentFileNode,
    EdgeType::HgFileNodeToHgCopyfromFileNode,
    EdgeType::HgManifestFileNodeToLinkedHgBonsaiMapping,
    EdgeType::HgManifestFileNodeToLinkedHgChangeset,
    EdgeType::HgManifestFileNodeToHgParentFileNode,
    EdgeType::HgManifestFileNodeToHgCopyfromFileNode,
    // Content
    EdgeType::FileContentToFileContentMetadata,
    EdgeType::FileContentMetadataToSha1Alias,
    EdgeType::FileContentMetadataToSha256Alias,
    EdgeType::FileContentMetadataToGitSha1Alias,
    EdgeType::AliasContentMappingToFileContent,
    // Derived data
    EdgeType::BlameToChangeset,
    EdgeType::ChangesetInfoMappingToChangesetInfo,
    EdgeType::ChangesetInfoToChangesetInfoParent,
    EdgeType::DeletedManifestV2MappingToRootDeletedManifestV2,
    EdgeType::DeletedManifestV2ToDeletedManifestV2Child,
    EdgeType::DeletedManifestV2ToLinkedChangeset,
    EdgeType::FastlogBatchToChangeset,
    EdgeType::FastlogBatchToPreviousBatch,
    EdgeType::FastlogDirToChangeset,
    EdgeType::FastlogDirToPreviousBatch,
    EdgeType::FastlogFileToChangeset,
    EdgeType::FastlogFileToPreviousBatch,
    EdgeType::FsnodeMappingToRootFsnode,
    EdgeType::FsnodeToChildFsnode,
    EdgeType::FsnodeToFileContent,
    EdgeType::SkeletonManifestMappingToRootSkeletonManifest,
    EdgeType::SkeletonManifestToSkeletonManifestChild,
    EdgeType::UnodeFileToBlame,
    EdgeType::UnodeFileToFastlogFile,
    EdgeType::UnodeFileToFileContent,
    EdgeType::UnodeFileToLinkedChangeset,
    EdgeType::UnodeFileToUnodeFileParent,
    EdgeType::UnodeManifestToFastlogDir,
    EdgeType::UnodeManifestToLinkedChangeset,
    EdgeType::UnodeManifestToUnodeManifestParent,
    EdgeType::UnodeManifestToUnodeFileChild,
    EdgeType::UnodeManifestToUnodeManifestChild,
    EdgeType::UnodeMappingToRootUnodeManifest,
];

// Does not recurse into history, edges to parents excluded
const SHALLOW_INCLUDE_EDGE_TYPES: &[EdgeType] = &[
    // Bonsai
    EdgeType::BookmarkToChangeset,
    EdgeType::ChangesetToFileContent,
    EdgeType::ChangesetToBonsaiHgMapping,
    EdgeType::BonsaiHgMappingToHgChangesetViaBonsai,
    EdgeType::PublishedBookmarksToChangeset,
    EdgeType::PublishedBookmarksToBonsaiHgMapping,
    EdgeType::ChangesetToChangesetInfoMapping,
    EdgeType::ChangesetToDeletedManifestV2Mapping,
    EdgeType::ChangesetToFsnodeMapping,
    EdgeType::ChangesetToSkeletonManifestMapping,
    EdgeType::ChangesetToUnodeMapping,
    // Hg
    EdgeType::HgBonsaiMappingToChangeset,
    EdgeType::HgChangesetToHgManifest,
    EdgeType::HgChangesetToHgManifestFileNode,
    EdgeType::HgChangesetViaBonsaiToHgChangeset,
    EdgeType::HgManifestToHgFileEnvelope,
    EdgeType::HgManifestToHgFileNode,
    EdgeType::HgManifestToHgManifestFileNode,
    EdgeType::HgManifestToChildHgManifest,
    EdgeType::HgFileEnvelopeToFileContent,
    // Content
    EdgeType::FileContentToFileContentMetadata,
    EdgeType::FileContentMetadataToSha1Alias,
    EdgeType::FileContentMetadataToSha256Alias,
    EdgeType::FileContentMetadataToGitSha1Alias,
    EdgeType::AliasContentMappingToFileContent,
    // Derived data
    EdgeType::ChangesetInfoMappingToChangesetInfo,
    EdgeType::DeletedManifestV2MappingToRootDeletedManifestV2,
    EdgeType::DeletedManifestV2ToDeletedManifestV2Child,
    EdgeType::FastlogBatchToPreviousBatch,
    EdgeType::FastlogDirToPreviousBatch,
    EdgeType::FastlogFileToPreviousBatch,
    EdgeType::FsnodeToChildFsnode,
    EdgeType::FsnodeToFileContent,
    EdgeType::FsnodeMappingToRootFsnode,
    EdgeType::SkeletonManifestMappingToRootSkeletonManifest,
    EdgeType::SkeletonManifestToSkeletonManifestChild,
    EdgeType::UnodeFileToBlame,
    EdgeType::UnodeFileToFastlogFile,
    EdgeType::UnodeFileToFileContent,
    EdgeType::UnodeManifestToFastlogDir,
    EdgeType::UnodeManifestToUnodeFileChild,
    EdgeType::UnodeManifestToUnodeManifestChild,
    EdgeType::UnodeMappingToRootUnodeManifest,
];

// Types that can result in loading hg data.  Useful for excludes.
const HG_EDGE_TYPES: &[EdgeType] = &[
    // Bonsai to Hg
    EdgeType::BookmarkToBonsaiHgMapping,
    EdgeType::BonsaiHgMappingToHgChangesetViaBonsai,
    EdgeType::PublishedBookmarksToBonsaiHgMapping,
    // Hg
    EdgeType::HgChangesetToHgManifest,
    EdgeType::HgChangesetToHgManifestFileNode,
    EdgeType::HgChangesetToHgParent,
    EdgeType::HgChangesetViaBonsaiToHgChangeset,
    EdgeType::HgManifestToHgFileEnvelope,
    EdgeType::HgManifestToHgFileNode,
    EdgeType::HgManifestToChildHgManifest,
    EdgeType::HgFileEnvelopeToFileContent,
    EdgeType::HgFileNodeToLinkedHgChangeset,
    EdgeType::HgFileNodeToHgParentFileNode,
    EdgeType::HgFileNodeToHgCopyfromFileNode,
    EdgeType::HgManifestFileNodeToLinkedHgChangeset,
    EdgeType::HgManifestFileNodeToHgParentFileNode,
    EdgeType::HgManifestFileNodeToHgCopyfromFileNode,
];

// Types that can result in loading bonsai data
const BONSAI_EDGE_TYPES: &[EdgeType] = &[
    // Bonsai
    EdgeType::BookmarkToChangeset,
    EdgeType::ChangesetToFileContent,
    EdgeType::ChangesetToBonsaiParent,
    EdgeType::PublishedBookmarksToChangeset,
];

const CONTENT_META_EDGE_TYPES: &[EdgeType] = &[
    // Content
    EdgeType::FileContentToFileContentMetadata,
    EdgeType::FileContentMetadataToSha1Alias,
    EdgeType::FileContentMetadataToSha256Alias,
    EdgeType::FileContentMetadataToGitSha1Alias,
    EdgeType::AliasContentMappingToFileContent,
];

// Things like phases and obs markers will go here
const MARKER_EDGE_TYPES: &[EdgeType] = &[EdgeType::ChangesetToPhaseMapping];

pub type EdgeTypeArg = GraphTypeArg<EdgeType>;

impl FromStr for EdgeTypeArg {
    type Err = Error;

    fn from_str(arg: &str) -> Result<EdgeTypeArg, Error> {
        Ok(match arg {
            ALL => GraphTypeArg(EdgeType::iter().collect()),
            DEEP => EdgeTypeArg::new(DEEP_INCLUDE_EDGE_TYPES.iter()),
            SHALLOW => EdgeTypeArg::new(SHALLOW_INCLUDE_EDGE_TYPES.iter()),
            CONTENTMETA => EdgeTypeArg::new(CONTENT_META_EDGE_TYPES.iter()),
            MARKER => EdgeTypeArg::new(MARKER_EDGE_TYPES.iter()),
            BONSAI => EdgeTypeArg::new(BONSAI_EDGE_TYPES.iter()),
            HG => EdgeTypeArg::new(HG_EDGE_TYPES.iter()),
            _ => EdgeType::from_str(arg)
                .map(|e| GraphTypeArg(vec![e]))
                .with_context(|| format_err!("Unknown EdgeType {}", arg))?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct GraphTypeArg<T>(pub Vec<T>);

impl<'a, T: 'a + Clone + Eq + Hash> GraphTypeArg<T> {
    pub fn new(it: impl Iterator<Item = &'a T>) -> Self {
        GraphTypeArg(it.cloned().collect())
    }

    pub fn parse_args(args: &[Self]) -> HashSet<T> {
        HashSet::from_iter(args.iter().flat_map(|arg| arg.0.clone()))
    }

    pub fn filter(include: &[Self], exclude: &[Self]) -> HashSet<T> {
        let mut include = Self::parse_args(include);
        let exclude = Self::parse_args(exclude);
        include.retain(|x| !exclude.contains(x));
        include
    }
}
