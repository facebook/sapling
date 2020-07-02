/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]
#![feature(never_type)]

mod dummy;
mod healer;

use anyhow::{bail, format_err, Context, Error, Result};
use blobstore::Blobstore;
use blobstore_factory::{make_blobstore, BlobstoreOptions, ReadOnlyStorage};
use blobstore_sync_queue::{BlobstoreSyncQueue, SqlBlobstoreSyncQueue};
use cached_config::ConfigStore;
use chrono::Duration as ChronoDuration;
use clap::{value_t, App, Arg};
use cmdlib::{
    args::{self, get_scuba_sample_builder},
    helpers::block_execute,
};
use context::{CoreContext, SessionContainer};
use dummy::{DummyBlobstore, DummyBlobstoreSyncQueue};
use fbinit::FacebookInit;
use futures::{compat::Future01CompatExt, future};
use healer::Healer;
use lazy_static::lazy_static;
use metaconfig_types::{BlobConfig, LocalDatabaseConfig, MetadataDatabaseConfig, StorageConfig};
use mononoke_types::DateTime;
use slog::{info, o, warn};
use sql::Connection;
use sql_construct::SqlConstructFromDatabaseConfig;
use sql_ext::facebook::myrouter_ready;
use sql_ext::{
    facebook::MysqlOptions,
    open_sqlite_path,
    replication::{LaggableCollectionMonitor, ReplicaLagMonitor, WaitForReplicationConfig},
};
use sql_facebook::{myrouter, raw};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

const CONFIGERATOR_REGIONS_CONFIG: &str = "myrouter/regions.json";
const QUIET_ARG: &'static str = "quiet";
const ITER_LIMIT_ARG: &'static str = "iteration-limit";
const HEAL_MIN_AGE_ARG: &'static str = "heal-min-age-secs";
const HEAL_CONCURRENCY_ARG: &str = "heal-concurrency";

lazy_static! {
    /// Minimal age of entry to consider if it has to be healed
    static ref DEFAULT_ENTRY_HEALING_MIN_AGE: ChronoDuration = ChronoDuration::minutes(2);
}

async fn open_mysql_raw_replicas(
    fb: FacebookInit,
    ctx: &CoreContext,
    tier: &str,
    regions: Arc<Vec<String>>,
) -> Result<Vec<(String, Connection)>, Error> {
    let raw_conns = regions.iter().cloned().map({
        move |region| async move {
            let mut conn_builder = raw::Builder::new(tier, raw::InstanceRequirement::ReplicaOnly);
            conn_builder.role_override("scriptro");
            conn_builder.explicit_region(&region);

            let conn = match conn_builder.build(fb).compat().await {
                Ok(c) =>
                    Some(Connection::Mysql(c)),
                Err(_e) => {
                    warn!(ctx.logger(),
                        "Could not connect to a replica in {}, likely that region does not have one.", region);
                    None
                }
            };

            (region, conn)
        }
    });

    let filtered: Vec<_> = future::join_all(raw_conns)
        .await
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

    Ok(filtered)
}

