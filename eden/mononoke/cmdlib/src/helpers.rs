/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs;
use std::future::Future;
use std::io;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use anyhow::bail;
use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use fbinit::FacebookInit;
use futures::future;
use futures::future::Either;
use futures::stream;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use services::Fb303Service;
use slog::error;
use slog::info;
use slog::Logger;
use tokio::io::AsyncBufReadExt;
use tokio::runtime::Handle;
use tokio::runtime::Runtime;
use tokio::signal::unix::signal;
use tokio::signal::unix::SignalKind;
use tokio::time;

use crate::args::MononokeMatches;
use crate::monitoring;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bookmarks::BookmarkName;
use bookmarks::BookmarksRef;
use context::CoreContext;
use mercurial_derived_data::DeriveHgChangeset;
use mercurial_types::HgChangesetId;
use mercurial_types::HgManifestId;
use mononoke_types::ChangesetId;
use repo_identity::RepoIdentityRef;
use stats::schedule_stats_aggregation_preview;

pub const ARG_SHUTDOWN_GRACE_PERIOD: &str = "shutdown-grace-period";
pub const ARG_FORCE_SHUTDOWN_PERIOD: &str = "force-shutdown-period";

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
    #[allow(clippy::single_element_loop)]
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

/// Resolve changeset id by either bookmark name, hg hash, or changset id hash
pub async fn csid_resolve(
    ctx: &CoreContext,
    container: impl RepoIdentityRef + BonsaiHgMappingRef + BookmarksRef,
    hash_or_bookmark: impl ToString,
) -> Result<ChangesetId, Error> {
    let res = csid_resolve_impl(ctx, &container, hash_or_bookmark).await;
    if let Ok(csid) = &res {
        info!(ctx.logger(), "changeset resolved as: {:?}", csid);
    }
    res
}

/// Read and resolve a list of changeset from file
pub async fn csids_resolve_from_file(
    ctx: &CoreContext,
    container: impl RepoIdentityRef + BonsaiHgMappingRef + BookmarksRef,
    filename: &str,
) -> Result<Vec<ChangesetId>, Error> {
    let file = tokio::fs::File::open(filename).await?;
    let file = tokio::io::BufReader::new(file);
    let mut lines = file.lines();
    let mut csids = vec![];
    while let Some(line) = lines.next_line().await? {
        csids.push(line);
    }

    stream::iter(csids)
        .map(|csid| csid_resolve_impl(ctx, &container, csid))
        .buffered(100)
        .try_collect::<Vec<_>>()
        .await
}

/// Resolve changeset id by either bookmark name, hg hash, or changset id hash
async fn csid_resolve_impl<C>(
    ctx: &CoreContext,
    container: &C,
    hash_or_bookmark: impl ToString,
) -> Result<ChangesetId, Error>
where
    C: BonsaiHgMappingRef + BookmarksRef,
{
    let hash_or_bookmark = hash_or_bookmark.to_string();
    if let Ok(name) = BookmarkName::new(hash_or_bookmark.clone()) {
        if let Some(cs_id) = container.bookmarks().get(ctx.clone(), &name).await? {
            return Ok(cs_id);
        }
    }
    if let Ok(hg_id) = HgChangesetId::from_str(&hash_or_bookmark) {
        if let Some(cs_id) = container
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(ctx, hg_id)
            .await?
        {
            return Ok(cs_id);
        }
    }
    if let Ok(cs_id) = ChangesetId::from_str(&hash_or_bookmark) {
        return Ok(cs_id);
    }
    bail!(
        "invalid (hash|bookmark) or does not exist in this repository: {}",
        hash_or_bookmark
    )
}

pub async fn get_root_manifest_id(
    ctx: &CoreContext,
    repo: BlobRepo,
    hash_or_bookmark: impl ToString,
) -> Result<HgManifestId, Error> {
    let cs_id = csid_resolve(ctx, &repo, hash_or_bookmark).await?;
    Ok(repo
        .derive_hg_changeset(ctx, cs_id)
        .await?
        .load(ctx, repo.blobstore())
        .await?
        .manifestid())
}

/// Get a tokio `Runtime` with potentially explicitly set number of core threads
pub fn create_runtime(
    log_thread_name_prefix: Option<&str>,
    worker_threads: Option<usize>,
) -> io::Result<Runtime> {
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder.enable_all();
    builder.thread_name(log_thread_name_prefix.unwrap_or("tk"));
    if let Some(worker_threads) = worker_threads {
        builder.worker_threads(worker_threads);
    }
    builder.build()
}

