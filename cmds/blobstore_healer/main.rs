// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(never_type)]

mod dummy;
mod healer;
mod rate_limiter;

use crate::rate_limiter::RateLimiter;
use blobstore::{Blobstore, PrefixBlobstore};
use blobstore_sync_queue::{BlobstoreSyncQueue, SqlBlobstoreSyncQueue, SqlConstructors};
use clap::{value_t, App};
use cmdlib::args;
use context::CoreContext;
use dummy::{DummyBlobstore, DummyBlobstoreSyncQueue};
use failure_ext::{bail_msg, ensure_msg, err_msg, prelude::*};
use futures::{
    future::{join_all, loop_fn, ok, Loop},
    prelude::*,
};
use futures_ext::{spawn_future, BoxFuture, FutureExt};
use glusterblob::Glusterblob;
use healer::RepoHealer;
use manifoldblob::ThriftManifoldBlob;
use metaconfig_types::{RemoteBlobstoreArgs, RepoConfig, RepoType};
use mononoke_types::RepositoryId;
use slog::{error, info, o, Logger};
use sql::myrouter;
use sqlblob::Sqlblob;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_timer::Delay;

const MIN_HEALER_ITERATION_DELAY: Duration = Duration::from_secs(60);

fn maybe_schedule_healer_for_repo(
    dry_run: bool,
    blobstore_sync_queue_limit: usize,
    logger: Logger,
    rate_limiter: RateLimiter,
    config: RepoConfig,
    myrouter_port: u16,
) -> Result<BoxFuture<(), Error>> {
    ensure_msg!(config.enabled, "Repo is disabled");

    let (db_address, blobstores_args) = match config.repotype {
        RepoType::BlobRemote {
            ref db_address,
            blobstores_args: RemoteBlobstoreArgs::Multiplexed { ref blobstores, .. },
            ..
        } => (db_address.clone(), blobstores.clone()),
        _ => bail_msg!("Repo doesn't use Multiplexed blobstore"),
    };

    let blobstores = {
        let mut blobstores = HashMap::new();
        for (id, args) in blobstores_args.into_iter() {
            match args {
                RemoteBlobstoreArgs::Manifold(args) => {
                    let blobstore = ThriftManifoldBlob::new(args.bucket)
                        .chain_err("While opening ThriftManifoldBlob")?;
                    let blobstore =
                        PrefixBlobstore::new(blobstore, format!("flat/{}", args.prefix));
                    let blobstore: Arc<Blobstore> = Arc::new(blobstore);
                    blobstores.insert(id, ok(blobstore).boxify());
                }
                RemoteBlobstoreArgs::Gluster(args) => {
                    let blobstore = Glusterblob::with_smc(args.tier, args.export, args.basepath)
                        .map(|blobstore| -> Arc<Blobstore> { Arc::new(blobstore) })
                        .boxify();
                    blobstores.insert(id, blobstore);
                }
                RemoteBlobstoreArgs::Mysql(args) => {
                    let blobstore: Arc<Blobstore> = Arc::new(Sqlblob::with_myrouter(
                        RepositoryId::new(config.repoid),
                        args.shardmap,
                        myrouter_port,
                        args.shard_num,
                    ));
                    blobstores.insert(id, ok(blobstore).boxify());
                }
                RemoteBlobstoreArgs::Multiplexed { .. } => {
                    bail_msg!("Unsupported nested Multiplexed blobstore")
                }
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
                        .map(move |blobstore| -> Arc<Blobstore> {
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

    let sync_queue: Arc<BlobstoreSyncQueue> = {
        let sync_queue = SqlBlobstoreSyncQueue::with_myrouter(db_address.clone(), myrouter_port);

        if !dry_run {
            Arc::new(sync_queue)
        } else {
            let logger = logger.new(o!("sync_queue" => ""));
            Arc::new(DummyBlobstoreSyncQueue::new(sync_queue, logger))
        }
    };

    let heal = blobstores.and_then(move |blobstores| {
        let repo_healer = RepoHealer::new(
            logger,
            blobstore_sync_queue_limit,
            RepositoryId::new(config.repoid),
            rate_limiter,
            sync_queue,
            Arc::new(blobstores),
        );

        if dry_run {
            // TODO(luk) use a proper context here and put the logger inside of it
            let ctx = CoreContext::test_mock();
            repo_healer.heal(ctx).boxify()
        } else {
            schedule_everlasting_healing(repo_healer)
        }
    });
    Ok(myrouter::wait_for_myrouter(myrouter_port, db_address)
        .and_then(|_| heal)
        .boxify())
}

fn schedule_everlasting_healing(repo_healer: RepoHealer) -> BoxFuture<(), Error> {
    let fut = loop_fn((), move |()| {
        let start = Instant::now();
        // TODO(luk) use a proper context here and put the logger inside of it
        let ctx = CoreContext::test_mock();

        repo_healer.heal(ctx).and_then(move |()| {
            let next_iter_deadline = start + MIN_HEALER_ITERATION_DELAY;
            Delay::new(next_iter_deadline)
                .map(|()| Loop::Continue(()))
                .from_err()
        })
    });

    spawn_future(fut).boxify()
}

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    let app = args::MononokeApp {
        safe_writes: true,
        hide_advanced_args: false,
        local_instances: true,
        default_glog: true,
    };
    app.build("blobstore healer job")
        .version("0.0.0")
        .about("Monitors blobstore_sync_queue to heal blobstores with missing data")
        .args_from_usage(
            r#"
            --sync-queue-limit=[LIMIT] 'set limit for how many queue entries to process'
            --dry-run 'performs a single healing and prints what would it do without doing it'
        "#,
        )
}

fn main() -> Result<()> {
    let matches = setup_app().get_matches();

    let logger = args::get_logger(&matches);
    let myrouter_port =
        args::parse_myrouter_port(&matches).ok_or(err_msg("Missing --myrouter-port"))?;

    let rate_limiter = RateLimiter::new(100);

    let repo_configs = args::read_configs(&matches)?;

    let blobstore_sync_queue_limit = value_t!(matches, "sync-queue-limit", usize).unwrap_or(10000);
    let dry_run = matches.is_present("dry-run");

    let healers: Vec<_> = repo_configs
        .repos
        .into_iter()
        .filter_map(move |(name, config)| {
            let logger = logger.new(o!(
                "repo" => format!("{} ({})", name, config.repoid),
            ));

            let scheduled = maybe_schedule_healer_for_repo(
                dry_run,
                blobstore_sync_queue_limit,
                logger.clone(),
                rate_limiter.clone(),
                config,
                myrouter_port,
            );

            match scheduled {
                Err(err) => {
                    error!(logger, "Did not schedule, because of: {:#?}", err);
                    None
                }
                Ok(scheduled) => {
                    info!(logger, "Successfully scheduled");
                    Some(scheduled)
                }
            }
        })
        .collect();

    let mut runtime = tokio::runtime::Runtime::new()?;
    let result = runtime.block_on(join_all(healers).map(|_| ()));
    runtime.shutdown_on_idle();
    result
}
