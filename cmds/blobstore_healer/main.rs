/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]
#![feature(never_type)]

mod dummy;
mod healer;

use anyhow::{bail, format_err, Error, Result};
use blobstore::Blobstore;
use blobstore_sync_queue::{BlobstoreSyncQueue, SqlBlobstoreSyncQueue, SqlConstructors};
use clap::{value_t, App};
use cloned::cloned;
use cmdlib::{args, helpers::create_runtime, monitoring};
use configerator::ConfigeratorAPI;
use context::CoreContext;
use dummy::{DummyBlobstore, DummyBlobstoreSyncQueue};
use failure_ext::chain::ChainExt;
use fbinit::FacebookInit;
use futures::{
    future::{join_all, loop_fn, ok, Loop},
    prelude::*,
};
use futures_ext::{spawn_future, BoxFuture, FutureExt};
use healer::Healer;
use manifoldblob::ThriftManifoldBlob;
use metaconfig_types::{BlobConfig, MetadataDBConfig, StorageConfig};
use prefixblob::PrefixBlobstore;
use slog::{error, info, o, Logger};
use sql::{myrouter, Connection};
use sqlblob::Sqlblob;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_timer::Delay;

const MAX_ALLOWED_REPLICATION_LAG_SECS: usize = 5;
const CONFIGERATOR_REGIONS_CONFIG: &str = "myrouter/regions.json";

fn maybe_schedule_healer_for_storage(
    fb: FacebookInit,
    dry_run: bool,
    drain_only: bool,
    blobstore_sync_queue_limit: usize,
    logger: Logger,
    storage_config: StorageConfig,
    myrouter_port: u16,
    replication_lag_db_regions: Vec<String>,
    source_blobstore_key: Option<String>,
    readonly_storage: bool,
) -> Result<BoxFuture<(), Error>> {
    let (db_address, blobstores_args) = match &storage_config {
        StorageConfig {
            dbconfig: MetadataDBConfig::Mysql { db_address, .. },
            blobstore: BlobConfig::Multiplexed { blobstores, .. },
        } => (db_address.clone(), blobstores.clone()),
        _ => bail!("Repo doesn't use Multiplexed blobstore"),
    };

    let blobstores: HashMap<_, BoxFuture<Arc<dyn Blobstore + 'static>, _>> = {
        let mut blobstores = HashMap::new();
        for (id, args) in blobstores_args {
            match args {
                BlobConfig::Manifold { bucket, prefix } => {
                    let blobstore = ThriftManifoldBlob::new(fb, bucket)
                        .chain_err("While opening ThriftManifoldBlob")?;
                    let blobstore = PrefixBlobstore::new(blobstore, format!("flat/{}", prefix));
                    let blobstore: Arc<dyn Blobstore> = Arc::new(blobstore);
                    blobstores.insert(id, ok(blobstore).boxify());
                }
                BlobConfig::Mysql {
                    shard_map,
                    shard_num,
                } => {
                    let blobstore = Sqlblob::with_myrouter(
                        fb,
                        shard_map,
                        myrouter_port,
                        shard_num,
                        readonly_storage,
                    )
                    .map(|blobstore| -> Arc<dyn Blobstore> { Arc::new(blobstore) });
                    blobstores.insert(id, blobstore.boxify());
                }
                unsupported => bail!("Unsupported blobstore type {:?}", unsupported),
            }
        }

        if !dry_run {
            blobstores
        } else {
            blobstores
                .into_iter()
                .map(|(id, blobstore)| {
                    let logger = logger.new(o!("blobstore" => format!("{:?}", id)));
                    let blobstore = blobstore
                        .map(move |blobstore| -> Arc<dyn Blobstore> {
                            Arc::new(DummyBlobstore::new(blobstore, logger))
                        })
                        .boxify();
                    (id, blobstore)
                })
                .collect()
        }
    };
    let blobstores = join_all(
        blobstores
            .into_iter()
            .map(|(key, value)| value.map(move |value| (key, value))),
    )
    .map(|blobstores| blobstores.into_iter().collect::<HashMap<_, _>>());

    let sync_queue: Arc<dyn BlobstoreSyncQueue> = {
        let sync_queue = SqlBlobstoreSyncQueue::with_myrouter(
            db_address.clone(),
            myrouter_port,
            readonly_storage,
        );

        if !dry_run {
            Arc::new(sync_queue)
        } else {
            let logger = logger.new(o!("sync_queue" => ""));
            Arc::new(DummyBlobstoreSyncQueue::new(sync_queue, logger))
        }
    };

    let mut replication_lag_db_conns = Vec::new();
    let mut conn_builder = Connection::myrouter_builder();
    conn_builder
        .service_type(myrouter::ServiceType::SLAVE)
        .locality(myrouter::DbLocality::EXPLICIT)
        .tier(db_address.clone(), None)
        .port(myrouter_port);

    for region in replication_lag_db_regions {
        conn_builder.explicit_region(region.clone());
        replication_lag_db_conns.push((region, conn_builder.build_read_only()));
    }

    let heal = blobstores.and_then(
        move |blobstores: HashMap<_, Arc<dyn Blobstore + 'static>>| {
            let repo_healer = Healer::new(
                logger.clone(),
                blobstore_sync_queue_limit,
                sync_queue,
                Arc::new(blobstores),
                source_blobstore_key,
                drain_only,
            );

            if dry_run {
                let ctx = CoreContext::new_with_logger(fb, logger);
                repo_healer.heal(ctx).boxify()
            } else {
                schedule_everlasting_healing(fb, logger, repo_healer, replication_lag_db_conns)
            }
        },
    );
    Ok(myrouter::wait_for_myrouter(myrouter_port, db_address)
        .and_then(|_| heal)
        .boxify())
}

