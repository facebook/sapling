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
use blobstore_factory::{make_blobstore, make_sql_factory, BlobstoreOptions, ReadOnlyStorage};
use blobstore_sync_queue::{BlobstoreSyncQueue, SqlBlobstoreSyncQueue};
use chrono::Duration as ChronoDuration;
use clap::{value_t, App, Arg};
use cloned::cloned;
use cmdlib::{
    args::{self, get_scuba_sample_builder},
    helpers::{block_execute, open_sql_with_config_and_mysql_options},
};
use configerator::ConfigeratorAPI;
use context::{CoreContext, SessionContainer};
use dummy::{DummyBlobstore, DummyBlobstoreSyncQueue};
use fbinit::FacebookInit;
use futures::{
    future::{self, join_all, loop_fn, ok, Loop},
    prelude::*,
};
use futures_ext::{spawn_future, BoxFuture, FutureExt};
use futures_preview::compat::Future01CompatExt;
use healer::Healer;
use lazy_static::lazy_static;
use metaconfig_types::{BlobConfig, MetadataDBConfig, StorageConfig};
use mononoke_types::DateTime;
use slog::{error, info, o, warn, Logger};
use sql::Connection;
use sql_ext::{open_sqlite_path, MysqlOptions};
use sql_facebook::{ext::ConnectionFbExt, myrouter, raw};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_timer::Delay;

const MAX_ALLOWED_REPLICATION_LAG_SECS: usize = 5;
const CONFIGERATOR_REGIONS_CONFIG: &str = "myrouter/regions.json";
const QUIET_ARG: &'static str = "quiet";
const ITER_LIMIT_ARG: &'static str = "iteration-limit";
const HEAL_MIN_AGE_ARG: &'static str = "heal-min-age-secs";

lazy_static! {
    /// Minimal age of entry to consider if it has to be healed
    static ref DEFAULT_ENTRY_HEALING_MIN_AGE: ChronoDuration = ChronoDuration::minutes(2);
}

fn open_mysql_raw_replicas(
    fb: FacebookInit,
    ctx: CoreContext,
    db_address: String,
    regions: Vec<String>,
) -> BoxFuture<Vec<(String, Connection)>, Error> {
    let raw_conns = regions.into_iter().map({
        cloned!(ctx);
        move |region| {
            let tier: &str = &db_address;
            let mut conn_builder = raw::Builder::new(tier, raw::InstanceRequirement::ReplicaOnly);
            conn_builder.role_override("scriptro");
            conn_builder.explicit_region(&region);
            conn_builder
                .build(fb)
                .then({
                    cloned!(ctx);
                    move |r| match r {
                        Ok(c) =>
                            Ok((region, Some(Connection::Mysql(c)))),
                        Err(_e) => {
                            warn!(ctx.logger(),
                                "Could not connect to a replica in {}, likely that region does not have one.", region);
                            Ok((region, None))
                        }
                    }
                })
        }
    });
    join_all(raw_conns)
        .map({
            cloned!(ctx);
            move |raw_conns| {
                let filtered: Vec<_> = raw_conns
                    .into_iter()
                    .filter_map(|(region, conn)| match conn {
                        Some(conn) => Some((region, conn)),
                        None => None,
                    })
                    .collect::<Vec<_>>();
                info!(
                    ctx.logger(),
                    "Monitoring regions: {:?}",
                    filtered.iter().map(|(r, _)| r).collect::<Vec<_>>()
                );
                filtered
            }
        })
        .boxify()
}

