/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::{cmp::min, fs, future::Future, io, path::Path, str::FromStr, thread, time::Duration};

use anyhow::{bail, format_err, Context, Error, Result};
use clap::ArgMatches;
use cloned::cloned;
use fbinit::FacebookInit;
use futures::sync::oneshot::Receiver;
use futures::{future as old_future, sync, Future as OldFuture, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};
use panichandler::Fate;
use signal_hook::{iterator::Signals, SIGINT, SIGTERM};
use slog::{debug, error, info, Logger};
use upload_trace::{manifold_thrift::thrift::RequestContext, UploadTrace};

use crate::args;
use crate::monitoring;
use blobrepo::BlobRepo;
use blobrepo_factory::ReadOnlyStorage;
use bookmarks::BookmarkName;
use changesets::SqlConstructors;
use context::CoreContext;
use mercurial_types::{HgChangesetId, HgManifestId};
use metaconfig_types::MetadataDBConfig;
use mononoke_types::ChangesetId;
use sql_ext::MysqlOptions;
use stats::{schedule_stats_aggregation, schedule_stats_aggregation_preview};

pub const ARG_SHUTDOWN_GRACE_PERIOD: &str = "shutdown-grace-period";
pub const ARG_FORCE_SHUTDOWN_PERIOD: &str = "force-shutdown-period";