async fn maybe_schedule_healer_for_storage(
    fb: FacebookInit,
    ctx: &CoreContext,
    dry_run: bool,
    drain_only: bool,
    blobstore_sync_queue_limit: usize,
    heal_concurrency: usize,
    storage_config: StorageConfig,
    mysql_options: MysqlOptions,
    source_blobstore_key: Option<String>,
    readonly_storage: ReadOnlyStorage,
    blobstore_options: &BlobstoreOptions,
    iter_limit: Option<u64>,
    heal_min_age: ChronoDuration,
) -> Result<(), Error> {
    let (blobstore_configs, multiplex_id, queue_db) = match storage_config.blobstore {
        BlobConfig::Multiplexed {
            blobstores,
            multiplex_id,
            queue_db,
            ..
        } => (blobstores, multiplex_id, queue_db),
        s => bail!("Storage doesn't use Multiplexed blobstore, got {:?}", s),
    };

    myrouter_ready(
        queue_db.remote_address(),
        mysql_options,
        ctx.logger().clone(),
    )
    .compat()
    .await?;

    let sync_queue = SqlBlobstoreSyncQueue::with_database_config(
        fb,
        &queue_db,
        mysql_options,
        readonly_storage.0,
    )
    .await
    .context("While opening sync queue")?;

    let sync_queue: Arc<dyn BlobstoreSyncQueue> = if dry_run {
        let logger = ctx.logger().new(o!("sync_queue" => ""));
        Arc::new(DummyBlobstoreSyncQueue::new(sync_queue, logger))
    } else {
        Arc::new(sync_queue)
    };

    let blobstores = blobstore_configs
        .into_iter()
        .map(|(id, blobconfig)| async move {
            let blobstore = make_blobstore(
                fb,
                blobconfig,
                mysql_options,
                readonly_storage,
                blobstore_options,
                ctx.logger(),
            )
            .await?;

            let blobstore: Arc<dyn Blobstore> = if dry_run {
                let logger = ctx.logger().new(o!("blobstore" => format!("{:?}", id)));
                Arc::new(DummyBlobstore::new(blobstore, logger))
            } else {
                blobstore
            };

            Result::<_, Error>::Ok((id, blobstore))
        });

    let blobstores = future::try_join_all(blobstores)
        .await?
        .into_iter()
        .collect::<HashMap<_, _>>();

    let regional_conns = match storage_config.metadata {
        MetadataDatabaseConfig::Local(LocalDatabaseConfig { path }) => {
            let c = open_sqlite_path(path.join("sqlite_dbs"), readonly_storage.0)?;
            vec![("sqlite_region".to_string(), Connection::with_sqlite(c))]
        }
        MetadataDatabaseConfig::Remote(remote) => {
            let db_address = remote.primary.db_address;
            let regions = ConfigStore::configerator(fb, None, None, Duration::from_secs(5))?
                .get_config_handle::<Vec<String>>(CONFIGERATOR_REGIONS_CONFIG.to_owned())?
                .get();
            info!(ctx.logger(), "Discovered regions: {:?}", *regions);
            if let Some(myrouter_port) = mysql_options.myrouter_port {
                let mut conn_builder = myrouter::Builder::new();
                conn_builder
                    .service_type(myrouter::ServiceType::SLAVE)
                    .locality(myrouter::DbLocality::EXPLICIT)
                    .tier(db_address, None)
                    .port(myrouter_port);

                regions
                    .iter()
                    .map(|region| {
                        conn_builder.explicit_region(region.clone());
                        let conn: Connection = conn_builder.build_read_only().into();
                        (region.clone(), conn)
                    })
                    .collect::<Vec<_>>()
            } else {
                open_mysql_raw_replicas(fb, ctx, &db_address, regions).await?
            }
        }
    };

    let multiplex_healer = Healer::new(
        blobstore_sync_queue_limit,
        heal_concurrency,
        sync_queue,
        Arc::new(blobstores),
        multiplex_id,
        source_blobstore_key,
        drain_only,
    );

    schedule_healing(
        ctx,
        multiplex_healer,
        regional_conns,
        iter_limit,
        heal_min_age,
    )
    .await
}

// Pass None as iter_limit for never ending run
async fn schedule_healing(
    ctx: &CoreContext,
    multiplex_healer: Healer,
    conns: Vec<(String, Connection)>,
    iter_limit: Option<u64>,
    heal_min_age: ChronoDuration,
) -> Result<(), Error> {
    let mut count = 0;
    let replication_monitor = LaggableCollectionMonitor::new(conns);
    let wait_config = WaitForReplicationConfig::default().with_logger(ctx.logger());

    loop {
        count += 1;
        if let Some(iter_limit) = iter_limit {
            if count > iter_limit {
                return Ok(());
            }
        }

        replication_monitor
            .wait_for_replication(&wait_config)
            .await
            .context("While waiting for replication")?;

        let now = DateTime::now().into_chrono();
        let healing_deadline = DateTime::new(now - heal_min_age);
        let last_batch_was_full_size = multiplex_healer
            .heal(ctx.clone(), healing_deadline)
            .compat()
            .await
            .context("While healing")?;

        // if last batch read was not full,  wait at least 1 second, to avoid busy looping as don't
        // want to hammer the database with thousands of reads a second.
        if !last_batch_was_full_size {
            info!(ctx.logger(), "The last batch was not full size, waiting...",);
            tokio::time::delay_for(Duration::from_secs(1)).await;
        }
    }
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
        ).arg(
            Arg::with_name(HEAL_CONCURRENCY_ARG)
                .long(HEAL_CONCURRENCY_ARG)
                .takes_value(true)
                .required(false)
                .help("How maby blobs to heal concurrently."),
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
    let storage_config = args::load_storage_configs(fb, &matches)?
        .storage
        .remove(storage_id)
        .ok_or(format_err!("Storage id `{}` not found", storage_id))?;
    let source_blobstore_key = matches.value_of("blobstore-key-like");
    let blobstore_sync_queue_limit = value_t!(matches, "sync-queue-limit", usize).unwrap_or(10000);
    let heal_concurrency = value_t!(matches, HEAL_CONCURRENCY_ARG, usize).unwrap_or(100);
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

    let healer = maybe_schedule_healer_for_storage(
        fb,
        &ctx,
        dry_run,
        drain_only,
        blobstore_sync_queue_limit,
        heal_concurrency,
        storage_config,
        mysql_options,
        source_blobstore_key.map(|s| s.to_string()),
        readonly_storage,
        &blobstore_options,
        iter_limit,
        healing_min_age,
    );

    block_execute(
        healer,
        fb,
        app_name,
        &logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}
