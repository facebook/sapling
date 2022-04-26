/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::graph::{EdgeType, NodeType, SqlShardInfo};
use crate::log;
use crate::progress::{
    sort_by_string, ProgressOptions, ProgressStateCountByType, ProgressStateMutex, ProgressSummary,
};
use crate::state::StepStats;
use crate::tail::TailParams;
use crate::validate::REPO;
use crate::walk::{OutgoingEdge, RepoWalkParams};

use anyhow::{format_err, Context, Error};
use blobrepo::BlobRepo;
use cmdlib::args::ResolvedRepo;
use derived_data_filenodes::FilenodesOnlyPublic;
use derived_data_manager::BonsaiDerivable as NewBonsaiDerivable;
use fbinit::FacebookInit;
use maplit::hashset;
use mercurial_derived_data::MappedHgChangesetId;
use once_cell::sync::Lazy;
use repo_factory::RepoFactory;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::{info, o, Logger};
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

fn reachable_graph_elements(
    mut include_edge_types: HashSet<EdgeType>,
    mut include_node_types: HashSet<NodeType>,
    root_node_types: &HashSet<NodeType>,
) -> (HashSet<EdgeType>, HashSet<NodeType>) {
    // This stops us logging that we're walking unreachable edge/node types
    let mut param_count = &include_edge_types.len() + &include_node_types.len();
    let mut last_param_count = 0;
    while param_count != last_param_count {
        let include_edge_types_stable = include_edge_types.clone();
        // Only retain edge types that are traversable
        include_edge_types.retain(|e| {
            e.incoming_type()
                .map(|t|
                    // its an incoming_type we want
                    (include_node_types.contains(&t) || root_node_types.contains(&t)) &&
                    // Another existing edge can get us to this node type
                    (root_node_types.contains(&t) || include_edge_types_stable.iter().any(|o| &o.outgoing_type() == &t)))
                .unwrap_or(true)
                // its an outgoing_type we want
                && include_node_types.contains(&e.outgoing_type())
        });
        // Only retain node types we expect to step to after graph entry
        include_node_types.retain(|t| {
            include_edge_types.iter().any(|e| {
                &e.outgoing_type() == t || e.incoming_type().map(|ot| &ot == t).unwrap_or(false)
            })
        });
        last_param_count = param_count;
        param_count = &include_edge_types.len() + &include_node_types.len();
    }
    (include_edge_types, include_node_types)
}

// Setup for just one repo. Try and keep clap parsing out of here, should be done beforehand
pub async fn setup_repo<'a>(
    walk_stats_key: &'static str,
    fb: FacebookInit,
    logger: &'a Logger,
    repo_factory: &'a RepoFactory,
    mut scuba_builder: MononokeScubaSampleBuilder,
    sql_shard_info: SqlShardInfo,
    scheduled_max: usize,
    repo_count: usize,
    resolved: &'a ResolvedRepo,
    walk_roots: Vec<OutgoingEdge>,
    mut tail_params: TailParams,
    include_edge_types: HashSet<EdgeType>,
    mut include_node_types: HashSet<NodeType>,
    hash_validation_node_types: HashSet<NodeType>,
    progress_options: ProgressOptions,
) -> Result<(RepoSubcommandParams, RepoWalkParams), Error> {
    let logger = if repo_count > 1 {
        logger.new(o!("repo" => resolved.name.clone()))
    } else {
        logger.clone()
    };

    let scheduled_max = scheduled_max / repo_count;
    scuba_builder.add(REPO, resolved.name.clone());

    // Only walk derived node types that the repo is configured to contain
    include_node_types.retain(|t| {
        if let Some(t) = t.derived_data_name() {
            resolved.config.derived_data_config.is_enabled(t)
        } else {
            true
        }
    });

    let mut root_node_types: HashSet<_> =
        walk_roots.iter().map(|e| e.label.outgoing_type()).collect();

    if let Some(ref mut chunking) = tail_params.chunking {
        chunking.chunk_by.retain(|t| {
            if let Some(t) = t.derived_data_name() {
                resolved.config.derived_data_config.is_enabled(t)
            } else {
                true
            }
        });

        root_node_types.extend(chunking.chunk_by.iter().cloned());
    }

    let (include_edge_types, include_node_types) =
        reachable_graph_elements(include_edge_types, include_node_types, &root_node_types);
    info!(
        logger,
        #log::GRAPH,
        "Walking edge types {:?}",
        sort_by_string(&include_edge_types)
    );
    info!(
        logger,
        #log::GRAPH,
        "Walking node types {:?}",
        sort_by_string(&include_node_types)
    );

    scuba_builder.add(REPO, resolved.name.clone());

    let mut progress_node_types = include_node_types.clone();
    for e in &walk_roots {
        progress_node_types.insert(e.target.get_type());
    }

    let progress_state = ProgressStateMutex::new(ProgressStateCountByType::new(
        fb,
        logger.clone(),
        walk_stats_key,
        resolved.name.clone(),
        progress_node_types,
        progress_options,
    ));

    let repo: BlobRepo = repo_factory
        .build(resolved.name.clone(), resolved.config.clone())
        .await?;

    Ok((
        RepoSubcommandParams {
            progress_state,
            tail_params,
            lfs_threshold: resolved.config.lfs.threshold,
        },
        RepoWalkParams {
            repo,
            logger: logger.clone(),
            scheduled_max,
            sql_shard_info,
            walk_roots,
            include_node_types,
            include_edge_types,
            hash_validation_node_types,
            scuba_builder,
        },
    ))
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