fn maybe_schedule_healer_for_storage(
    fb: FacebookInit,
    ctx: CoreContext,
    dry_run: bool,
    drain_only: bool,
    blobstore_sync_queue_limit: usize,
    storage_config: StorageConfig,
    mysql_options: MysqlOptions,
    source_blobstore_key: Option<String>,
    readonly_storage: ReadOnlyStorage,
    blobstore_options: BlobstoreOptions,
    sync_queue: BoxFuture<SqlBlobstoreSyncQueue, Error>,
    iter_limit: Option<u64>,
    heal_min_age: ChronoDuration,
) -> Result<BoxFuture<(), Error>> {
    let blobstore_configs = match &storage_config {
        StorageConfig {
            blobstore: BlobConfig::Multiplexed { blobstores, .. },
            ..
        } => blobstores.clone(),
        s => bail!("Storage doesn't use Multiplexed blobstore, got {:?}", s),
    };

    let blobstores = make_sql_factory(
        fb,
        storage_config.dbconfig.clone(),
        mysql_options,
        readonly_storage,
        ctx.logger().clone(),
    )
    .and_then({
        cloned!(ctx);
        move |sql_factory| {
            let blobstores: HashMap<_, BoxFuture<Arc<dyn Blobstore + 'static>, _>> = {
                let mut blobstores = HashMap::new();
                for (id, blobconfig) in blobstore_configs {
                    let blobstore = make_blobstore(
                        fb,
                        &blobconfig,
                        &sql_factory,
                        mysql_options,
                        readonly_storage,
                        blobstore_options.clone(),
                    );
                    blobstores.insert(id, blobstore.boxify());
                }

                if !dry_run {
                    blobstores
                } else {
                    blobstores
                        .into_iter()
                        .map(|(id, blobstore)| {
                            let logger = ctx.logger().new(o!("blobstore" => format!("{:?}", id)));
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

            join_all(
                blobstores
                    .into_iter()
                    .map(|(key, value)| value.map(move |value| (key, value))),
            )
            .map(|blobstores| blobstores.into_iter().collect::<HashMap<_, _>>())
        }
    });

    let sync_queue = if !dry_run {
        sync_queue
            .map(|q| Arc::new(q) as Arc<dyn BlobstoreSyncQueue>)
            .boxify()
    } else {
        sync_queue
            .map({
                cloned!(ctx);
                move |sync_queue| {
                    let logger = ctx.logger().new(o!("sync_queue" => ""));
                    Arc::new(DummyBlobstoreSyncQueue::new(sync_queue, logger))
                        as Arc<dyn BlobstoreSyncQueue>
                }
            })
            .boxify()
    };

    let regional_conns = match storage_config.dbconfig {
        MetadataDBConfig::LocalDB { path } => {
            open_sqlite_path(path.join("sqlite_dbs"), readonly_storage.0)
                .into_future()
                .map(|c| vec![("sqlite_region".to_string(), Connection::with_sqlite(c))])
                .boxify()
        }
        MetadataDBConfig::Mysql { db_address, .. } => {
            let cfgr = ConfigeratorAPI::new(fb)?;
            let regions = cfgr
                .get_entity(CONFIGERATOR_REGIONS_CONFIG, Duration::from_secs(5))?
                .contents;
            let regions: Vec<String> = serde_json::from_str(&regions)?;
            if let Some(myrouter_port) = mysql_options.myrouter_port {
                info!(ctx.logger(), "Monitoring regions: {:?}", regions);
                let mut conn_builder = myrouter::Builder::new();
                conn_builder
                    .service_type(myrouter::ServiceType::SLAVE)
                    .locality(myrouter::DbLocality::EXPLICIT)
                    .tier(db_address.clone(), None)
                    .port(myrouter_port);
                let mut myrouter_conns = vec![];
                for region in regions {
                    conn_builder.explicit_region(region.clone());
                    let conn: Connection = conn_builder.build_read_only().into();
                    let conn_fut = future::ok((region, conn));
                    myrouter_conns.push(conn_fut);
                }
                join_all(myrouter_conns).boxify()
            } else {
                open_mysql_raw_replicas(fb, ctx.clone(), db_address, regions)
            }
        }
    };

    let heal = regional_conns.join3(blobstores, sync_queue).and_then(
        move |(regional_conns, blobstores, sync_queue): (
            Vec<(String, Connection)>,
            HashMap<_, Arc<dyn Blobstore + 'static>>,
            Arc<dyn BlobstoreSyncQueue>,
        )| {
            let repo_healer = Healer::new(
                blobstore_sync_queue_limit,
                sync_queue,
                Arc::new(blobstores),
                source_blobstore_key,
                drain_only,
            );
            schedule_healing(ctx, repo_healer, regional_conns, iter_limit, heal_min_age)
        },
    );

    Ok(heal.boxify())
}

// Pass None as iter_limit for never ending run
fn schedule_healing(
    ctx: CoreContext,
    repo_healer: Healer,
    regional_conns: Vec<(String, Connection)>,
    iter_limit: Option<u64>,
    heal_min_age: ChronoDuration,
) -> BoxFuture<(), Error> {
    let regional_conns = Arc::new(regional_conns);
    let fut = loop_fn(0, move |count: u64| {
        match iter_limit {
            Some(limit) => {
                if count >= limit {
                    return future::ok(Loop::Break(())).left_future();
                }
            }
            _ => (),
        }
        let max_replication_lag_fn = max_replication_lag(regional_conns.clone());
        let now = DateTime::now().into_chrono();
        let healing_deadline = DateTime::new(now - heal_min_age);
        repo_healer
            .heal(ctx.clone(), healing_deadline)
            .and_then({
                let logger = ctx.logger().clone();
                move |last_batch_full_sized| {
                    ensure_small_db_replication_lag(
                        logger,
                        max_replication_lag_fn,
                        last_batch_full_sized,
                    )
                    .map(move |_lag| Loop::Continue(count + 1))
                }
            })
            .right_future()
    });

    spawn_future(fut).boxify()
}

fn max_replication_lag(
    replication_lag_db_conns: Arc<Vec<(String, Connection)>>,
) -> impl Fn() -> Box<dyn Future<Item = (String, usize), Error = Error> + Send> {
    move || {
        // Check what max replication lag on replicas
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
                        }
                        None => Err(err),
                    })
                    .and_then({
                        cloned!(conn);
                        move |maybe_secs| {
                            match (&conn, maybe_secs) {
                                (Connection::Sqlite(_), None) => Ok((region, 0)),
                                (Connection::Sqlite(_), Some(x)) => Ok((region, x)),
                                (_, Some(lag_secs)) =>  Ok((region, lag_secs)),
                                (_, None) => Err(format_err!("Could not fetch db replication lag for {}. Failing to avoid overloading db", region)),
                            }
                        }
                    })
            })
            .collect();

        Box::new(join_all(lag_secs_futs).and_then(move |lags| {
            let (region, max_lag_secs): (String, usize) = lags
                .into_iter()
                .max_by_key(|(_, lag)| *lag)
                .unwrap_or(("".to_string(), 0));
            ok((region, max_lag_secs))
        }))
    }
}