/// Starts a future as a server, and waits until a termination signal is received.
///
/// When the termination signal is received, the `quiesce` callback is
/// called.  This should perform any steps required to quiesce the
/// server.  Requests should still be accepted.
///
/// After the configured quiesce timeout, the `shutdown` future is awaited.
/// This should do any additional work to stop accepting connections and wait
/// until all outstanding requests have been handled. The `server` future
/// continues to run while `shutdown` is being awaited.
///
/// Once `shutdown` returns, the `server` future is cancelled, and the process
/// exits. If `shutdown_timeout` is exceeded, an error is returned.
pub async fn serve_forever_async<Server, QuiesceFn, ShutdownFut>(
    server: Server,
    logger: &Logger,
    quiesce: QuiesceFn,
    shutdown_grace_period: Duration,
    shutdown: ShutdownFut,
    shutdown_timeout: Duration,
) -> Result<(), Error>
where
    Server: Future<Output = Result<(), Error>> + Send + 'static,
    QuiesceFn: FnOnce(),
    ShutdownFut: Future<Output = ()>,
{
    // We want to prevent Folly's signal handlers overriding our
    // intended action with a termination signal. Mononoke server,
    // in particular, depends on this - otherwise our attempts to
    // catch and handle SIGTERM turn into Folly backtracing and killing us.
    unsafe {
        libc::signal(libc::SIGTERM, libc::SIG_DFL);
    }

    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;

    let terminate = terminate.recv();
    let interrupt = interrupt.recv();
    futures::pin_mut!(terminate, interrupt);

    // This future becomes ready when we receive a termination signal
    let signalled = future::select(terminate, interrupt);

    let stats_agg = schedule_stats_aggregation_preview()
        .map_err(|_| Error::msg("Failed to create stats aggregation worker"))?;
    // Note: this returns a JoinHandle, which we drop, thus detaching the task
    // It thus does not count towards shutdown_on_idle below
    tokio::task::spawn(stats_agg);

    // Spawn the server onto its own task
    let server_handle = tokio::task::spawn(server);

    // Now wait for the termination signal, or a server exit.
    let server_result: Result<(), Error> = match future::select(server_handle, signalled).await {
        Either::Left((join_handle_res, _)) => {
            let res = join_handle_res.map_err(Error::from).and_then(|res| res);
            match res.as_ref() {
                Ok(()) => {
                    error!(&logger, "Server has exited! Starting shutdown...");
                }
                Err(e) => {
                    error!(
                        &logger,
                        "Server exited with an error! Starting shutdown... Error: {:?}", e
                    );
                }
            }
            res
        }
        Either::Right(..) => {
            info!(&logger, "Signalled! Starting shutdown...");
            Ok(())
        }
    };

    // Shutting down: wait for the grace period.
    quiesce();
    info!(
        &logger,
        "Waiting {}s before shutting down server",
        shutdown_grace_period.as_secs(),
    );

    time::sleep(shutdown_grace_period).await;

    info!(&logger, "Shutting down...");
    let () = time::timeout(shutdown_timeout, shutdown)
        .map_err(|_| Error::msg("Timed out shutting down server"))
        .await?;

    server_result
}

/// Same as "serve_forever_async", but blocks using the provided runtime,
/// for compatibility with existing sync code using it.
pub fn serve_forever<Server, QuiesceFn, ShutdownFut>(
    handle: &Handle,
    server: Server,
    logger: &Logger,
    quiesce: QuiesceFn,
    shutdown_grace_period: Duration,
    shutdown: ShutdownFut,
    shutdown_timeout: Duration,
) -> Result<(), Error>
where
    Server: Future<Output = Result<(), Error>> + Send + 'static,
    QuiesceFn: FnOnce(),
    ShutdownFut: Future<Output = ()>,
{
    handle.block_on(serve_forever_async(
        server,
        logger,
        quiesce,
        shutdown_grace_period,
        shutdown,
        shutdown_timeout,
    ))
}

/// Executes the future and waits for it to finish.
pub fn block_execute<F, Out, S: Fb303Service + Sync + Send + 'static>(
    future: F,
    fb: FacebookInit,
    app_name: &str,
    logger: &Logger,
    matches: &MononokeMatches,
    service: S,
) -> Result<Out, Error>
where
    F: Future<Output = Result<Out, Error>>,
{
    block_execute_impl(future, fb, app_name, logger, matches, service)
}

/// Executes the future and waits for it to finish.
fn block_execute_impl<F, Out, S: Fb303Service + Sync + Send + 'static>(
    future: F,
    fb: FacebookInit,
    app_name: &str,
    logger: &Logger,
    matches: &MononokeMatches,
    service: S,
) -> Result<Out, Error>
where
    F: Future<Output = Result<Out, Error>>,
{
    monitoring::start_fb303_server(fb, app_name, logger, matches, service)?;

    let result = matches.runtime().block_on(async {
        #[cfg(not(test))]
        {
            let stats_agg = schedule_stats_aggregation_preview()
                .map_err(|_| Error::msg("Failed to create stats aggregation worker"))?;
            // Note: this returns a JoinHandle, which we drop, thus detaching the task
            // It thus does not count towards shutdown_on_idle below
            tokio::task::spawn(stats_agg);
        }

        future.await
    });

    // Log error in glog format (main will log, but not with glog)
    result.map_err(move |e| {
        error!(logger, "Execution error: {:?}", e);
        // Shorten the error that main will print, given that already printed in glog form
        format_err!("Execution failed")
    })
}
