// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{sync::Arc, time::Instant};

use clap::Arg;
use cloned::cloned;
use failure::{err_msg, format_err, Error};
use futures::{future, stream::Stream, Future, IntoFuture};
use futures_ext::FutureExt;
use serde_derive::{Deserialize, Serialize};
use serde_json;
use tokio::runtime;

use blobstore::Blobstore;
use blobstore_sync_queue::{BlobstoreSyncQueue, BlobstoreSyncQueueEntry, SqlBlobstoreSyncQueue};
use cmdlib::args;
use context::CoreContext;
use manifoldblob::{ManifoldRange, ThriftManifoldBlob};
use metaconfig_types::{BlobConfig, BlobstoreId, MetadataDBConfig, StorageConfig};
use mononoke_types::{BlobstoreBytes, DateTime, RepositoryId};
use sql_ext::SqlConstructors;

/// Save manifold continuation token each once per `PRESERVE_STATE_RATIO` entries
const PRESERVE_STATE_RATIO: usize = 10_000;
/// PRESERVE_STATE_RATIO should be divisible by CHUNK_SIZE as otherwise progress
/// reporting will be broken
const CHUNK_SIZE: usize = 1000;
const INIT_COUNT_VALUE: usize = 0;

#[derive(Debug)]
struct ManifoldArgs {
    bucket: String,
    prefix: String,
}

/// Configuration options
#[derive(Debug)]
struct Config {
    db_address: String,
    myrouter_port: u16,
    manifold_args: ManifoldArgs,
    repo_id: RepositoryId,
    src_blobstore_id: BlobstoreId,
    dst_blobstore_id: BlobstoreId,
    start_key: Option<String>,
    end_key: Option<String>,
    ctx: CoreContext,
    state_key: Option<String>,
    dry_run: bool,
    started_at: Instant,
}

/// State used to resume iteration in case of restart
#[derive(Debug, Clone)]
struct State {
    count: usize,
    init_range: Arc<ManifoldRange>,
    current_range: Arc<ManifoldRange>,
}

impl State {
    fn from_init(init_range: Arc<ManifoldRange>) -> Self {
        Self {
            count: INIT_COUNT_VALUE,
            current_range: init_range.clone(),
            init_range,
        }
    }