pub fn upload_and_show_trace(ctx: CoreContext) -> impl OldFuture<Item = (), Error = !> {
    if !ctx.trace().is_enabled() {
        debug!(ctx.logger(), "Trace is disabled");
        return Ok(()).into_future().left_future();
    }

    let rc = RequestContext {
        bucketName: "mononoke_prod".into(),
        apiKey: "".into(),
        ..Default::default()
    };

    ctx.trace()
        .upload_to_manifold(rc)
        .then(move |upload_res| {
            match upload_res {
                Err(err) => debug!(ctx.logger(), "Failed to upload trace: {:#?}", err),
                Ok(()) => debug!(
                    ctx.logger(),
                    "Trace taken: https://our.intern.facebook.com/intern/mononoke/trace/{}",
                    ctx.trace().id()
                ),
            }
            Ok(())
        })
        .right_future()
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CreateStorage {
    ExistingOnly,
    ExistingOrCreate,
}

pub fn setup_repo_dir<P: AsRef<Path>>(data_dir: P, create: CreateStorage) -> Result<()> {
    let data_dir = data_dir.as_ref();

    if !data_dir.is_dir() {
        bail!("{:?} does not exist or is not a directory", data_dir);
    }

    // Validate directory layout
    for subdir in &["blobs"] {
        let subdir = data_dir.join(subdir);

        if subdir.exists() && !subdir.is_dir() {
            bail!("{:?} already exists and is not a directory", subdir);
        }

        if !subdir.exists() {
            if CreateStorage::ExistingOnly == create {
                bail!("{:?} not found in ExistingOnly mode", subdir,);
            }
            fs::create_dir(&subdir)
                .with_context(|| format!("failed to create subdirectory {:?}", subdir))?;
        }
    }
    Ok(())
}

pub struct CachelibSettings {
    pub cache_size: usize,
    pub max_process_size_gib: Option<u32>,
    pub min_process_size_gib: Option<u32>,
    pub use_tupperware_shrinker: bool,
    pub presence_cache_size: Option<usize>,
    pub changesets_cache_size: Option<usize>,
    pub filenodes_cache_size: Option<usize>,
    pub idmapping_cache_size: Option<usize>,
    pub with_content_sha1_cache: bool,
    pub content_sha1_cache_size: Option<usize>,
    pub blob_cache_size: Option<usize>,
}

impl Default for CachelibSettings {
    fn default() -> Self {
        Self {
            cache_size: 20 * 1024 * 1024 * 1024,
            max_process_size_gib: None,
            min_process_size_gib: None,
            use_tupperware_shrinker: false,
            presence_cache_size: None,
            changesets_cache_size: None,
            filenodes_cache_size: None,
            idmapping_cache_size: None,
            with_content_sha1_cache: false,
            content_sha1_cache_size: None,
            blob_cache_size: None,
        }
    }
}

pub fn init_cachelib_from_settings(fb: FacebookInit, settings: CachelibSettings) -> Result<()> {
    // Millions of lookups per second
    let lock_power = 10;
    // Assume 200 bytes average cache item size and compute bucketsPower
    let expected_item_size_bytes = 200;
    let cache_size_bytes = settings.cache_size;
    let item_count = cache_size_bytes / expected_item_size_bytes;

    // Because `bucket_count` is a power of 2, bucket_count.trailing_zeros() is log2(bucket_count)
    let bucket_count = item_count
        .checked_next_power_of_two()
        .ok_or_else(|| Error::msg("Cache has too many objects to fit a `usize`?!?"))?;
    let buckets_power = min(bucket_count.trailing_zeros() + 1 as u32, 32);

    let mut cache_config = cachelib::LruCacheConfig::new(cache_size_bytes)
        .set_pool_rebalance(cachelib::PoolRebalanceConfig {
            interval: Duration::new(300, 0),
            strategy: cachelib::RebalanceStrategy::HitsPerSlab {
                // A small increase in hit ratio is desired
                diff_ratio: 0.05,
                min_retained_slabs: 1,
                // Objects newer than 30 seconds old might be about to become interesting
                min_tail_age: Duration::new(30, 0),
                ignore_untouched_slabs: false,
            },
        })
        .set_access_config(buckets_power, lock_power);

    if settings.use_tupperware_shrinker {
        if settings.max_process_size_gib.is_some() || settings.min_process_size_gib.is_some() {
            bail!("Can't use both Tupperware shrinker and manually configured shrinker");
        }
        cache_config = cache_config.set_tupperware_shrinker();
    } else {
        match (settings.max_process_size_gib, settings.min_process_size_gib) {
            (None, None) => (),
            (Some(_), None) | (None, Some(_)) => {
                bail!("If setting process size limits, must set both max and min");
            }
            (Some(max), Some(min)) => {
                cache_config = cache_config.set_shrinker(cachelib::ShrinkMonitor {
                    shrinker_type: cachelib::ShrinkMonitorType::ResidentSize {
                        max_process_size_gib: max,
                        min_process_size_gib: min,
                    },
                    interval: Duration::new(10, 0),
                    max_resize_per_iteration_percent: 25,
                    max_removed_percent: 50,
                    strategy: cachelib::RebalanceStrategy::HitsPerSlab {
                        // A small increase in hit ratio is desired
                        diff_ratio: 0.05,
                        min_retained_slabs: 1,
                        // Objects newer than 30 seconds old might be about to become interesting
                        min_tail_age: Duration::new(30, 0),
                        ignore_untouched_slabs: false,
                    },
                });
            }
        };
    }

    cachelib::init_cache_once(fb, cache_config)?;
    cachelib::init_cacheadmin("mononoke")?;

    // Give each cache 5% of the available space, bar the blob cache which gets everything left
    // over. We can adjust this with data.
    let available_space = cachelib::get_available_space()?;
    cachelib::get_or_create_volatile_pool(
        "blobstore-presence",
        settings.presence_cache_size.unwrap_or(available_space / 20),
    )?;

    cachelib::get_or_create_volatile_pool(
        "changesets",
        settings
            .changesets_cache_size
            .unwrap_or(available_space / 20),
    )?;
    cachelib::get_or_create_volatile_pool(
        "filenodes",
        settings
            .filenodes_cache_size
            .unwrap_or(available_space / 20),
    )?;
    cachelib::get_or_create_volatile_pool(
        "bonsai_hg_mapping",
        settings
            .idmapping_cache_size
            .unwrap_or(available_space / 20),
    )?;

    if settings.with_content_sha1_cache {
        cachelib::get_or_create_volatile_pool(
            "content-sha1",
            settings
                .content_sha1_cache_size
                .unwrap_or(available_space / 20),
        )?;
    }

    cachelib::get_or_create_volatile_pool(
        "blobstore-blobs",
        settings
            .blob_cache_size
            .unwrap_or(cachelib::get_available_space()?),
    )?;

    Ok(())
}

/// Resovle changeset id by either bookmark name, hg hash, or changset id hash
pub fn csid_resolve(
    ctx: CoreContext,
    repo: BlobRepo,
    hash_or_bookmark: impl ToString,
) -> impl OldFuture<Item = ChangesetId, Error = Error> {
    let hash_or_bookmark = hash_or_bookmark.to_string();
    BookmarkName::new(hash_or_bookmark.clone())
        .into_future()
        .and_then({
            cloned!(repo, ctx);
            move |name| repo.get_bonsai_bookmark(ctx, &name)
        })
        .and_then(|csid| csid.ok_or(Error::msg("invalid bookmark")))
        .or_else({
            cloned!(ctx, repo, hash_or_bookmark);
            move |_| {
                HgChangesetId::from_str(&hash_or_bookmark)
                    .into_future()
                    .and_then(move |hg_csid| repo.get_bonsai_from_hg(ctx, hg_csid))
                    .and_then(|csid| csid.ok_or(Error::msg("invalid hg changeset")))
            }
        })
        .or_else({
            cloned!(hash_or_bookmark);
            move |_| ChangesetId::from_str(&hash_or_bookmark)
        })
        .inspect(move |csid| {
            info!(ctx.logger(), "changeset resolved as: {:?}", csid);
        })
        .map_err(move |_| {
            format_err!(
                "invalid (hash|bookmark) or does not exist in this repository: {}",
                hash_or_bookmark
            )
        })
}

pub fn get_root_manifest_id(
    ctx: CoreContext,
    repo: BlobRepo,
    hash_or_bookmark: impl ToString,
) -> impl OldFuture<Item = HgManifestId, Error = Error> {
    csid_resolve(ctx.clone(), repo.clone(), hash_or_bookmark).and_then(move |bcs_id| {
        repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id)
            .and_then({
                cloned!(ctx, repo);
                move |hg_cs_id| repo.get_changeset_by_changesetid(ctx.clone(), hg_cs_id)
            })
            .map(|cs| cs.manifestid())
    })
}

