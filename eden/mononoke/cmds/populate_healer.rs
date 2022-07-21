/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Instant;

use anyhow::bail;
use anyhow::format_err;
use anyhow::Error;
use clap_old::Arg;
use cloned::cloned;
use fbinit::FacebookInit;
use futures::compat::Future01CompatExt;
use futures::FutureExt;
use futures::TryFutureExt;
use futures_old::Future;
use futures_old::IntoFuture;
use serde_derive::Deserialize;
use serde_derive::Serialize;

use blobstore::Blobstore;
use blobstore::BlobstoreKeyParam;
use blobstore::BlobstoreKeySource;
use blobstore::DEFAULT_PUT_BEHAVIOUR;
use blobstore_sync_queue::BlobstoreSyncQueue;
use blobstore_sync_queue::BlobstoreSyncQueueEntry;
use blobstore_sync_queue::OperationKey;
use blobstore_sync_queue::SqlBlobstoreSyncQueue;
use cmdlib::args;
use context::CoreContext;
use fileblob::Fileblob;
use manifoldblob::ManifoldBlob;
use metaconfig_types::BlobConfig;
use metaconfig_types::BlobstoreId;
use metaconfig_types::MetadataDatabaseConfig;
use metaconfig_types::MultiplexId;
use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use metaconfig_types::StorageConfig;
use mononoke_types::BlobstoreBytes;
use mononoke_types::DateTime;
use mononoke_types::RepositoryId;
use sql_construct::facebook::FbSqlConstruct;
use sql_ext::facebook::MysqlOptions;
use sql_ext::facebook::ReadConnectionType;

/// Save manifold continuation token each once per `PRESERVE_STATE_RATIO` entries
const PRESERVE_STATE_RATIO: usize = 10_000;
/// PRESERVE_STATE_RATIO should be divisible by CHUNK_SIZE as otherwise progress
/// reporting will be broken
const INIT_COUNT_VALUE: usize = 0;

/// Configuration options
struct Config {
    db_address: String,
    mysql_options: MysqlOptions,
    blobstore_args: BlobConfig,
    repo_id: RepositoryId,
    src_blobstore_id: BlobstoreId,
    #[allow(unused)]
    dst_blobstore_id: BlobstoreId,
    multiplex_id: MultiplexId,
    start_key: Option<String>,
    end_key: Option<String>,
    ctx: CoreContext,
    state_key: Option<String>,
    dry_run: bool,
    started_at: Instant,
    readonly_storage: bool,
}

/// State used to resume iteration in case of restart
#[derive(Debug, Clone)]
struct State {
    count: usize,
    init_range: Arc<BlobstoreKeyParam>,
    current_range: Arc<BlobstoreKeyParam>,
}

impl State {
    fn from_init(init_range: Arc<BlobstoreKeyParam>) -> Self {
        Self {
            count: INIT_COUNT_VALUE,
            current_range: init_range.clone(),
            init_range,
        }
    }

