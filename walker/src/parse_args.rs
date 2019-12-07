/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::blobstore;
use crate::graph::{Node, NodeType};

use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use blobrepo_factory::open_blobrepo_given_datasources;
use bookmarks::BookmarkName;
use clap::{App, Arg, ArgMatches, SubCommand};
use cmdlib::args;
use fbinit::FacebookInit;
use futures_ext::{BoxFuture, FutureExt};
use metaconfig_types::Redaction;
use slog::{info, Logger};
use std::{collections::HashSet, iter::FromIterator, str::FromStr};

#[derive(Clone)]
pub struct RepoWalkParams {
    pub scheduled_max: usize,
    pub walk_roots: Vec<Node>,
    pub include_types: HashSet<NodeType>,
    pub tail_secs: Option<u64>,
}

// Sub commands
pub const COUNT_OBJECTS: &'static str = "count-objects";
pub const SCRUB_OBJECTS: &'static str = "scrub-objects";

// Subcommand args
const ENABLE_REDACTION_ARG: &'static str = "enable-redaction";
const SCHEDULED_MAX_ARG: &'static str = "scheduled-max";
const TAIL_INTERVAL_ARG: &'static str = "tail-interval";
const EXCLUDE_TYPE_ARG: &'static str = "exclude-type";
const INCLUDE_TYPE_ARG: &'static str = "include-type";
const BOOKMARK_ARG: &'static str = "bookmark";
const INNER_BLOBSTORE_ID_ARG: &'static str = "inner-blobstore-id";

// Toplevel args - healer and populate healer have this one at top level
// so keeping it there for consistency
const STORAGE_ID_ARG: &'static str = "storage-id";

const DEFAULT_INCLUDE_TYPES: &[NodeType] = &[
    NodeType::Bookmark,
    NodeType::BonsaiChangeset,
    NodeType::BonsaiChangesetFromHgChangeset,
    NodeType::HgChangesetFromBonsaiChangeset,
    NodeType::BonsaiParents,
    NodeType::HgChangeset,
    NodeType::HgManifest,
    NodeType::HgFileEnvelope,
    NodeType::HgFileNode,
    NodeType::FileContent,
    NodeType::FileContentMetadata,
];

pub fn setup_toplevel_app<'a, 'b>(app_name: &str) -> App<'a, 'b> {
    let app_template = args::MononokeApp::new(app_name);

    let count_objects =
        setup_subcommand_args(SubCommand::with_name(COUNT_OBJECTS).about("count objects"));

    let scrub_objects =
        setup_subcommand_args(SubCommand::with_name(SCRUB_OBJECTS).about("scrub objects"));

    let app = app_template.build()
        .version("0.0.0")
        .about("Walks the mononoke commit and/or derived data graphs, with option of performing validations and modifications")
        .arg(
            Arg::with_name(STORAGE_ID_ARG)
                .long(STORAGE_ID_ARG)
                .takes_value(true)
                .required(false)
                .help("id of storage group to operate over, e.g. manifold_xdb_multiplex"),
        )
        .subcommand(count_objects)
        .subcommand(scrub_objects);
    let app = args::add_fb303_args(app);
    app
}

// Add the args the "start from repo" walk types need
fn setup_subcommand_args<'a, 'b>(subcmd: App<'a, 'b>) -> App<'a, 'b> {
    return subcmd
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
            Arg::with_name(EXCLUDE_TYPE_ARG)
                .long(EXCLUDE_TYPE_ARG)
                .short("x")
                .takes_value(true)
                .multiple(true)
                .number_of_values(1)
                .required(false)
                .help("Graph node types to exclude from the walk. These are removed from the include-type list"),
        )
        .arg(
            Arg::with_name(INCLUDE_TYPE_ARG)
                .long(INCLUDE_TYPE_ARG)
                .short("t")
                .takes_value(true)
                .multiple(true)
                .number_of_values(1)
                .required(false)
                .help("Graph node types to include in the walk. Defaults to core Mononoke and Hg types."),
        )
        .arg(
            Arg::with_name(BOOKMARK_ARG)
                .long(BOOKMARK_ARG)
                .short("b")
                .takes_value(true)
                .required(true)
                .multiple(true)
                .number_of_values(1)
                .help("Bookmark(s) to start traversal from"),
        )
        .arg(
            Arg::with_name(INNER_BLOBSTORE_ID_ARG)
                .long(INNER_BLOBSTORE_ID_ARG)
                .takes_value(true)
                .required(false)
                .help("If main blobstore in the storage config is a multiplexed one, use inner blobstore with this id")
        )
        ;
}

