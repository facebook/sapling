/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::blobstore;
use crate::graph::{EdgeType, Node, NodeType};
use crate::parse_node::parse_node;
use crate::progress::{
    sort_by_string, ProgressStateCountByType, ProgressStateMutex, ProgressSummary,
};
use crate::state::StepStats;
use crate::validate::{CheckType, REPO, WALK_TYPE};
use crate::walk::OutgoingEdge;

use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use blobrepo_factory::open_blobrepo_given_datasources;
use blobstore_factory::make_metadata_sql_factory;
use bookmarks::BookmarkName;
use clap::{App, Arg, ArgMatches, SubCommand, Values};
use cmdlib::args;
use fbinit::FacebookInit;
use futures::{
    compat::Future01CompatExt,
    future::{BoxFuture, FutureExt, TryFutureExt},
};
use futures_ext::FutureExt as _;
use lazy_static::lazy_static;
use metaconfig_types::{Redaction, ScrubAction};
use samplingblob::SamplingHandler;
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use slog::{info, warn, Logger};
use std::{collections::HashSet, iter::FromIterator, str::FromStr, sync::Arc, time::Duration};

pub struct RepoWalkDatasources {
    pub blobrepo: BoxFuture<'static, Result<BlobRepo, Error>>,
    pub scuba_builder: ScubaSampleBuilder,
}

#[derive(Clone)]
pub struct RepoWalkParams {
    pub enable_derive: bool,
    pub scheduled_max: usize,
    pub walk_roots: Vec<OutgoingEdge>,
    pub include_node_types: HashSet<NodeType>,
    pub include_edge_types: HashSet<EdgeType>,
    pub tail_secs: Option<u64>,
    pub quiet: bool,
    pub progress_state: ProgressStateMutex<ProgressStateCountByType<StepStats, ProgressSummary>>,
    pub error_as_data_node_types: HashSet<NodeType>,
    pub error_as_data_edge_types: HashSet<EdgeType>,
}

pub const PROGRESS_SAMPLE_RATE: u64 = 1000;
pub const PROGRESS_SAMPLE_DURATION_S: u64 = 5;

// Sub commands
pub const SCRUB: &str = "scrub";
pub const COMPRESSION_BENEFIT: &str = "compression-benefit";
pub const VALIDATE: &str = "validate";

// Subcommand args
const QUIET_ARG: &str = "quiet";
const ENABLE_REDACTION_ARG: &str = "enable-redaction";
const SCHEDULED_MAX_ARG: &str = "scheduled-max";
const TAIL_INTERVAL_ARG: &str = "tail-interval";
const ERROR_AS_DATA_NODE_TYPE_ARG: &str = "error-as-data-node-type";
const ERROR_AS_DATA_EDGE_TYPE_ARG: &str = "error-as-data-edge-type";
const EXCLUDE_NODE_TYPE_ARG: &str = "exclude-node-type";
const INCLUDE_NODE_TYPE_ARG: &str = "include-node-type";
const EXCLUDE_EDGE_TYPE_ARG: &str = "exclude-edge-type";
const INCLUDE_EDGE_TYPE_ARG: &str = "include-edge-type";
const BOOKMARK_ARG: &str = "bookmark";
const WALK_ROOT_ARG: &str = "walk-root";
const INNER_BLOBSTORE_ID_ARG: &str = "inner-blobstore-id";
const SCRUB_BLOBSTORE_ACTION_ARG: &str = "scrub-blobstore-action";
const ENABLE_DERIVE_ARG: &str = "enable-derive";
const PROGRESS_SAMPLE_RATE_ARG: &str = "progress-sample-rate";
const PROGRESS_INTERVAL_ARG: &str = "progress-interval";
pub const LIMIT_DATA_FETCH_ARG: &str = "limit-data-fetch";
pub const COMPRESSION_LEVEL_ARG: &str = "compression-level";
pub const SAMPLE_RATE_ARG: &str = "sample-rate";
pub const EXCLUDE_CHECK_TYPE_ARG: &str = "exclude-check-type";
pub const INCLUDE_CHECK_TYPE_ARG: &str = "include-check-type";
pub const EXCLUDE_SAMPLE_NODE_TYPE_ARG: &str = "exclude-sample-node-type";
pub const INCLUDE_SAMPLE_NODE_TYPE_ARG: &str = "include-sample-node-type";
const SCUBA_TABLE_ARG: &str = "scuba-table";
const SCUBA_LOG_FILE_ARG: &str = "scuba-log-file";