    fn with_current_many(self, current_range: Arc<ManifoldRange>, num: usize) -> Self {
        let State {
            count, init_range, ..
        } = self;
        Self {
            count: count + num,
            init_range,
            current_range,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct StateSerde {
    init_range: ManifoldRange,
    current_range: ManifoldRange,
}

impl From<StateSerde> for State {
    fn from(state: StateSerde) -> Self {
        Self {
            count: INIT_COUNT_VALUE,
            init_range: Arc::new(state.init_range),
            current_range: Arc::new(state.current_range),
        }
    }
}

impl<'a> From<&'a State> for StateSerde {
    fn from(state: &'a State) -> Self {
        Self {
            init_range: (*state.init_range).clone(),
            current_range: (*state.current_range).clone(),
        }
    }
}

fn parse_args() -> Result<Config, Error> {
    let app = args::MononokeApp {
        safe_writes: true,
        hide_advanced_args: false,
        default_glog: true,
    }
    .build("populate healer queue")
    .version("0.0.0")
    .about("Populate blobstore queue from existing manifold bucket")
    .arg(
        Arg::with_name("source-blobstore-id")
            .long("source-blobstore-id")
            .short("s")
            .takes_value(true)
            .value_name("SOURCE")
            .help("source blobstore identifier"),
    )
    .arg(
        Arg::with_name("destination-blobstore-id")
            .long("destination-blobstore-id")
            .short("d")
            .takes_value(true)
            .value_name("DESTINATION")
            .help("destination blobstore identifier"),
    )
    .arg(
        Arg::with_name("start-key")
            .long("start-key")
            .takes_value(true)
            .value_name("START_KEY")
            .help("if specified iteration will start from this key"),
    )
    .arg(
        Arg::with_name("end-key")
            .long("end-key")
            .takes_value(true)
            .value_name("END_KEY")
            .help("if specified iteration will end at this key"),
    )
    .arg(
        Arg::with_name("resume-state-key")
            .long("resume-state-key")
            .takes_value(true)
            .value_name("STATE_MANIFOLD_KEY")
            .help("manifold key which contains current iteration state and can be used to resume"),
    )
    .arg(
        Arg::with_name("dry-run")
            .long("dry-run")
            .help("do not add entries to a queue"),
    );

    let matches = app.get_matches();
    let repo_id = args::get_repo_id(&matches)?;

    let repo_config = args::read_configs(&matches)?
        .repos
        .into_iter()
        .filter(|(_, config)| RepositoryId::new(config.repoid) == repo_id)
        .map(|(_, config)| config)
        .next()
        .ok_or(format_err!(
            "failed to find config with repo id: {:?}",
            repo_id
        ))?;

    let src_blobstore_id = matches
        .value_of("source-blobstore-id")
        .ok_or(err_msg("`source-blobstore-id` argument is required"))
        .and_then(|src| src.parse::<u64>().map_err(Error::from))
        .map(BlobstoreId::new)?;
    let dst_blobstore_id = matches
        .value_of("destination-blobstore-id")
        .ok_or(err_msg("`destination-blobstore-id` argument is required"))
        .and_then(|dst| dst.parse::<u64>().map_err(Error::from))
        .map(BlobstoreId::new)?;
    if src_blobstore_id == dst_blobstore_id {
        return Err(err_msg(
            "`source-blobstore-id` and `destination-blobstore-id` can not be equal",
        ));
    }

    let (blobstores, db_address) = match &repo_config.storage_config {
        StorageConfig {
            dbconfig: MetadataDBConfig::Mysql { db_address, .. },
            blobstore: BlobConfig::Multiplexed { blobstores, .. },
        } => (blobstores, db_address),
        storage => return Err(format_err!("unsupported storage: {:?}", storage)),
    };
    let manifold_args = blobstores
        .iter()
        .filter(|(id, _)| src_blobstore_id == *id)
        .map(|(_, args)| args)
        .next()
        .ok_or(format_err!(
            "failed to find source blobstore id: {:?}",
            src_blobstore_id,
        ))
        .and_then(|args| match args {
            BlobConfig::Manifold { bucket, prefix } => Ok(ManifoldArgs {
                bucket: bucket.clone(),
                prefix: prefix.clone(),
            }),
            _ => Err(err_msg("source blobstore must be a manifold")),
        })?;

    let myrouter_port =
        args::parse_myrouter_port(&matches).ok_or(err_msg("myrouter-port must be specified"))?;

    Ok(Config {
        repo_id,
        db_address: db_address.clone(),
        myrouter_port,
        manifold_args,
        src_blobstore_id,
        dst_blobstore_id,
        start_key: matches.value_of("start-key").map(String::from),
        end_key: matches.value_of("end-key").map(String::from),
        state_key: matches.value_of("resume-state-key").map(String::from),
        ctx: args::get_core_context(&matches),
        dry_run: matches.is_present("dry-run"),
        started_at: Instant::now(),
    })
}

fn get_resume_state(
    manifold: &ThriftManifoldBlob,
    config: &Config,
) -> impl Future<Item = State, Error = Error> {
    let resume_state = match &config.state_key {
        Some(state_key) => manifold
            .get(config.ctx.clone(), state_key.clone())
            .map(|data| {
                data.and_then(|data| serde_json::from_slice::<StateSerde>(&*data.into_bytes()).ok())
                    .map(State::from)
            })
            .left_future(),
        None => future::ok(None).right_future(),
    };

    let init_state = {
        let start = format!(
            "flat/repo{:04}.{}",
            config.repo_id.id(),
            config.start_key.clone().unwrap_or_else(|| "".to_string())
        );
        let end = format!(
            "flat/repo{:04}.{}",
            config.repo_id.id(),
            config.end_key.clone().unwrap_or_else(|| "\x7f".to_string()),
        );
        State::from_init(Arc::new(ManifoldRange::from(start..end)))
    };

    resume_state.map(move |resume_state| match resume_state {
        None => init_state,
        // if initial_state mismatch, start from provided initial state
        Some(ref resume_state) if resume_state.init_range != init_state.init_range => init_state,
        Some(resume_state) => resume_state,
    })
}

fn put_resume_state(
    manifold: &ThriftManifoldBlob,
    config: &Config,
    state: State,
) -> impl Future<Item = State, Error = Error> {
    match &config.state_key {
        Some(state_key) if state.count % PRESERVE_STATE_RATIO == INIT_COUNT_VALUE => {
            let started_at = config.started_at;
            let ctx = config.ctx.clone();
            cloned!(state_key, manifold);
            serde_json::to_vec(&StateSerde::from(&state))
                .map(|state_json| BlobstoreBytes::from_bytes(state_json))
                .map_err(Error::from)
                .into_future()
                .and_then(move |state_data| manifold.put(ctx, state_key, state_data))
                .map(move |_| {
                    if termion::is_tty(&std::io::stderr()) {
                        let elapsed = started_at.elapsed().as_secs() as f64;
                        let count = state.count as f64;
                        eprintln!(
                            "Keys processed: {:.0} speed: {:.2}/s",
                            count,
                            count / elapsed
                        );
                    }
                    state
                })
                .left_future()
        }
        _ => future::ok(state).right_future(),
    }
}

fn populate_healer_queue(
    manifold: ThriftManifoldBlob,
    queue: Arc<dyn BlobstoreSyncQueue>,
    config: Arc<Config>,
) -> impl Future<Item = State, Error = Error> {
    get_resume_state(&manifold, &config).and_then(move |state| {
        manifold
            .enumerate((*state.current_range).clone())
            .chunks(CHUNK_SIZE)
            .fold(state, move |state, entries| {
                let range = entries[0].range.clone();
                let state = state.with_current_many(range, entries.len());
                let repo_id = config.repo_id;
                let src_blobstore_id = config.src_blobstore_id;

                let enqueue = if config.dry_run {
                    future::ok(()).left_future()
                } else {
                    let iterator_box = Box::new(entries.into_iter().map(move |entry| {
                        BlobstoreSyncQueueEntry::new(
                            repo_id,
                            entry.key,
                            src_blobstore_id,
                            DateTime::now(),
                        )
                    }));
                    queue
                        .add_many(config.ctx.clone(), iterator_box)
                        .right_future()
                };

                enqueue.and_then({
                    cloned!(manifold, config);
                    move |_| put_resume_state(&manifold, &config, state)
                })
            })
    })
}

fn main() -> Result<(), Error> {
    let config = Arc::new(parse_args()?);
    let manifold = ThriftManifoldBlob::new(config.manifold_args.bucket.clone())?.into_inner();
    let queue: Arc<dyn BlobstoreSyncQueue> = Arc::new(SqlBlobstoreSyncQueue::with_myrouter(
        config.db_address.clone(),
        config.myrouter_port,
    ));
    let mut runtime = runtime::Runtime::new()?;
    runtime.block_on(populate_healer_queue(manifold, queue, config))?;
    Ok(())
}