pub fn parse_args_common(
    fb: FacebookInit,
    logger: &Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> Result<(BoxFuture<BlobRepo, Error>, RepoWalkParams), Error> {
    let (_, config) = args::get_config(&matches)?;
    let common_config = cmdlib::args::read_common_config(&matches)?;

    let scheduled_max = args::get_usize_opt(&sub_m, SCHEDULED_MAX_ARG).unwrap_or(4096) as usize;
    let inner_blobstore_id = args::get_u64_opt(&sub_m, INNER_BLOBSTORE_ID_ARG);
    let tail_secs = args::get_u64_opt(&sub_m, TAIL_INTERVAL_ARG);

    let redaction = if sub_m.is_present(ENABLE_REDACTION_ARG) {
        config.redaction
    } else {
        Redaction::Disabled
    };

    let caching = cmdlib::args::init_cachelib(fb, &matches);

    // TODO, add some error text showing valid types on parse error
    let include_types: Result<HashSet<NodeType>, Error> = match sub_m.values_of(INCLUDE_TYPE_ARG) {
        None => Ok(HashSet::from_iter(DEFAULT_INCLUDE_TYPES.iter().cloned())),
        Some(values) => values.map(NodeType::from_str).collect(),
    };
    let mut include_types = include_types?;

    let exclude_types: Result<HashSet<NodeType>, Error> = match sub_m.values_of(EXCLUDE_TYPE_ARG) {
        None => Ok(HashSet::new()),
        Some(values) => values.map(NodeType::from_str).collect(),
    };
    let exclude_types = exclude_types?;

    info!(logger, "Excluding types {:?}", exclude_types);
    include_types.retain(|x| !exclude_types.contains(x));

    let bookmarks: Result<Vec<_>, Error> = match sub_m.values_of(BOOKMARK_ARG) {
        None => Err(format_err!("No bookmark passed")),
        Some(values) => values.map(|bookmark| BookmarkName::new(bookmark)).collect(),
    };
    let bookmarks = bookmarks?;

    // TODO, add other root types like hg change ids etc.
    let walk_roots: Vec<_> = bookmarks.into_iter().map(Node::Bookmark).collect();

    info!(logger, "Walking roots {:?} ", walk_roots);

    info!(logger, "Walking types {:?}", include_types);

    let myrouter_port = args::parse_myrouter_port(&matches);
    let readonly_storage = args::parse_readonly_storage(&matches);
    let storage_id = matches.value_of(STORAGE_ID_ARG);
    let storage_config = match storage_id {
        Some(storage_id) => args::read_storage_configs(&matches)?
            .remove(storage_id)
            .ok_or(format_err!("Storage id `{}` not found", storage_id))?,
        None => config.storage_config.clone(),
    };

    // Open the blobstore explicitly so we can do things like run on one side of a multiplex
    let datasources_fut = blobstore::open_blobstore(
        fb,
        myrouter_port,
        storage_config,
        inner_blobstore_id,
        None,
        readonly_storage,
        logger.clone(),
    );

    let blobrepo_fut = open_blobrepo_given_datasources(
        fb,
        datasources_fut,
        config.repoid,
        caching,
        config.bookmarks_cache_ttl,
        redaction,
        common_config.scuba_censored_table,
        config.filestore,
        readonly_storage,
    )
    .boxify();

    Ok((
        blobrepo_fut,
        RepoWalkParams {
            scheduled_max,
            walk_roots,
            include_types,
            tail_secs,
        },
    ))
}