fn schedule_everlasting_healing(
    fb: FacebookInit,
    logger: Logger,
    repo_healer: Healer,
    replication_lag_db_conns: Vec<(String, Connection)>,
) -> BoxFuture<(), Error> {
    let replication_lag_db_conns = Arc::new(replication_lag_db_conns);

    let fut = loop_fn((), move |()| {
        let ctx = CoreContext::new_with_logger(fb, logger.clone());

        cloned!(logger, replication_lag_db_conns);
        repo_healer.heal(ctx).and_then(move |()| {
            ensure_small_db_replication_lag(logger, replication_lag_db_conns)
                .map(|()| Loop::Continue(()))
        })
    });

    spawn_future(fut).boxify()
}

fn ensure_small_db_replication_lag(
    logger: Logger,
    replication_lag_db_conns: Arc<Vec<(String, Connection)>>,
) -> impl Future<Item = (), Error = Error> {
    // Make sure we've slept at least once before continuing
    let last_max_lag: Option<usize> = None;

    loop_fn(last_max_lag, move |last_max_lag| {
        if last_max_lag.is_some() && last_max_lag.unwrap() < MAX_ALLOWED_REPLICATION_LAG_SECS {
            // No need check rep lag again, was ok on last loop
            return ok(Loop::Break(())).left_future();
        }

        // Check what max replication lag on replicas, and sleep for that long.
        // This is done in order to avoid overloading the db.
        let lag_secs_futs: Vec<_> = replication_lag_db_conns
            .iter()
            .map(|(region, conn)| {
                cloned!(region);

                conn.show_replica_lag_secs()
                    .or_else(|err| match err.downcast_ref::<sql::error::ServerError>() {
                        Some(server_error) => {
                            // 1918 is discovery failed (i.e. there is no server matching the
                            // constraints). This is fine, that means we don't need to monitor it.
                            if server_error.code == 1918 {
                                Ok(Some(0))
                            } else {
                                Err(err)
                            }
                        },
                        None => Err(err),
                    })
                    .and_then(|maybe_secs| {
                        let err = format_err!(
                            "Could not fetch db replication lag for {}. Failing to avoid overloading db",
                            region
                        );

                        maybe_secs
                            .ok_or(err)
                            .map(|lag_secs| (region, lag_secs))
                    })
            })
            .collect();

        cloned!(logger);

        join_all(lag_secs_futs)
            .and_then(move |lags| {
                let (region, max_lag_secs): (String, usize) = lags
                    .into_iter()
                    .max_by_key(|(_, lag)| *lag)
                    .unwrap_or(("".to_string(), 0));
                info!(
                    logger,
                    "Max replication lag is {}, {}s", region, max_lag_secs
                );
                let max_lag = Duration::from_secs(max_lag_secs as u64);

                let start = Instant::now();
                let next_iter_deadline = start + max_lag;

                Delay::new(next_iter_deadline)
                    .map(move |()| Loop::Continue(Some(max_lag_secs)))
                    .from_err()
            })
            .right_future()
    })
}