fn compute_delay(
    logger: Logger,
    max_lag_secs: usize,
    last_batch_full_sized: bool,
) -> Option<usize> {
    // if there are items remaining (i.e. last batch read was full sized),
    // and lag < bound, carry on without pause
    if last_batch_full_sized && max_lag_secs < MAX_ALLOWED_REPLICATION_LAG_SECS {
        info!(
            logger,
            "As there are items remaining and lag < bound, carry on without pause",
        );
        return None;
    }

    // if last batch read was not full,  wait at least 1 second,
    // to avoid busy looping as don't want to hammer the database
    // with thousands of reads a second.
    let max_lag_secs = if !last_batch_full_sized {
        info!(
            logger,
            "As the last batch was not full sized, wait at least one second",
        );
        std::cmp::max(1, max_lag_secs)
    } else {
        max_lag_secs
    };

    return Some(max_lag_secs);
}

fn ensure_small_db_replication_lag<F>(
    logger: Logger,
    compute_max_lag: F,
    last_batch_full_sized: bool,
) -> impl Future<Item = usize, Error = Error>
where
    F: Fn() -> Box<dyn Future<Item = (String, usize), Error = Error> + Send>,
{
    // Make sure we've slept at least once before continuing
    let last_max_lag: Option<usize> = None;
    let total_lag: usize = 0;

    loop_fn(
        (total_lag, last_max_lag),
        move |(total_lag, last_max_lag)| {
            if last_max_lag.is_some() && last_max_lag.unwrap() < MAX_ALLOWED_REPLICATION_LAG_SECS {
                // No need check rep lag again, was ok on last loop
                return ok(Loop::Break(total_lag)).left_future();
            }

            cloned!(logger);

            // Check what max replication lag on replicas, and sleep for that long.
            // This is done in order to avoid overloading the db.
            compute_max_lag()
                .and_then(move |(region, max_lag_secs)| {
                    info!(
                        logger,
                        "Max replication lag is {}, {}s", region, max_lag_secs,
                    );

                    match compute_delay(logger, max_lag_secs, last_batch_full_sized) {
                        None => return ok(Loop::Break(total_lag)).left_future(),
                        Some(max_lag_secs) => {
                            let max_lag = Duration::from_secs(max_lag_secs as u64);
                            let start = Instant::now();
                            let next_iter_deadline = start + max_lag;
                            let total_lag = total_lag + max_lag_secs;

                            Delay::new(next_iter_deadline)
                                .map(move |()| Loop::Continue((total_lag, Some(max_lag_secs))))
                                .from_err()
                                .right_future()
                        }
                    }
                })
                .right_future()
        },
    )
}