    fn with_current_many(self, current_range: Arc<BlobstoreKeyParam>, num: usize) -> Self {
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
    init_range: BlobstoreKeyParam,
    current_range: BlobstoreKeyParam,
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

fn parse_args(fb: FacebookInit) -> Result<Config, Error> {
    let app = args::MononokeAppBuilder::new("populate healer queue")
        .build()
        .about("Populate blobstore queue from existing key source")
        .arg(
            Arg::with_name("storage-id")
                .long("storage-id")
                .short("S")
                .takes_value(true)
                .value_name("STORAGEID")
                .help("Storage identifier"),
        )
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
                .short("D")
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
                .help(
                    "manifold key which contains current iteration state and can be used to resume",
                ),
        )
        .arg(
            Arg::with_name("dry-run")
                .long("dry-run")
                .help("do not add entries to a queue"),
        );

    let matches = app.get_matches(fb)?;
    let logger = matches.logger();
    let config_store = matches.config_store();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let repo_id = args::get_repo_id(config_store, &matches)?;

    let storage_id = matches
        .value_of("storage-id")
        .ok_or_else(|| Error::msg("`storage-id` argument required"))?;

    let storage_config = args::load_storage_configs(config_store, &matches)?
        .storage
        .remove(storage_id)
        .ok_or_else(|| Error::msg("Unknown `storage-id`"))?;

    let src_blobstore_id = matches
        .value_of("source-blobstore-id")
        .ok_or_else(|| Error::msg("`source-blobstore-id` argument is required"))
        .and_then(|src| src.parse::<u64>().map_err(Error::from))
        .map(BlobstoreId::new)?;
    let dst_blobstore_id = matches
        .value_of("destination-blobstore-id")
        .ok_or_else(|| Error::msg("`destination-blobstore-id` argument is required"))
        .and_then(|dst| dst.parse::<u64>().map_err(Error::from))
        .map(BlobstoreId::new)?;
    if src_blobstore_id == dst_blobstore_id {
        bail!("`source-blobstore-id` and `destination-blobstore-id` can not be equal");
    }

    let (blobstores, multiplex_id, db_address) = match storage_config {
        StorageConfig {
            metadata:
                MetadataDatabaseConfig::Remote(RemoteMetadataDatabaseConfig {
                    primary: RemoteDatabaseConfig { db_address },
                    ..
                }),
            blobstore:
                BlobConfig::Multiplexed {
                    blobstores,
                    multiplex_id,
                    ..
                },
            ..
        } => (blobstores, multiplex_id, db_address),
        storage => return Err(format_err!("unsupported storage: {:?}", storage)),
    };
    let blobstore_args = blobstores
        .iter()
        .filter(|(id, ..)| src_blobstore_id == *id)
        .map(|(.., args)| args)
        .next()
        .ok_or_else(|| format_err!("failed to find source blobstore id: {:?}", src_blobstore_id,))
        .map(|args| args.clone())?;

    let mysql_options = matches.mysql_options().clone();
    let readonly_storage = matches.readonly_storage();
    Ok(Config {
        repo_id,
        db_address,
        mysql_options,
        blobstore_args,
        src_blobstore_id,
        dst_blobstore_id,
        multiplex_id,
        start_key: matches.value_of("start-key").map(String::from),
        end_key: matches.value_of("end-key").map(String::from),
        state_key: matches.value_of("resume-state-key").map(String::from),
        ctx,
        dry_run: matches.is_present("dry-run"),
        started_at: Instant::now(),
        readonly_storage: readonly_storage.0,
    })
}

async fn get_resume_state(
    blobstore: Arc<dyn BlobstoreKeySource>,
    config: &Config,
) -> Result<State, Error> {
    let resume_state = match &config.state_key {
        Some(state_key) => {
            blobstore
                .get(&config.ctx, state_key)
                .compat()
                .map(|data| {
                    data.and_then(|data| {
                        serde_json::from_slice::<StateSerde>(&*data.into_raw_bytes()).ok()
                    })
                    .map(State::from)
                })
                .compat()
                .await
        }
        None => Ok(None),
    };

    let init_state = {
        let start = format!(
            "repo{:04}.{}",
            config.repo_id.id(),
            config.start_key.clone().unwrap_or_else(|| "".to_string())
        );
        let end = format!(
            "repo{:04}.{}",
            config.repo_id.id(),
            config.end_key.clone().unwrap_or_else(|| "\x7f".to_string()),
        );
        State::from_init(Arc::new(BlobstoreKeyParam::from(start..=end)))
    };

    resume_state.map(move |resume_state| {
        match resume_state {
            None => init_state,
            // if initial_state mismatch, start from provided initial state
            Some(ref resume_state) if resume_state.init_range != init_state.init_range => {
                init_state
            }
            Some(resume_state) => resume_state,
        }
    })
}

async fn put_resume_state(
    blobstore: Arc<dyn BlobstoreKeySource>,
    config: &Config,
    state: State,
) -> Result<State, Error> {
    match &config.state_key {
        Some(state_key) if state.count % PRESERVE_STATE_RATIO == INIT_COUNT_VALUE => {
            let started_at = config.started_at;
            cloned!(state_key, blobstore);
            serde_json::to_vec(&StateSerde::from(&state))
                .map(BlobstoreBytes::from_bytes)
                .map_err(Error::from)
                .into_future()
                .and_then(move |state_data| {
                    async move { blobstore.put(&config.ctx, state_key, state_data).await }
                        .boxed()
                        .compat()
                })
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
                .compat()
                .await
        }
        _ => Ok(state),
    }
}

async fn populate_healer_queue(
    blobstore: Arc<dyn BlobstoreKeySource>,
    queue: Arc<dyn BlobstoreSyncQueue>,
    config: Arc<Config>,
) -> Result<State, Error> {
    let mut state = get_resume_state(blobstore.clone(), &config).await?;
    let mut token = state.current_range.clone();
    loop {
        let entries = blobstore
            .enumerate(&config.ctx, &state.current_range)
            .await?;
        state = state.with_current_many(token, entries.keys.len());
        if !config.dry_run {
            let src_blobstore_id = config.src_blobstore_id;
            let multiplex_id = config.multiplex_id;
            let entries = entries
                .keys
                .into_iter()
                .map(move |entry| {
                    BlobstoreSyncQueueEntry::new(
                        entry,
                        src_blobstore_id,
                        multiplex_id,
                        DateTime::now(),
                        OperationKey::gen(),
                        None,
                    )
                })
                .collect();
            queue.add_many(&config.ctx, entries).await?;
        }
        state = put_resume_state(blobstore.clone(), &config, state).await?;
        match entries.next_token {
            Some(next_token) => {
                token = Arc::new(next_token);
            }
            None => return Ok(state),
        };
    }
}

fn make_key_source(
    fb: FacebookInit,
    args: &BlobConfig,
) -> Result<Arc<dyn BlobstoreKeySource>, Error> {
    match args {
        BlobConfig::Manifold { bucket, .. } => {
            let res = Arc::new(
                ManifoldBlob::new(fb, bucket, None, None, None, None, DEFAULT_PUT_BEHAVIOUR)?
                    .into_inner(),
            );
            Ok(res)
        }
        BlobConfig::Files { path } => {
            let res = Arc::new(Fileblob::create(path, DEFAULT_PUT_BEHAVIOUR)?);
            Ok(res)
        }
        _ => Err(format_err!("Unsupported Blobstore type")),
    }
}
#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let config = Arc::new(parse_args(fb)?);
    let blobstore = make_key_source(fb, &config.blobstore_args);
    match blobstore {
        Ok(blobstore) => {
            let mut mysql_options = config.mysql_options.clone();
            mysql_options.read_connection_type = ReadConnectionType::ReplicaOnly;

            let queue: Arc<dyn BlobstoreSyncQueue> = Arc::new(SqlBlobstoreSyncQueue::with_mysql(
                fb,
                config.db_address.clone(),
                &mysql_options,
                config.readonly_storage,
            )?);

            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(populate_healer_queue(blobstore, queue, config))?;
            Ok(())
        }
        Err(error) => Err(error),
    }
}