pub fn open_sql_with_config_and_mysql_options<T>(
    fb: FacebookInit,
    dbconfig: MetadataDBConfig,
    mysql_options: MysqlOptions,
    readonly_storage: ReadOnlyStorage,
) -> BoxFuture<T, Error>
where
    T: SqlConstructors,
{
    let name = T::LABEL;
    match dbconfig {
        MetadataDBConfig::LocalDB { path } => {
            T::with_sqlite_path(path.join("sqlite_dbs"), readonly_storage.0)
                .into_future()
                .boxify()
        }
        MetadataDBConfig::Mysql { db_address, .. } if name != "filenodes" => {
            T::with_xdb(fb, db_address, mysql_options, readonly_storage.0)
        }
        MetadataDBConfig::Mysql { .. } => Err(Error::msg(
            "Use SqlFilenodes::with_sharded_myrouter for filenodes",
        ))
        .into_future()
        .boxify(),
    }
}

/// Get a tokio `Runtime` with potentially explicitly set number of core threads
pub fn create_runtime(
    log_thread_name_prefix: Option<&str>,
    core_threads: Option<usize>,
) -> io::Result<tokio_compat::runtime::Runtime> {
    let mut builder = tokio_compat::runtime::Builder::new();
    builder.name_prefix(log_thread_name_prefix.unwrap_or("tk-"));
    if let Some(core_threads) = core_threads {
        builder.core_threads(core_threads);
    }
    builder.build()
}

/// Starts a future as a server, and waits until a termination signal is received.
///
/// When the termination signal is received, the `quiesce` callback is
/// called.  This should perform any steps required to quiesce the
/// server.  Requests should still be accepted.
///
/// After the configured quiesce timeout, the `server` future is
/// cancelled, and the `shutdown` callback is called.  This should do
/// any additional work to stop accepting connections and wait until all
/// outstanding requests have been handled.
///
/// Currently the `shutdown` callback can return `true` to indicate that
/// the runtime should wait until it is idle before shutting down.  Note
/// that this option will be removed in Tokio 0.2.
///
/// When `shutdown` completes, or when the force shutdown timer expires, the
/// runtime will be shutdown and the process will exit.
pub fn serve_forever<Server, QuiesceFn, ShutdownFn>(
    runtime: tokio_compat::runtime::Runtime,
    server: Server,
    logger: &Logger,
    quiesce: QuiesceFn,
    shutdown_grace_period: Duration,
    shutdown: ShutdownFn,
    shutdown_timeout: Duration,
) -> Result<(), Error>
where
    Server: OldFuture<Item = (), Error = ()> + Send + 'static,
    QuiesceFn: FnOnce(),
    ShutdownFn: FnOnce() -> bool,
{
    // Block until receiving a signal that tells us to exit.
    let block = || -> Result<(), Error> {
        let signals = Signals::new(&[SIGTERM, SIGINT])?;
        for signal in signals.forever() {
            info!(&logger, "Signalled: {}", signal);
            break;
        }
        Ok(())
    };
    block_on_fn(
        runtime,
        server,
        logger,
        block,
        quiesce,
        shutdown_grace_period,
        shutdown,
        shutdown_timeout,
    )?;

    Ok(())
}