fn setup_app<'a, 'b>(app_name: &str) -> App<'a, 'b> {
    let app = args::MononokeApp::new(app_name)
        .with_scuba_logging_args()
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
        )
        .arg(
            Arg::with_name(QUIET_ARG)
                .long(QUIET_ARG)
                .short("q")
                .takes_value(false)
                .required(false)
                .help("Log a lot less"),
        )
        .arg(
            Arg::with_name(ITER_LIMIT_ARG)
                .long(ITER_LIMIT_ARG)
                .takes_value(true)
                .required(false)
                .help("If specified, only perform the given number of iterations"),
        )
        .arg(
            Arg::with_name(HEAL_MIN_AGE_ARG)
                .long(HEAL_MIN_AGE_ARG)
                .takes_value(true)
                .required(false)
                .help("Seconds. If specified, override default minimum age to heal of 120 seconds"),
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
    let mysql_options = args::parse_mysql_options(&matches);
    let readonly_storage = args::parse_readonly_storage(&matches);
    let blobstore_options = args::parse_blobstore_options(&matches);
    let storage_config = args::read_storage_configs(fb, &matches)?
        .remove(storage_id)
        .ok_or(format_err!("Storage id `{}` not found", storage_id))?;
    let source_blobstore_key = matches.value_of("blobstore-key-like");
    let blobstore_sync_queue_limit = value_t!(matches, "sync-queue-limit", usize).unwrap_or(10000);
    let dry_run = matches.is_present("dry-run");
    let drain_only = matches.is_present("drain-only");
    if drain_only && source_blobstore_key.is_none() {
        bail!("Missing --blobstore-key-like restriction for --drain-only");
    }

    let iter_limit = args::get_u64_opt(&matches, ITER_LIMIT_ARG);
    let healing_min_age = args::get_i64_opt(&matches, HEAL_MIN_AGE_ARG)
        .map(|s| ChronoDuration::seconds(s))
        .unwrap_or(*DEFAULT_ENTRY_HEALING_MIN_AGE);
    let quiet = matches.is_present(QUIET_ARG);
    if !quiet {
        info!(logger, "Using storage_config {:?}", storage_config);
    }

    let scuba = get_scuba_sample_builder(fb, &matches)?;

    let ctx = SessionContainer::new_with_defaults(fb).new_context(logger.clone(), scuba);

    let sync_queue = open_sql_with_config_and_mysql_options::<SqlBlobstoreSyncQueue>(
        fb,
        storage_config.dbconfig.clone(),
        mysql_options,
        readonly_storage,
    );

    let healer = {
        let scheduled = maybe_schedule_healer_for_storage(
            fb,
            ctx,
            dry_run,
            drain_only,
            blobstore_sync_queue_limit,
            storage_config,
            mysql_options,
            source_blobstore_key.map(|s| s.to_string()),
            readonly_storage,
            blobstore_options,
            sync_queue,
            iter_limit,
            healing_min_age,
        );

        match scheduled {
            Err(err) => {
                error!(logger, "Did not schedule, because of: {:#?}", err);
                return Err(err);
            }
            Ok(scheduled) => scheduled,
        }
    };

    block_execute(healer.compat(), fb, app_name, &logger, &matches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use tokio::runtime::Runtime;

    fn compute_delay_test(
        fb: FacebookInit,
        simulated_lag_secs: usize,
        last_batch_full: bool,
        expected_wait_time: Option<usize>,
    ) {
        let ctx = CoreContext::test_mock(fb);
        let logger = ctx.logger();
        cloned!(logger);

        let delay = compute_delay(logger, simulated_lag_secs, last_batch_full);

        assert!(
            delay == expected_wait_time,
            "compute_delay should have returned {:?}, but returned {:?}",
            expected_wait_time,
            delay,
        );
    }

    #[fbinit::test]
    fn compute_delay_test_suite(fb: FacebookInit) {
        // max lag is 2, last batch is not full, we should wait at least 2 seconds
        compute_delay_test(fb, 2, false, Some(2));
        // max lag is 0, last batch is not full, we should wait at least 1 seconds
        compute_delay_test(fb, 0, false, Some(1));
        // max lag is 3, last batch is full, we should not wait
        compute_delay_test(fb, 3, true, None)
    }

    fn simulated_constant_lag(
        lag: usize,
    ) -> impl Fn() -> Box<dyn Future<Item = (String, usize), Error = Error> + Send> {
        move || Box::new(ok(("simulated_lag".to_string(), lag)))
    }

    fn simulated_decreasing_lag(
        initial_lag: usize,
        decrement: usize,
    ) -> impl Fn() -> Box<dyn Future<Item = (String, usize), Error = Error> + Send> {
        let lag = RefCell::new(initial_lag);
        move || {
            Box::new(ok((
                "simulated_lag".to_string(),
                lag.replace_with(|&mut old| if old < decrement { 0 } else { old - decrement }),
            )))
        }
    }

    fn ensure_small_db_replication_lag_test<F>(
        fb: FacebookInit,
        simulated_lag: F,
        last_batch_full: bool,
        expected_wait_time: usize,
    ) where
        F: Fn() -> Box<dyn Future<Item = (String, usize), Error = Error> + Send> + Send + 'static,
    {
        let ctx = CoreContext::test_mock(fb);
        let logger = ctx.logger();
        cloned!(logger);

        let mut runtime = Runtime::new().unwrap();
        let lag_future = ensure_small_db_replication_lag(logger, simulated_lag, last_batch_full);
        let elapsed_delay = runtime.block_on(lag_future);

        match elapsed_delay {
            Ok(elapsed_delay) =>
            assert!(elapsed_delay == expected_wait_time,
                "ensure_small_db_replication_lag should have waited {} seconds, but waited {} seconds",
                expected_wait_time, elapsed_delay),
            Err(e) => assert!(false, "ensure_small_db_replication_lab returned an error: {:?}", e),
        }
    }

    #[fbinit::test]
    fn ensure_small_db_replication_lag_test_suite(fb: FacebookInit) {
        // max lag is 2, last batch is not full, we should wait at least 2 seconds
        ensure_small_db_replication_lag_test(fb, simulated_constant_lag(2), false, 2);
        // max lag is 0, last batch is not full, we should wait at least 1 seconds
        ensure_small_db_replication_lag_test(fb, simulated_constant_lag(0), false, 1);
        // max lag is 3, last batch is full, we should not wait
        ensure_small_db_replication_lag_test(fb, simulated_constant_lag(3), true, 0);

        // max lag initially is 8, and decreases at every cycle by 2.
        // lags are thus 8, 6, 4, ...  since the first two lags are > MAX_LAG, we wait 8 + 6
        // the third lag is < MAX_LAG and last batch is full, so we don't wait anymore
        ensure_small_db_replication_lag_test(fb, simulated_decreasing_lag(8, 2), true, 14);

        // max lag initially is 8, and decreases at every cycle by 2.
        // lags are thus 8, 6, 4, ...  since the first two lags are > MAX_LAG, we wait 8 + 6
        // then lag is 4, but the last batch is not full, so we wait 4 more seconds
        ensure_small_db_replication_lag_test(fb, simulated_decreasing_lag(8, 2), false, 18);
    }

}
