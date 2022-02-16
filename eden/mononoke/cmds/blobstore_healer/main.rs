/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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
use borrowed::borrowed;
use cached_config::ConfigStore;
use chrono::Duration as ChronoDuration;
use clap_old::Arg;
use cmdlib::{
    args::{self, MononokeClapApp},
    helpers::block_execute,
    value_t,
};
use context::{CoreContext, SessionContainer};
use dummy::{DummyBlobstore, DummyBlobstoreSyncQueue};
use fbinit::FacebookInit;
use futures::future;
use futures_03_ext::BufferedParams;
use healer::Healer;
use lazy_static::lazy_static;
use metaconfig_types::{BlobConfig, DatabaseConfig, StorageConfig};
use mononoke_types::DateTime;
use slog::{info, o};
use sql_construct::SqlConstructFromDatabaseConfig;
#[cfg(fbcode_build)]
use sql_ext::facebook::MyAdmin;
use sql_ext::{
    facebook::MysqlOptions,
    replication::{NoReplicaLagMonitor, ReplicaLagMonitor, WaitForReplicationConfig},
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

const QUIET_ARG: &str = "quiet";
const ITER_LIMIT_ARG: &str = "iteration-limit";
const HEAL_MIN_AGE_ARG: &str = "heal-min-age-secs";
const HEAL_CONCURRENCY_ARG: &str = "heal-concurrency";
const HEAL_MAX_BYTES: &str = "heal-max-bytes";

lazy_static! {
    /// Minimal age of entry to consider if it has to be healed
    static ref DEFAULT_ENTRY_HEALING_MIN_AGE: ChronoDuration = ChronoDuration::minutes(2);
}

async fn maybe_schedule_healer_for_storage(
    fb: FacebookInit,
    ctx: &CoreContext,
    dry_run: bool,
    drain_only: bool,
    blobstore_sync_queue_limit: usize,
    buffered_params: BufferedParams,
    storage_config: StorageConfig,
    mysql_options: &MysqlOptions,
    source_blobstore_key: Option<String>,
    readonly_storage: ReadOnlyStorage,
    blobstore_options: &BlobstoreOptions,
    iter_limit: Option<u64>,
    heal_min_age: ChronoDuration,
    config_store: &ConfigStore,
) -> Result<(), Error> {
    let (blobstore_configs, multiplex_id, queue_db, scuba_table, scuba_sample_rate) =
        match storage_config.blobstore {
            BlobConfig::Multiplexed {
                blobstores,
                multiplex_id,
                queue_db,
                scuba_table,
                scuba_sample_rate,
                ..
            } => (
                blobstores,
                multiplex_id,
                queue_db,
                scuba_table,
                scuba_sample_rate,
            ),
            s => bail!("Storage doesn't use Multiplexed blobstore, got {:?}", s),
        };

    let sync_queue = SqlBlobstoreSyncQueue::with_database_config(
        fb,
        &queue_db,
        mysql_options,
        readonly_storage.0,
    )
    .context("While opening sync queue")?;

    let sync_queue: Arc<dyn BlobstoreSyncQueue> = if dry_run {
        let logger = ctx.logger().new(o!("sync_queue" => ""));
        Arc::new(DummyBlobstoreSyncQueue::new(sync_queue, logger))
    } else {
        Arc::new(sync_queue)
    };

    let blobstores = blobstore_configs.into_iter().map({
        borrowed!(scuba_table);
        move |(id, _, blobconfig)| async move {
            let blobconfig = BlobConfig::Logging {
                blobconfig: Box::new(blobconfig),
                scuba_table: scuba_table.clone(),
                scuba_sample_rate,
            };

            let blobstore = make_blobstore(
                fb,
                blobconfig,
                mysql_options,
                readonly_storage,
                blobstore_options,
                ctx.logger(),
                config_store,
                &blobstore_factory::default_scrub_handler(),
                None,
            )
            .await?;

            let blobstore: Arc<dyn Blobstore> = if dry_run {
                let logger = ctx.logger().new(o!("blobstore" => format!("{:?}", id)));
                Arc::new(DummyBlobstore::new(blobstore, logger))
            } else {
                blobstore
            };

            Result::<_, Error>::Ok((id, blobstore))
        }
    });

    let blobstores = future::try_join_all(blobstores)
        .await?
        .into_iter()
        .collect::<HashMap<_, _>>();

    let lag_monitor: Box<dyn ReplicaLagMonitor> = match queue_db {
        DatabaseConfig::Local(_) => Box::new(NoReplicaLagMonitor()),
        DatabaseConfig::Remote(remote) => {
            #[cfg(fbcode_build)]
            {
                let myadmin = MyAdmin::new(fb)?;
                Box::new(myadmin.single_shard_lag_monitor(remote.db_address))
            }
            #[cfg(not(fbcode_build))]
            {
                let _ = remote;
                unimplemented!("Remote DB is not yet implemented for non fbcode builds");
            }
        }
    };

    let multiplex_healer = Healer::new(
        blobstore_sync_queue_limit,
        buffered_params,
        sync_queue,
        Arc::new(blobstores),
        multiplex_id,
        source_blobstore_key,
        drain_only,
    );

    schedule_healing(ctx, multiplex_healer, lag_monitor, iter_limit, heal_min_age).await
}

// Pass None as iter_limit for never ending run
async fn schedule_healing(
    ctx: &CoreContext,
    multiplex_healer: Healer,
    lag_monitor: Box<dyn ReplicaLagMonitor>,
    iter_limit: Option<u64>,
    heal_min_age: ChronoDuration,
) -> Result<(), Error> {
    let mut count = 0;
    let wait_config = WaitForReplicationConfig::default().with_logger(ctx.logger());
    let healing_start_time = Instant::now();
    let mut total_deleted_rows = 0;

    loop {
        let iteration_start_time = Instant::now();
        count += 1;
        if let Some(iter_limit) = iter_limit {
            if count > iter_limit {
                return Ok(());
            }
        }

        lag_monitor
            .wait_for_replication(&wait_config)
            .await
            .context("While waiting for replication")?;

        let now = DateTime::now().into_chrono();
        let healing_deadline = DateTime::new(now - heal_min_age);
        let (last_batch_was_full_size, deleted_rows) = multiplex_healer
            .heal(ctx, healing_deadline)
            .await
            .context("While healing")?;

        total_deleted_rows += deleted_rows;
        let total_elapsed = healing_start_time.elapsed().as_secs_f32();
        let iteration_elapsed = iteration_start_time.elapsed().as_secs_f32();
        info!(
            ctx.logger(),
            "Iteration rows processed: {} rows, {}s; total: {} rows, {}s",
            deleted_rows,
            iteration_elapsed,
            total_deleted_rows,
            total_elapsed,
        );

        // if last batch read was not full,  wait at least 1 second, to avoid busy looping as don't
        // want to hammer the database with thousands of reads a second.
        if !last_batch_was_full_size {
            info!(ctx.logger(), "The last batch was not full size, waiting...",);
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

fn setup_app<'a, 'b>(app_name: &str) -> MononokeClapApp<'a, 'b> {
    args::MononokeAppBuilder::new(app_name)
        .with_scuba_logging_args()
        .with_fb303_args()
        .build()
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
        ).arg(
            Arg::with_name(HEAL_MAX_BYTES)
                .long(HEAL_MAX_BYTES)
                .takes_value(true)
                .required(false)
                .help("max combined size of concurrently healed blobs \
                       (approximate, will still let individual larger blobs through)")
        )
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let app_name = "blobstore_healer";
    let matches = setup_app(app_name).get_matches(fb)?;

    let storage_id = matches
        .value_of("storage-id")
        .ok_or(Error::msg("Missing storage-id"))?;
    let logger = matches.logger();
    let config_store = matches.config_store();
    let mysql_options = matches.mysql_options();
    let readonly_storage = matches.readonly_storage();
    let blobstore_options = matches.blobstore_options();
    let storage_config = args::load_storage_configs(config_store, &matches)?
        .storage
        .remove(storage_id)
        .ok_or(format_err!("Storage id `{}` not found", storage_id))?;
    let source_blobstore_key = matches.value_of("blobstore-key-like");
    let blobstore_sync_queue_limit = value_t!(matches, "sync-queue-limit", usize).unwrap_or(10000);
    let heal_concurrency = value_t!(matches, HEAL_CONCURRENCY_ARG, usize).unwrap_or(100);
    let heal_max_bytes = value_t!(matches, HEAL_MAX_BYTES, u64).unwrap_or(10_000_000_000);
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

    let scuba = matches.scuba_sample_builder();

    let ctx = SessionContainer::new_with_defaults(fb).new_context(logger.clone(), scuba);
    let buffered_params = BufferedParams {
        weight_limit: heal_max_bytes,
        buffer_size: heal_concurrency,
    };

    let healer = maybe_schedule_healer_for_storage(
        fb,
        &ctx,
        dry_run,
        drain_only,
        blobstore_sync_queue_limit,
        buffered_params,
        storage_config,
        mysql_options,
        source_blobstore_key.map(|s| s.to_string()),
        *readonly_storage,
        blobstore_options,
        iter_limit,
        healing_min_age,
        config_store,
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