fn setup_app<'a, 'b>(app_name: &str) -> App<'a, 'b> {
    let app = args::MononokeApp::new(app_name)
        .build()
        .version("0.0.0")
        .about("Monitors blobstore_sync_queue to heal blobstores with missing data")
        .args_from_usage(
            r#"
            --sync-queue-limit=[LIMIT] 'set limit for how many queue entries to process'
            --dry-run 'performs a single healing and prints what would it do without doing it'
            --drain-only 'drain the queue without healing.  Use with caution.'
            --storage-id=[STORAGE_ID] 'id of storage group to be healed, e.g. manifold_xdb_multiplex'
            --blobstore-key-like=[BLOBSTORE_KEY] 'Optional source blobstore key in SQL LIKE format, e.g. repo0138.hgmanifest%'
        "#,
    );
    args::add_fb303_args(app)
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let app_name = "blobstore_healer";
    let matches = setup_app(app_name).get_matches();

    let storage_id = matches
        .value_of("storage-id")
        .ok_or(Error::msg("Missing storage-id"))?;
    let logger = args::init_logging(fb, &matches);
    let myrouter_port =
        args::parse_myrouter_port(&matches).ok_or(Error::msg("Missing --myrouter-port"))?;
    let readonly_storage = args::parse_readonly_storage(&matches);
    let storage_config = args::read_storage_configs(&matches)?
        .remove(storage_id)
        .ok_or(format_err!("Storage id `{}` not found", storage_id))?;
    let source_blobstore_key = matches.value_of("blobstore-key-like");
    let blobstore_sync_queue_limit = value_t!(matches, "sync-queue-limit", usize).unwrap_or(10000);
    let dry_run = matches.is_present("dry-run");
    let drain_only = matches.is_present("drain-only");
    if drain_only && source_blobstore_key.is_none() {
        bail!("Missing --blobstore-key-like restriction for --drain-only");
    }
    info!(logger, "Using storage_config {:#?}", storage_config);

    let cfgr = ConfigeratorAPI::new(fb)?;
    let regions = cfgr
        .get_entity(CONFIGERATOR_REGIONS_CONFIG, Duration::from_secs(5))?
        .contents;
    let regions: Vec<String> = serde_json::from_str(&regions)?;
    info!(logger, "Monitoring regions: {:?}", regions);

    let healer = {
        let scheduled = maybe_schedule_healer_for_storage(
            fb,
            dry_run,
            drain_only,
            blobstore_sync_queue_limit,
            logger.clone(),
            storage_config,
            myrouter_port,
            regions,
            source_blobstore_key.map(|s| s.to_string()),
            readonly_storage.0,
        );

        match scheduled {
            Err(err) => {
                error!(logger, "Did not schedule, because of: {:#?}", err);
                return Err(err);
            }
            Ok(scheduled) => {
                info!(logger, "Successfully scheduled");
                scheduled
            }
        }
    };

    let mut runtime = create_runtime(None)?;

    // Thread with a thrift service is now detached
    monitoring::start_fb303_and_stats_agg(fb, &mut runtime, app_name, &logger, &matches)?;

    let result = runtime.block_on(healer.map(|_| ()));
    runtime.shutdown_on_idle();
    result
}
