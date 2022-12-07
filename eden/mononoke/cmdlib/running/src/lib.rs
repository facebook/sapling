/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::future::Future;
use std::time::Duration;

use anyhow::Error;
use anyhow::Result;
use futures::future;
use futures::future::Either;
use futures::TryFutureExt;
use slog::error;
use slog::info;
use slog::Logger;
use tokio::signal::unix::signal;
use tokio::signal::unix::SignalKind;
use tokio::time;

/// Run a server future, and wait until a termination signal is received.
///
/// When the termination signal is received, the `quiesce` callback is called.
/// This should perform any steps required to quiesce the server, for example
/// by removing this instance from routing configuration, or asking the load
/// balancer to stop sending requests to this instance.  Requests that do
/// arrive should still be accepted.
///
/// After the configured quiesce timeout, the `shutdown` future is awaited.
/// This should do any additional work to stop accepting connections and wait
/// until all outstanding requests have been handled. The `server` future
/// continues to run while `shutdown` is being awaited.
///
/// Once both `shutdown` and `server` have completed, the process
/// exits. If `shutdown_timeout` is exceeded, the server process is canceled
/// and an error is returned.
pub async fn run_until_terminated<Server, QuiesceFn, ShutdownFut>(
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

    // Spawn the server onto its own task
    let server_handle = tokio::task::spawn(server);

    // Now wait for the termination signal, or a server exit.
    let server_result_or_handle = match future::select(server_handle, signalled).await {
        Either::Left((server_result, _)) => {
            let server_result = server_result.map_err(Error::from).and_then(|res| res);
            match server_result.as_ref() {
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
            Either::Left(server_result)
        }
        Either::Right((_, server_handle)) => {
            info!(&logger, "Signalled! Starting shutdown...");
            Either::Right(server_handle)
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

    let shutdown = async move {
        shutdown.await;
        match server_result_or_handle {
            Either::Left(server_result) => server_result,
            Either::Right(server_handle) => server_handle.await?,
        }
    };

    info!(&logger, "Shutting down...");
    time::timeout(shutdown_timeout, shutdown)
        .map_err(|_| Error::msg("Timed out shutting down server"))
        .await?
}
