/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(never_type)]
#![deny(warnings)]

use anyhow::{format_err, Error, Result};
use bookmarks::BookmarkName;
use clap::{Arg, ArgMatches};
use cloned::cloned;
use cmdlib::{args, helpers::block_execute};
use fbinit::FacebookInit;
use futures::{
    compat::{Future01CompatExt, Stream01CompatExt},
    future,
    stream::StreamExt,
};
use futures_ext::StreamExt as StreamExt2;
use futures_old::stream::Stream;
use mercurial_types::HgChangesetId;
use metaconfig_types::RepoConfig;
use scuba_ext::ScubaSampleBuilder;
use sql_construct::{facebook::FbSqlConstruct, SqlConstruct};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::delay_for;

mod errors;
mod replay_stream;
mod sql_replay_bookmarks_queue;
mod sync_bookmark;

use replay_stream::BufferSize;
use sql_replay_bookmarks_queue::{Backfill, QueueLimit, SqlReplayBookmarksQueue};

const DEFAULT_BUFFER_SIZE: usize = 50;
const DEFAULT_QUEUE_LIMIT: usize = 5000;

const ARG_BUFFER_SIZE: &'static str = "buffer-size";
const ARG_MAX_ITERATIONS: &'static str = "max-iterations";
const ARG_CTX_SCUBA_TABLE: &'static str = "log-scuba-table";
const ARG_STATUS_SCUBA_TABLE: &'static str = "status-scuba-table";
const ARG_QUEUE_LIMIT: &'static str = "queue-limit";
const ARG_SQL_QUEUE_SOURCE: &'static str = "sql-queue-type";
const ARG_SQL_QUEUE_NAME: &'static str = "sql-name";
const ARG_DELAY: &'static str = "delay";
const ARG_BACKFILL: &'static str = "backfill";

const SOURCE_SQLITE: &'static str = "sqlite";
const SOURCE_XDB: &'static str = "xdb";