const SHALLOW_VALUE_ARG: &str = "shallow";
const DEEP_VALUE_ARG: &str = "deep";
const MARKER_VALUE_ARG: &str = "marker";
const HG_VALUE_ARG: &str = "hg";
const BONSAI_VALUE_ARG: &str = "bonsai";
const CONTENT_META_VALUE_ARG: &str = "contentmeta";

// Toplevel args - healer and populate healer have this one at top level
// so keeping it there for consistency
const STORAGE_ID_ARG: &str = "storage-id";

pub const DEFAULT_INCLUDE_NODE_TYPES: &[NodeType] = &[
    NodeType::Bookmark,
    NodeType::BonsaiChangeset,
    NodeType::BonsaiHgMapping,
    NodeType::BonsaiPhaseMapping,
    NodeType::PublishedBookmarks,
    NodeType::HgBonsaiMapping,
    NodeType::HgChangeset,
    NodeType::HgManifest,
    NodeType::HgFileEnvelope,
    NodeType::HgFileNode,
    NodeType::FileContent,
    NodeType::FileContentMetadata,
    NodeType::AliasContentMapping,
];

// Goes as far into history as it can
const DEEP_INCLUDE_EDGE_TYPES: &[EdgeType] = &[
    // Bonsai
    EdgeType::BookmarkToBonsaiChangeset,
    EdgeType::BonsaiChangesetToFileContent,
    EdgeType::BonsaiChangesetToBonsaiParent,
    EdgeType::BonsaiChangesetToBonsaiHgMapping,
    EdgeType::BonsaiHgMappingToHgChangeset,
    EdgeType::PublishedBookmarksToBonsaiChangeset,
    EdgeType::PublishedBookmarksToBonsaiHgMapping,
    // Hg
    EdgeType::HgBonsaiMappingToBonsaiChangeset,
    EdgeType::HgChangesetToHgParent,
    EdgeType::HgChangesetToHgManifest,
    EdgeType::HgManifestToHgFileEnvelope,
    EdgeType::HgManifestToHgFileNode,
    EdgeType::HgManifestToChildHgManifest,
    EdgeType::HgFileEnvelopeToFileContent,
    EdgeType::HgLinkNodeToHgBonsaiMapping,
    EdgeType::HgLinkNodeToHgChangeset,
    EdgeType::HgFileNodeToHgParentFileNode,
    EdgeType::HgFileNodeToHgCopyfromFileNode,
    // Content
    EdgeType::FileContentToFileContentMetadata,
    EdgeType::FileContentMetadataToSha1Alias,
    EdgeType::FileContentMetadataToSha256Alias,
    EdgeType::FileContentMetadataToGitSha1Alias,
    EdgeType::AliasContentMappingToFileContent,
];

// Does not recurse into history, edges to parents excluded
const SHALLOW_INCLUDE_EDGE_TYPES: &[EdgeType] = &[
    // Bonsai
    EdgeType::BookmarkToBonsaiChangeset,
    EdgeType::BonsaiChangesetToFileContent,
    EdgeType::BonsaiChangesetToBonsaiHgMapping,
    EdgeType::BonsaiHgMappingToHgChangeset,
    EdgeType::PublishedBookmarksToBonsaiChangeset,
    EdgeType::PublishedBookmarksToBonsaiHgMapping,
    // Hg
    EdgeType::HgBonsaiMappingToBonsaiChangeset,
    EdgeType::HgChangesetToHgManifest,
    EdgeType::HgManifestToHgFileEnvelope,
    EdgeType::HgManifestToHgFileNode,
    EdgeType::HgManifestToChildHgManifest,
    EdgeType::HgFileEnvelopeToFileContent,
    // Content
    EdgeType::FileContentToFileContentMetadata,
    EdgeType::FileContentMetadataToSha1Alias,
    EdgeType::FileContentMetadataToSha256Alias,
    EdgeType::FileContentMetadataToGitSha1Alias,
    EdgeType::AliasContentMappingToFileContent,
];