pub fn block_on_fn<Server, QuiesceFn, ShutdownFn, BlockFn>(
    runtime: tokio_compat::runtime::Runtime,
    server: Server,
    logger: &Logger,
    block: BlockFn,
    quiesce: QuiesceFn,
    shutdown_grace_period: Duration,
    shutdown: ShutdownFn,
    shutdown_timeout: Duration,
) -> Result<(), Error>
where
    Server: OldFuture<Item = (), Error = ()> + Send + 'static,
    QuiesceFn: FnOnce(),
    ShutdownFn: FnOnce() -> bool,
    BlockFn: FnOnce() -> Result<(), Error>,
{
    let (shutdown_pub, shutdown_sub) = sync::oneshot::channel::<()>();
    let main = join_stats_agg(server, shutdown_sub)?;
    runtime.spawn(main);

    block()?;

    // Shutting down: wait for the grace period.
    quiesce();
    info!(
        &logger,
        "Waiting {}s before shutting down server",
        shutdown_grace_period.as_secs(),
    );
    thread::sleep(shutdown_grace_period);

    info!(&logger, "Shutting down...");
    let _ = shutdown_pub.send(());

    // Create a background thread to panic if we fail to shutdown within the timeout.
    panichandler::set_panichandler(Fate::Abort);
    thread::spawn(move || {
        thread::sleep(shutdown_timeout);
        panic!("Timed out shutting down runtime");
    });

    if shutdown() {
        runtime
            .shutdown_on_idle()
            .wait()
            .map_err(|_| Error::msg("Failed to shutdown runtime!"))?;
    } else {
        runtime
            .shutdown_now()
            .wait()
            .map_err(|_| Error::msg("Failed to shutdown runtime!"))?;
    }

    Ok(())
}

/// Executes the future and waits for it to finish.
pub fn block_execute<F, Out>(
    future: F,
    fb: FacebookInit,
    app_name: &str,
    logger: &Logger,
    matches: &ArgMatches,
) -> Result<Out, Error>
where
    F: Future<Output = Result<Out, Error>> + Send + 'static,
{
    monitoring::start_fb303_server(fb, app_name, logger, matches)?;
    let mut runtime = args::init_runtime(&matches)?;

    let result = runtime.block_on_std(async {
        let stats_agg = schedule_stats_aggregation_preview()
            .map_err(|_| Error::msg("Failed to create stats aggregation worker"))?;
        // Note: this returns a JoinHandle, which we drop, thus detaching the task
        // It thus does not count towards shutdown_on_idle below
        tokio_preview::task::spawn(stats_agg);

        future.await
    });

    // Only needed while we have a compat runtime - this waits for futures without
    // a handle to stop
    runtime.shutdown_on_idle();

    match &result {
        Ok(_) => info!(logger, "Execution succeeded"),
        Err(e) => error!(logger, "Execution error: {:?}", e),
    };
    result
}

/// Join the future with stats aggregator and return a new joined future
fn join_stats_agg<F>(future: F, shutdown_sub: Receiver<()>) -> Result<BoxFuture<(), ()>, Error>
where
    F: OldFuture<Item = (), Error = ()> + Send + 'static,
{
    let stats_agg = schedule_stats_aggregation()
        .map_err(|_| Error::msg("Failed to create stats aggregation worker"))?
        .discard();

    let main = (stats_agg, future)
        .into_future()
        .select2(shutdown_sub)
        .then({
            move |res| -> Result<(), ()> {
                match res {
                    Ok(old_future::Either::B(_)) => Ok(()),
                    Err(old_future::Either::A(_)) => Err(()),
                    _ => {
                        // NOTE: We need to panic here, because otherwise main is going to be blocked on
                        // waiting for a signal forever. This shouldn't normally ever happen.
                        unreachable!("Terminated or signal listener was dropped.")
                    }
                }
            }
        });

    Ok(main.boxify())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::args;
    use anyhow::Error;
    use futures_preview::future::lazy;
    use slog_glog_fmt;
    use tokio_timer::sleep;

    fn create_logger() -> Logger {
        slog_glog_fmt::facebook_logger().unwrap()
    }

    fn exec_matches<'a>() -> ArgMatches<'a> {
        let app = args::MononokeApp::new("test_app").build();
        let arg_vec = vec!["test_prog", "--mononoke-config-path", "/tmp/testpath"];
        args::add_fb303_args(app).get_matches_from(arg_vec)
    }

    #[fbinit::test]
    fn test_block_execute_success(fb: FacebookInit) {
        let future = lazy({ |_| -> Result<(), Error> { Ok(()) } });
        let logger = create_logger();
        let matches = exec_matches();
        let res = block_execute(future, fb, "test_app", &logger, &matches);
        assert!(res.is_ok());
    }

    #[fbinit::test]
    fn test_block_execute_error(fb: FacebookInit) {
        let future = lazy({ |_| -> Result<(), Error> { Err(Error::msg("Some error")) } });
        let logger = create_logger();
        let matches = exec_matches();
        let res = block_execute(future, fb, "test_app", &logger, &matches);
        assert!(res.is_err());
    }

    #[test]
    fn test_block_on_fn_shutsdown() {
        let logger = create_logger();
        let matches = exec_matches();
        let runtime = args::init_runtime(&matches).unwrap();
        let server = sleep(Duration::from_secs(42)).discard();
        block_on_fn(
            runtime,
            server,
            &logger,
            || -> Result<(), Error> { Ok(()) },
            || (),
            Duration::from_secs(0),
            || true,
            Duration::from_secs(10),
        )
        .unwrap();
    }
}