// NOTE: We have to use our own implementation of open_sql here (as opposed to the ones used in the
// rest of Mononoke), because we're getting our XDB tier from args, not from the Mononoke config.
// The reason for that is the XDB DB for the queue lives in a Mercurial XDB, not in the Mononoke
// XBD.
async fn open_sql(
    fb: FacebookInit,
    matches: &ArgMatches<'_>,
    readonly_storage: bool,
) -> Result<SqlReplayBookmarksQueue, Error> {
    let mysql_options = args::parse_mysql_options(matches);

    // NOTE: We make this required in our args, hence unwrap.
    let name = matches.value_of(ARG_SQL_QUEUE_NAME).unwrap();

    match matches.value_of(ARG_SQL_QUEUE_SOURCE) {
        Some(SOURCE_SQLITE) => {
            let mut path = PathBuf::new();
            path.push(name);
            SqlReplayBookmarksQueue::with_sqlite_path(
                path.join(SqlReplayBookmarksQueue::LABEL),
                readonly_storage,
            )
        }
        Some(SOURCE_XDB) => {
            SqlReplayBookmarksQueue::with_xdb(fb, name.to_string(), mysql_options, readonly_storage)
                .await
        }
        // NOTE: We make this required and restrict valid values in our args, hence the panic.
        x => panic!("Invalid {}: {:?}", ARG_SQL_QUEUE_SOURCE, x),
    }
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let app = args::MononokeApp::new("Replay bookmarks from Mercurial into Mononoke")
        .with_advanced_args_hidden()
        .with_fb303_args()
        .build()
        .arg(
            Arg::with_name(ARG_CTX_SCUBA_TABLE)
                .long(ARG_CTX_SCUBA_TABLE)
                .takes_value(true)
                .required(false)
                .help("DEPRECATED - Scuba table to route CoreContext to."),
        )
        .arg(
            Arg::with_name(ARG_STATUS_SCUBA_TABLE)
                .long(ARG_STATUS_SCUBA_TABLE)
                .takes_value(true)
                .required(false)
                .help("Scuba table to route sync outcomes to."),
        )
        .arg(
            Arg::with_name(ARG_MAX_ITERATIONS)
                .long(ARG_MAX_ITERATIONS)
                .takes_value(true)
                .required(false)
                .help("Max number of iterations to perform."),
        )
        .arg(
            Arg::with_name(ARG_BUFFER_SIZE)
                .long(ARG_BUFFER_SIZE)
                .takes_value(true)
                .required(false)
                .help("Count of bookmarks to replay concurrently"),
        )
        .arg(
            Arg::with_name(ARG_QUEUE_LIMIT)
                .long(ARG_QUEUE_LIMIT)
                .takes_value(true)
                .required(false)
                .help("Limit the number of rows to fetch from the queue"),
        )
        .arg(
            Arg::with_name(ARG_DELAY)
                .long(ARG_DELAY)
                .takes_value(true)
                .required(false)
                .help("How long to sleep after processing a batch"),
        )
        .arg(
            Arg::with_name(ARG_BACKFILL)
                .long(ARG_BACKFILL)
                .takes_value(false)
                .required(false)
                .help("Whether to look for backfill = 1 entries"),
        )
        .arg(
            Arg::with_name(ARG_SQL_QUEUE_SOURCE)
                .takes_value(true)
                .required(true)
                .possible_values(&[SOURCE_SQLITE, SOURCE_XDB])
                .help("What engine to use for SQL"),
        )
        .arg(
            Arg::with_name(ARG_SQL_QUEUE_NAME)
                .takes_value(true)
                .required(true)
                .help("Where is the SQL DB (directory for SQLite, XDB tier for XDB)"),
        );

    let matches = app.get_matches();
    let readonly_storage = args::parse_readonly_storage(&matches);
    args::init_cachelib(fb, &matches, None);

    let logger = args::init_logging(fb, &matches);
    let queue = open_sql(fb, &matches, readonly_storage.0);
    let (repo_name, RepoConfig { infinitepush, .. }) = args::get_config(fb, &matches)?;
    let blobrepo = args::open_repo(fb, &logger, &matches);

    let backfill = Backfill(matches.is_present(ARG_BACKFILL));
    let buffer_size = BufferSize(args::get_usize(
        &matches,
        ARG_BUFFER_SIZE,
        DEFAULT_BUFFER_SIZE,
    ));
    let queue_limit = QueueLimit(args::get_usize(
        &matches,
        ARG_QUEUE_LIMIT,
        DEFAULT_QUEUE_LIMIT,
    ));
    let maybe_max_iterations = args::get_u64_opt(&matches, ARG_MAX_ITERATIONS);
    let maybe_delay = args::get_u64_opt(&matches, ARG_DELAY);

    let mut status_scuba = match matches.value_of(ARG_STATUS_SCUBA_TABLE) {
        Some(table) => ScubaSampleBuilder::new(fb, table),
        None => ScubaSampleBuilder::with_discard(),
    };

    status_scuba
        .add_common_server_data()
        .add("reponame", repo_name.as_ref());

    let infinitepush_namespace = infinitepush.namespace.ok_or(format_err!(
        "Infinitepush is not enabled in repository {:?}",
        repo_name
    ))?;

    let main = async {
        let (queue, blobrepo) = future::try_join(queue, blobrepo.compat()).await?;

        let infinitepush_namespace = Arc::new(infinitepush_namespace);
        let do_replay = {
            cloned!(logger);
            move |name: &BookmarkName, hg_cs_id: &HgChangesetId| {
                sync_bookmark::sync_bookmark(
                    fb,
                    blobrepo.clone(),
                    logger.clone(),
                    infinitepush_namespace.clone(),
                    name,
                    hg_cs_id,
                )
            }
        };

        let stream = replay_stream::process_replay_stream(
            queue,
            repo_name,
            backfill,
            buffer_size,
            queue_limit,
            status_scuba,
            logger.clone(),
            do_replay,
        );

        let mut stream = match maybe_max_iterations {
            Some(max_iterations) => stream.take(max_iterations).left_stream(),
            None => stream.right_stream(),
        }
        .compat();

        while let Some(_) = stream.next().await {
            if let Some(delay) = maybe_delay {
                delay_for(Duration::new(delay, 0)).await;
            }
        }

        Ok(())
    };

    block_execute(
        main,
        fb,
        "commitcloud_bookmarks_filler",
        &logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}