// Types that can result in loading hg data.  Useful for excludes.
const HG_EDGE_TYPES: &[EdgeType] = &[
    // Bonsai to Hg
    EdgeType::BookmarkToBonsaiHgMapping,
    EdgeType::BonsaiHgMappingToHgChangeset,
    EdgeType::PublishedBookmarksToBonsaiHgMapping,
    // Hg
    EdgeType::HgChangesetToHgParent,
    EdgeType::HgChangesetToHgManifest,
    EdgeType::HgManifestToHgFileEnvelope,
    EdgeType::HgManifestToHgFileNode,
    EdgeType::HgManifestToChildHgManifest,
    EdgeType::HgFileEnvelopeToFileContent,
    EdgeType::HgLinkNodeToHgChangeset,
    EdgeType::HgFileNodeToHgParentFileNode,
    EdgeType::HgFileNodeToHgCopyfromFileNode,
];

// Types that can result in loading bonsai data
const BONSAI_EDGE_TYPES: &[EdgeType] = &[
    // Bonsai
    EdgeType::BookmarkToBonsaiChangeset,
    EdgeType::BonsaiChangesetToFileContent,
    EdgeType::BonsaiChangesetToBonsaiParent,
    EdgeType::PublishedBookmarksToBonsaiChangeset,
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
const MARKER_EDGE_TYPES: &[EdgeType] = &[EdgeType::BonsaiChangesetToBonsaiPhaseMapping];

lazy_static! {
    static ref INCLUDE_CHECK_TYPE_HELP: String = format!(
        "Check types to include, defaults to: {:?}",
        CheckType::ALL_VARIANTS,
    );

    static ref INCLUDE_NODE_TYPE_HELP: String = format!(
        "Graph node types we want to step to in the walk. Defaults to core Mononoke and Hg types: {:?}",
        DEFAULT_INCLUDE_NODE_TYPES
    );

    static ref EXCLUDE_NODE_TYPE_HELP: String = format!(
        "Graph node types to exclude from walk. They are removed from the include node types. Specific any of: {:?}",
        NodeType::ALL_VARIANTS,
    );

    static ref INCLUDE_EDGE_TYPE_HELP: String = format!(
        "Graph edge types to include in the walk. Can pass pre-configured sets via deep, shallow, hg, bonsai, as well as individual types. Defaults to deep: {:?}",
        DEEP_INCLUDE_EDGE_TYPES
    );

    static ref EXCLUDE_EDGE_TYPE_HELP: String = format!(
        "Graph edge types to exclude from walk. Can pass pre-configured sets via deep, shallow, hg, bonsai, as well as individual types. Defaults to deep.  All individual types: {:?}",
        EdgeType::ALL_VARIANTS,
    );
}

pub fn setup_toplevel_app<'a, 'b>(app_name: &str) -> App<'a, 'b> {
    let app_template = args::MononokeApp::new(app_name).with_fb303_args();

    let scrub_objects =
        setup_subcommand_args(SubCommand::with_name(SCRUB).about("scrub, checks data is present by reading it and counting it. Combine with --enable-scrub-blobstore to check across a multiplex"))
        .arg(
            Arg::with_name(LIMIT_DATA_FETCH_ARG)
                .long(LIMIT_DATA_FETCH_ARG)
                .takes_value(false)
                .required(false)
                .help("Limit the amount of data fetched from stores, by not streaming large files to the end."),
        );

    let compression_benefit = setup_subcommand_args(
        SubCommand::with_name(COMPRESSION_BENEFIT).about("estimate compression benefit"),
    )
    .arg(
        Arg::with_name(COMPRESSION_LEVEL_ARG)
            .long(COMPRESSION_LEVEL_ARG)
            .takes_value(true)
            .required(false)
            .help("Zstd compression level to use. 3 is the default"),
    )
    .arg(
        Arg::with_name(SAMPLE_RATE_ARG)
            .long(SAMPLE_RATE_ARG)
            .takes_value(true)
            .required(false)
            .help("How many files to sample. Pass 1 to try all, 120 to do 1 in 120, etc."),
    )
    .arg(
        Arg::with_name(EXCLUDE_SAMPLE_NODE_TYPE_ARG)
            .long(EXCLUDE_SAMPLE_NODE_TYPE_ARG)
            .short("S")
            .takes_value(true)
            .multiple(true)
            .number_of_values(1)
            .required(false)
            .help("Node types to exclude from sampling for size"),
    )
    .arg(
        Arg::with_name(INCLUDE_SAMPLE_NODE_TYPE_ARG)
            .long(INCLUDE_SAMPLE_NODE_TYPE_ARG)
            .short("s")
            .takes_value(true)
            .multiple(true)
            .number_of_values(1)
            .required(false)
            .help("Node types to sample for size"),
    );

    let validate = setup_subcommand_args(
        SubCommand::with_name(VALIDATE).about("estimate compression benefit"),
    )
    .arg(
        Arg::with_name(EXCLUDE_CHECK_TYPE_ARG)
            .long(EXCLUDE_CHECK_TYPE_ARG)
            .short("C")
            .takes_value(true)
            .multiple(true)
            .number_of_values(1)
            .required(false)
            .help("Checks to exclude"),
    )
    .arg(
        Arg::with_name(INCLUDE_CHECK_TYPE_ARG)
            .long(INCLUDE_CHECK_TYPE_ARG)
            .short("c")
            .takes_value(true)
            .multiple(true)
            .number_of_values(1)
            .required(false)
            .help(&INCLUDE_CHECK_TYPE_HELP),
    );

    app_template.build()
        .version("0.0.0")
        .about("Walks the mononoke commit and/or derived data graphs, with option of performing validations and modifications")
        .arg(
            Arg::with_name(STORAGE_ID_ARG)
                .long(STORAGE_ID_ARG)
                .takes_value(true)
                .required(false)
                .help("id of storage group to operate over, e.g. manifold_xdb_multiplex"),
        )
        .subcommand(compression_benefit)
        .subcommand(scrub_objects)
        .subcommand(validate)
}

// Add the args the "start from repo" walk types need
fn setup_subcommand_args<'a, 'b>(subcmd: App<'a, 'b>) -> App<'a, 'b> {
    return subcmd
        .arg(
            Arg::with_name(QUIET_ARG)
                .long(QUIET_ARG)
                .short("q")
                .takes_value(false)
                .required(false)
                .help("Log a lot less"),
        )
        .arg(
            Arg::with_name(ENABLE_REDACTION_ARG)
                .long(ENABLE_REDACTION_ARG)
                .takes_value(false)
                .required(false)
                .help("Use redaction from config. Default is redaction off."),
        )
        .arg(
            Arg::with_name(SCHEDULED_MAX_ARG)
                .long(SCHEDULED_MAX_ARG)
                .takes_value(true)
                .required(false)
                .help("Maximum number of walk step tasks to attempt to execute at once.  Default 4096."),
        )
        .arg(
            Arg::with_name(TAIL_INTERVAL_ARG)
                .long(TAIL_INTERVAL_ARG)
                .short("f")
                .takes_value(true)
                .required(false)
                .help("Tail by polling the entry points at interval of TAIL seconds"),
        )
        .arg(
            Arg::with_name(PROGRESS_INTERVAL_ARG)
                .long(PROGRESS_INTERVAL_ARG)
                .takes_value(true)
                .required(false)
                .help("Minimum interval between progress reports in seconds."),
        )
        .arg(
            Arg::with_name(PROGRESS_SAMPLE_RATE_ARG)
                .long(PROGRESS_SAMPLE_RATE_ARG)
                .takes_value(true)
                .required(false)
                .help("Sample the walk output stream for progress roughly 1 in N steps. Only log if progress-interval has passed."),
        )
        .arg(
            Arg::with_name(ENABLE_DERIVE_ARG)
                .long(ENABLE_DERIVE_ARG)
                .takes_value(false)
                .required(false)
                .help("Enable derivation of data (e.g. hg, file metadata). Default is false"),
        )
        .arg(
            Arg::with_name(SCRUB_BLOBSTORE_ACTION_ARG)
                .long(SCRUB_BLOBSTORE_ACTION_ARG)
                .takes_value(true)
                .required(false)
                .help("Enable ScrubBlobstore with the given action. Checks for keys missing from stores. In ReportOnly mode this logs only, otherwise it performs a copy to the missing stores."),
        )
        .arg(
            Arg::with_name(EXCLUDE_NODE_TYPE_ARG)
                .long(EXCLUDE_NODE_TYPE_ARG)
                .short("x")
                .takes_value(true)
                .multiple(true)
                .number_of_values(1)
                .required(false)
                .help(&EXCLUDE_NODE_TYPE_HELP),
        )
        .arg(
            Arg::with_name(INCLUDE_NODE_TYPE_ARG)
                .long(INCLUDE_NODE_TYPE_ARG)
                .short("i")
                .takes_value(true)
                .multiple(true)
                .number_of_values(1)
                .required(false)
                .help(&INCLUDE_NODE_TYPE_HELP),
        )
        .arg(
            Arg::with_name(EXCLUDE_EDGE_TYPE_ARG)
                .long(EXCLUDE_EDGE_TYPE_ARG)
                .short("X")
                .takes_value(true)
                .multiple(true)
                .number_of_values(1)
                .required(false)
                .help(&EXCLUDE_EDGE_TYPE_HELP),
        )
        .arg(
            Arg::with_name(INCLUDE_EDGE_TYPE_ARG)
                .long(INCLUDE_EDGE_TYPE_ARG)
                .short("I")
                .takes_value(true)
                .multiple(true)
                .number_of_values(1)
                .required(false)
                .help(&INCLUDE_EDGE_TYPE_HELP),
        )
        .arg(
            Arg::with_name(BOOKMARK_ARG)
                .long(BOOKMARK_ARG)
                .short("b")
                .takes_value(true)
                .required(false)
                .multiple(true)
                .number_of_values(1)
                .help("Bookmark(s) to start traversal from"),
        )
        .arg(
            Arg::with_name(WALK_ROOT_ARG)
                .long(WALK_ROOT_ARG)
                .short("r")
                .takes_value(true)
                .required(false)
                .multiple(true)
                .number_of_values(1)
                .help("Root(s) to start traversal from in format <NodeType>:<node_key>, e.g. Bookmark:master or HgChangeset:7712b62acdc858689504945ac8965a303ded6626"),
        )
        .arg(
            Arg::with_name(ERROR_AS_DATA_NODE_TYPE_ARG)
                .long(ERROR_AS_DATA_NODE_TYPE_ARG)
                .short("e")
                .takes_value(true)
                .multiple(true)
                .number_of_values(1)
                .required(false)
                .help("Use this to continue walking even walker found an error.  Types of nodes to allow the walker to convert an ErrorKind::NotTraversable to a NodeData::ErrorAsData(NotTraversable)"),
        )
        .arg(
            Arg::with_name(ERROR_AS_DATA_EDGE_TYPE_ARG)
                .long(ERROR_AS_DATA_EDGE_TYPE_ARG)
                .short("E")
                .takes_value(true)
                .multiple(true)
                .number_of_values(1)
                .required(false)
                .help("Types of edges to allow the walker to convert an ErrorKind::NotTraversable to a NodeData::ErrorAsData(NotTraversable). If empty then allow all edges for the nodes specified via error-as-data-node-type"),
        )
        .arg(
            Arg::with_name(INNER_BLOBSTORE_ID_ARG)
                .long(INNER_BLOBSTORE_ID_ARG)
                .takes_value(true)
                .required(false)
                .help("If main blobstore in the storage config is a multiplexed one, use inner blobstore with this id")
        )
        .arg(
            Arg::with_name(SCUBA_TABLE_ARG)
                .long(SCUBA_TABLE_ARG)
                .takes_value(true)
                .multiple(false)
                .required(false)
                .help("Scuba table for logging nodes with issues. e.g. mononoke_walker"),
        )
        .arg(
            Arg::with_name(SCUBA_LOG_FILE_ARG)
                .long(SCUBA_LOG_FILE_ARG)
                .takes_value(true)
                .multiple(false)
                .required(false)
                .help("A log file to write Scuba logs to (primarily useful in testing)"),
        );
}

pub fn parse_node_types(
    sub_m: &ArgMatches<'_>,
    include_arg_name: &str,
    exclude_arg_name: &str,
    default: &[NodeType],
) -> Result<HashSet<NodeType>, Error> {
    let mut include_node_types: HashSet<NodeType> = match sub_m.values_of(include_arg_name) {
        None => Ok(HashSet::from_iter(default.iter().cloned())),
        Some(values) => values.map(NodeType::from_str).collect(),
    }?;
    let exclude_node_types: HashSet<NodeType> = match sub_m.values_of(exclude_arg_name) {
        None => Ok(HashSet::new()),
        Some(values) => values.map(NodeType::from_str).collect(),
    }?;
    include_node_types.retain(|x| !exclude_node_types.contains(x));
    Ok(include_node_types)
}

// parse the pre-defined groups we have for deep, shallow, hg, bonsai etc.
fn parse_edge_value(arg: &str) -> Result<HashSet<EdgeType>, Error> {
    match arg {
        BONSAI_VALUE_ARG => Ok(HashSet::from_iter(BONSAI_EDGE_TYPES.iter().cloned())),
        CONTENT_META_VALUE_ARG => Ok(HashSet::from_iter(CONTENT_META_EDGE_TYPES.iter().cloned())),
        DEEP_VALUE_ARG => Ok(HashSet::from_iter(DEEP_INCLUDE_EDGE_TYPES.iter().cloned())),
        MARKER_VALUE_ARG => Ok(HashSet::from_iter(MARKER_EDGE_TYPES.iter().cloned())),
        HG_VALUE_ARG => Ok(HashSet::from_iter(HG_EDGE_TYPES.iter().cloned())),
        SHALLOW_VALUE_ARG => Ok(HashSet::from_iter(
            SHALLOW_INCLUDE_EDGE_TYPES.iter().cloned(),
        )),
        _ => EdgeType::from_str(arg).map(|e| {
            let mut h = HashSet::new();
            h.insert(e);
            h
        }),
    }
}

fn parse_edge_values(
    values: Option<Values>,
    default: &[EdgeType],
) -> Result<HashSet<EdgeType>, Error> {
    match values {
        None => Ok(HashSet::from_iter(default.iter().cloned())),
        Some(values) => values
            .map(parse_edge_value)
            .collect::<Result<Vec<HashSet<EdgeType>>, Error>>()
            .map(|m| m.into_iter().flatten().collect::<HashSet<EdgeType>>()),
    }
}

fn parse_edge_types(
    sub_m: &ArgMatches<'_>,
    include_arg_name: &str,
    exclude_arg_name: &str,
    default: &[EdgeType],
) -> Result<HashSet<EdgeType>, Error> {
    let mut include_edge_types = parse_edge_values(sub_m.values_of(include_arg_name), default)?;
    let exclude_edge_types = parse_edge_values(sub_m.values_of(exclude_arg_name), &vec![])?;
    include_edge_types.retain(|x| !exclude_edge_types.contains(x));
    Ok(include_edge_types)
}

fn reachable_graph_elements(
    mut include_edge_types: HashSet<EdgeType>,
    mut include_node_types: HashSet<NodeType>,
    root_node_types: HashSet<NodeType>,
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
                    include_node_types.contains(&t) &&
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

pub fn setup_common(
    walk_stats_key: &'static str,
    fb: FacebookInit,
    logger: &Logger,
    blobstore_sampler: Option<Arc<dyn SamplingHandler>>,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> Result<(RepoWalkDatasources, RepoWalkParams), Error> {
    let (_, config) = args::get_config(fb, &matches)?;
    let quiet = sub_m.is_present(QUIET_ARG);
    let common_config = cmdlib::args::read_common_config(fb, &matches)?;
    let scheduled_max = args::get_usize_opt(&sub_m, SCHEDULED_MAX_ARG).unwrap_or(4096) as usize;
    let inner_blobstore_id = args::get_u64_opt(&sub_m, INNER_BLOBSTORE_ID_ARG);
    let tail_secs = args::get_u64_opt(&sub_m, TAIL_INTERVAL_ARG);
    let progress_interval_secs = args::get_u64_opt(&sub_m, PROGRESS_INTERVAL_ARG);
    let progress_sample_rate = args::get_u64_opt(&sub_m, PROGRESS_SAMPLE_RATE_ARG);

    let enable_derive = sub_m.is_present(ENABLE_DERIVE_ARG);

    let redaction = if sub_m.is_present(ENABLE_REDACTION_ARG) {
        config.redaction
    } else {
        Redaction::Disabled
    };

    let caching = cmdlib::args::init_cachelib(fb, &matches, None);

    let include_edge_types = parse_edge_types(
        sub_m,
        INCLUDE_EDGE_TYPE_ARG,
        EXCLUDE_EDGE_TYPE_ARG,
        DEEP_INCLUDE_EDGE_TYPES,
    )?;

    let include_node_types = parse_node_types(
        sub_m,
        INCLUDE_NODE_TYPE_ARG,
        EXCLUDE_NODE_TYPE_ARG,
        DEFAULT_INCLUDE_NODE_TYPES,
    )?;

    let mut walk_roots: Vec<OutgoingEdge> = vec![];

    if sub_m.is_present(BOOKMARK_ARG) {
        let bookmarks: Result<Vec<BookmarkName>, Error> = match sub_m.values_of(BOOKMARK_ARG) {
            None => Err(format_err!("No bookmark passed to --{}", BOOKMARK_ARG)),
            Some(values) => values.map(|bookmark| BookmarkName::new(bookmark)).collect(),
        };

        let mut bookmarks = bookmarks?
            .into_iter()
            .map(|b| OutgoingEdge::new(EdgeType::RootToBookmark, Node::Bookmark(b)))
            .collect();
        walk_roots.append(&mut bookmarks);
    }

    if sub_m.is_present(WALK_ROOT_ARG) {
        let roots: Vec<_> = match sub_m.values_of(WALK_ROOT_ARG) {
            None => Err(format_err!("No root node passed to --{}", WALK_ROOT_ARG)),
            Some(values) => values.map(|root| parse_node(root)).collect(),
        }?;
        let mut roots = roots
            .into_iter()
            .filter_map(|node| {
                node.get_type()
                    .root_edge_type()
                    .map(|et| OutgoingEdge::new(et, node))
            })
            .collect();
        walk_roots.append(&mut roots);
    }

    if walk_roots.is_empty() {
        return Err(format_err!(
            "No walk roots provided, pass with --{} or --{}",
            BOOKMARK_ARG,
            WALK_ROOT_ARG,
        ));
    }

    info!(logger, "Walking roots {:?} ", walk_roots);

    let root_node_types: HashSet<_> = walk_roots.iter().map(|e| e.label.outgoing_type()).collect();

    let (include_edge_types, include_node_types) =
        reachable_graph_elements(include_edge_types, include_node_types, root_node_types);
    info!(
        logger,
        "Walking edge types {:?}",
        sort_by_string(&include_edge_types)
    );
    info!(
        logger,
        "Walking node types {:?}",
        sort_by_string(&include_node_types)
    );

    let readonly_storage = args::parse_readonly_storage(&matches);

    let error_as_data_node_types = parse_node_types(
        sub_m,
        ERROR_AS_DATA_NODE_TYPE_ARG,
        EXCLUDE_NODE_TYPE_ARG,
        &[],
    )?;
    let error_as_data_edge_types = parse_edge_types(
        sub_m,
        ERROR_AS_DATA_EDGE_TYPE_ARG,
        EXCLUDE_EDGE_TYPE_ARG,
        &[],
    )?;
    if !error_as_data_node_types.is_empty() || !error_as_data_edge_types.is_empty() {
        if !readonly_storage.0 {
            Err(format_err!(
                "Error as data could mean internal state is invalid, run with --readonly-storage to ensure no risk of persisting it"
            ))?
        }
        warn!(
            logger,
            "Error as data enabled, walk results may not be complete. Errors as data enabled for node types {:?} edge types {:?}",
            sort_by_string(&error_as_data_node_types),
            sort_by_string(&error_as_data_edge_types)
        );
    }

    let mysql_options = args::parse_mysql_options(&matches);

    let storage_id = matches.value_of(STORAGE_ID_ARG);
    let storage_config = match storage_id {
        Some(storage_id) => {
            let mut configs = args::read_storage_configs(fb, &matches)?;
            configs.remove(storage_id).ok_or(format_err!(
                "Storage id `{}` not found in {:?}",
                storage_id,
                configs.keys()
            ))?
        }
        None => config.storage_config.clone(),
    };

    let blobstore_options = args::parse_blobstore_options(&matches);

    let scuba_table = sub_m.value_of(SCUBA_TABLE_ARG).map(|a| a.to_string());
    let repo_name = args::get_repo_name(fb, &matches)?;
    let mut scuba_builder = ScubaSampleBuilder::with_opt_table(fb, scuba_table.clone());
    scuba_builder.add_common_server_data();
    scuba_builder.add(WALK_TYPE, walk_stats_key);
    scuba_builder.add(REPO, repo_name.clone());

    if let Some(scuba_log_file) = sub_m.value_of(SCUBA_LOG_FILE_ARG) {
        scuba_builder = scuba_builder.with_log_file(scuba_log_file)?;
    }

    let scrub_action = sub_m
        .value_of(SCRUB_BLOBSTORE_ACTION_ARG)
        .map(ScrubAction::from_str)
        .transpose()?;

    // Open the blobstore explicitly so we can do things like run on one side of a multiplex
    let blobstore = blobstore::open_blobstore(
        fb,
        mysql_options,
        storage_config.blobstore,
        inner_blobstore_id,
        None,
        readonly_storage,
        scrub_action,
        blobstore_sampler,
        scuba_builder.clone(),
        walk_stats_key,
        repo_name.clone(),
        blobstore_options,
        logger.clone(),
    )
    .boxed()
    .compat()
    .boxify();

    let sql_factory = make_metadata_sql_factory(
        fb,
        storage_config.metadata,
        mysql_options,
        readonly_storage,
        logger.clone(),
    )
    .boxify();

    let blobrepo = open_blobrepo_given_datasources(
        fb,
        blobstore,
        sql_factory,
        config.repoid,
        caching,
        config.bookmarks_cache_ttl,
        redaction,
        common_config.scuba_censored_table,
        config.filestore,
        readonly_storage,
        config.derived_data_config,
        repo_name.clone(),
    )
    .compat()
    .boxed();

    let mut progress_node_types = include_node_types.clone();
    for e in &walk_roots {
        progress_node_types.insert(e.target.get_type());
    }

    let progress_state = ProgressStateMutex::new(ProgressStateCountByType::new(
        logger.clone(),
        walk_stats_key,
        repo_name,
        progress_node_types,
        progress_sample_rate.unwrap_or(PROGRESS_SAMPLE_RATE),
        Duration::from_secs(progress_interval_secs.unwrap_or(PROGRESS_SAMPLE_DURATION_S)),
    ));

    Ok((
        RepoWalkDatasources {
            blobrepo,
            scuba_builder,
        },
        RepoWalkParams {
            enable_derive,
            scheduled_max,
            walk_roots,
            include_node_types,
            include_edge_types,
            tail_secs,
            quiet,
            progress_state,
            error_as_data_node_types,
            error_as_data_edge_types,
        },
    ))
}
