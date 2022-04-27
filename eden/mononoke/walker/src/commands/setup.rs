/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::graph::{EdgeType, NodeType};
use crate::progress::{ProgressStateCountByType, ProgressStateMutex, ProgressSummary};
use crate::state::StepStats;
use crate::tail::TailParams;
use crate::walk::RepoWalkParams;

use anyhow::{format_err, Context, Error};
use derived_data_filenodes::FilenodesOnlyPublic;
use derived_data_manager::BonsaiDerivable as NewBonsaiDerivable;
use maplit::hashset;
use mercurial_derived_data::MappedHgChangesetId;
use once_cell::sync::Lazy;
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
};
use strum::{IntoEnumIterator, VariantNames};
use strum_macros::{AsRefStr, EnumString, EnumVariantNames};

// Per repo things we don't pass into the walk
pub struct RepoSubcommandParams {
    pub progress_state: ProgressStateMutex<ProgressStateCountByType<StepStats, ProgressSummary>>,
    pub tail_params: TailParams,
    pub lfs_threshold: Option<u64>,
}

// These don't vary per repo
#[derive(Clone)]
pub struct JobWalkParams {
    pub enable_derive: bool,
    pub quiet: bool,
    pub error_as_data_node_types: HashSet<NodeType>,
    pub error_as_data_edge_types: HashSet<EdgeType>,
    pub repo_count: usize,
}

pub struct JobParams {
    pub walk_params: JobWalkParams,
    pub per_repo: Vec<(RepoSubcommandParams, RepoWalkParams)>,
}

pub const PROGRESS_SAMPLE_RATE: u64 = 1000;
pub const PROGRESS_SAMPLE_DURATION_S: u64 = 5;

// Sub commands
pub const SCRUB: &str = "scrub";
pub const COMPRESSION_BENEFIT: &str = "compression-benefit";
pub const VALIDATE: &str = "validate";
pub const CORPUS: &str = "corpus";

const DEFAULT_VALUE_ARG: &str = "default";
const DERIVED_VALUE_ARG: &str = "derived";
const SHALLOW_VALUE_ARG: &str = "shallow";
const DEEP_VALUE_ARG: &str = "deep";
const MARKER_VALUE_ARG: &str = "marker";
const HG_VALUE_ARG: &str = "hg";
const BONSAI_VALUE_ARG: &str = "bonsai";
const CONTENT_META_VALUE_ARG: &str = "contentmeta";
const ALL_VALUE_ARG: &str = "all";

const DERIVED_PREFIX: &str = "derived_";

static DERIVED_DATA_INCLUDE_NODE_TYPES: Lazy<HashMap<String, Vec<NodeType>>> = Lazy::new(|| {
    let mut m: HashMap<String, Vec<NodeType>> = HashMap::new();
    for t in NodeType::iter() {
        if let Some(n) = t.derived_data_name() {
            m.entry(format!("{}{}", DERIVED_PREFIX, n))
                .or_default()
                .push(t);
        }
    }
    m
});

#[allow(dead_code)]
static NODE_TYPE_POSSIBLE_VALUES: Lazy<Vec<&'static str>> = Lazy::new(|| {
    let mut v = vec![
        ALL_VALUE_ARG,
        BONSAI_VALUE_ARG,
        DEFAULT_VALUE_ARG,
        DERIVED_VALUE_ARG,
        HG_VALUE_ARG,
    ];
    v.extend(
        DERIVED_DATA_INCLUDE_NODE_TYPES
            .keys()
            .map(|e| e.as_ref() as &'static str),
    );
    v.extend(NodeType::VARIANTS.iter());
    v
});

#[allow(dead_code)]
static EDGE_TYPE_POSSIBLE_VALUES: Lazy<Vec<&'static str>> = Lazy::new(|| {
    let mut v = vec![
        DEEP_VALUE_ARG,
        SHALLOW_VALUE_ARG,
        ALL_VALUE_ARG,
        BONSAI_VALUE_ARG,
        HG_VALUE_ARG,
        CONTENT_META_VALUE_ARG,
        MARKER_VALUE_ARG,
    ];
    v.extend(EdgeType::VARIANTS.iter());
    v
});

pub const DEFAULT_INCLUDE_NODE_TYPES: &[NodeType] = &[
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

const BONSAI_INCLUDE_NODE_TYPES: &[NodeType] = &[NodeType::Bookmark, NodeType::Changeset];

// Goes as far into history as it can
pub const DEEP_INCLUDE_EDGE_TYPES: &[EdgeType] = &[
    // Bonsai
    EdgeType::BookmarkToChangeset,
    EdgeType::ChangesetToFileContent,
    EdgeType::ChangesetToBonsaiParent,
    EdgeType::ChangesetToBonsaiHgMapping,
    EdgeType::BonsaiHgMappingToHgChangesetViaBonsai,
    EdgeType::PublishedBookmarksToChangeset,
    EdgeType::PublishedBookmarksToBonsaiHgMapping,
    EdgeType::ChangesetToChangesetInfoMapping,
    EdgeType::ChangesetToDeletedManifestMapping,
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
    EdgeType::DeletedManifestMappingToRootDeletedManifest,
    EdgeType::DeletedManifestToDeletedManifestChild,
    EdgeType::DeletedManifestToLinkedChangeset,
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
    EdgeType::ChangesetToDeletedManifestMapping,
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
    EdgeType::DeletedManifestMappingToRootDeletedManifest,
    EdgeType::DeletedManifestToDeletedManifestChild,
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

#[derive(Clone, Debug, PartialEq, Eq, AsRefStr, EnumVariantNames, EnumString)]
pub enum OutputFormat {
    Debug,
    PrettyDebug,
}

// Things like phases and obs markers will go here
const MARKER_EDGE_TYPES: &[EdgeType] = &[EdgeType::ChangesetToPhaseMapping];

// parse the pre-defined groups we have for default etc
pub fn parse_node_value(arg: &str) -> Result<HashSet<NodeType>, Error> {
    Ok(match arg {
        ALL_VALUE_ARG => HashSet::from_iter(NodeType::iter()),
        DEFAULT_VALUE_ARG => HashSet::from_iter(DEFAULT_INCLUDE_NODE_TYPES.iter().cloned()),
        BONSAI_VALUE_ARG => HashSet::from_iter(BONSAI_INCLUDE_NODE_TYPES.iter().cloned()),
        DERIVED_VALUE_ARG => {
            HashSet::from_iter(DERIVED_DATA_INCLUDE_NODE_TYPES.values().flatten().cloned())
        }
        HG_VALUE_ARG => {
            let mut s = HashSet::new();
            for d in &[MappedHgChangesetId::NAME, FilenodesOnlyPublic::NAME] {
                let d = DERIVED_DATA_INCLUDE_NODE_TYPES.get(&format!("{}{}", DERIVED_PREFIX, d));
                s.extend(d.unwrap().iter().cloned());
            }
            s
        }
        _ => {
            if let Some(v) = DERIVED_DATA_INCLUDE_NODE_TYPES.get(arg) {
                HashSet::from_iter(v.iter().cloned())
            } else {
                NodeType::from_str(arg)
                    .map(|e| hashset![e])
                    .with_context(|| format_err!("Unknown NodeType {}", arg))?
            }
        }
    })
}

// parse the pre-defined groups we have for deep, shallow, hg, bonsai etc.
pub fn parse_edge_value(arg: &str) -> Result<HashSet<EdgeType>, Error> {
    Ok(match arg {
        ALL_VALUE_ARG => HashSet::from_iter(EdgeType::iter()),
        BONSAI_VALUE_ARG => HashSet::from_iter(BONSAI_EDGE_TYPES.iter().cloned()),
        CONTENT_META_VALUE_ARG => HashSet::from_iter(CONTENT_META_EDGE_TYPES.iter().cloned()),
        DEEP_VALUE_ARG => HashSet::from_iter(DEEP_INCLUDE_EDGE_TYPES.iter().cloned()),
        MARKER_VALUE_ARG => HashSet::from_iter(MARKER_EDGE_TYPES.iter().cloned()),
        HG_VALUE_ARG => HashSet::from_iter(HG_EDGE_TYPES.iter().cloned()),
        SHALLOW_VALUE_ARG => HashSet::from_iter(SHALLOW_INCLUDE_EDGE_TYPES.iter().cloned()),
        _ => EdgeType::from_str(arg)
            .map(|e| hashset![e])
            .with_context(|| format_err!("Unknown EdgeType {}", arg))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bad_parse_node_value() {
        let r = parse_node_value("bad_node_type");
        assert!(r.is_err());
    }
}
