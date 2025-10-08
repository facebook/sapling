/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Debug;
use std::fmt::Display;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use async_trait::async_trait;
use edenfs_error::ConnectAndRequestError;
use edenfs_error::ErrorHandlingStrategy;
use edenfs_error::HasErrorHandlingStrategy;
use edenfs_error::Result;
use edenfs_telemetry::EdenSample;
use edenfs_telemetry::QueueingScubaLogger;
use edenfs_telemetry::SampleLogger;
use edenfs_telemetry::create_logger;
use fbinit::FacebookInit;
use futures_stats::TimedTryFutureExt;
use lazy_static::lazy_static;
use parking_lot::Mutex;
use rand::Rng;
use rand::distributions::Alphanumeric;
use rand::thread_rng;
use tokio::sync::Semaphore;

use crate::client::Client;
use crate::client::EdenFsClientStatsHandler;
use crate::client::EdenFsConnection;
use crate::client::NoopEdenFsClientStatsHandler;
use crate::client::connector::Connector;
use crate::client::connector::StreamingEdenFsConnector;
use crate::client::connector::StreamingEdenFsThriftClientFuture;
use crate::methods::EdenThriftMethod;
use crate::use_case::UseCase;

lazy_static! {
    static ref SCUBA_CLIENT: QueueingScubaLogger =
        QueueingScubaLogger::new(create_logger("edenfs_client".to_string()), 1000);
}

// Number of attempts to make for a given Thrift request before giving up.
const MAX_RETRY_ATTEMPTS: usize = 3;

/// A client for interacting with the EdenFS Thrift service.
///
/// `ThriftClient` provides methods for communicating with the EdenFS Thrift service, allowing you to
/// perform operations such as querying mount points, checking daemon status, and managing
/// checkouts.
///
/// This is the core client implementation that handles connections, retries, and error handling.
///
/// The client automatically handles:
/// - Connection management and reconnection if EdenFS restarts
/// - Request retries based on error types
/// - Concurrency limiting to prevent overloading the EdenFS server
#[allow(dead_code)]
pub struct ThriftClient {
    use_case: Arc<UseCase>,
    connector: StreamingEdenFsConnector,
    connection: Mutex<EdenFsConnection<StreamingEdenFsThriftClientFuture>>,
    stats_handler: Box<dyn EdenFsClientStatsHandler + Send + Sync>,
    session_id: String,
    /// Eden has limits on concurrency and will return server overloaded (or timeout) errors if we
    /// send too many. Experimentally, even for large builds (see details in D36136516), we don't
    /// get much performance improvement beyond 2K concurrent requests, regardless of whether Eden
    /// has a fast or slow connection to source control, a warm cache or not, and a lot of CPU
    /// available to run or not.
    semaphore: Semaphore,
}

#[async_trait]
impl Client for ThriftClient {
    fn new(fb: FacebookInit, use_case: Arc<UseCase>, socket_file: PathBuf) -> Self {
        let connector = StreamingEdenFsConnector::new(fb, socket_file.clone());
        let connection = Mutex::new(EdenFsConnection {
            epoch: 0,
            client: connector.connect(None, None),
        });

        Self {
            use_case: use_case.clone(),
            connector,
            connection,
            stats_handler: Box::new(NoopEdenFsClientStatsHandler {}),
            semaphore: Semaphore::new(use_case.max_concurrent_requests()),
            session_id: generate_id(),
        }
    }

    fn set_stats_handler(
        &mut self,
        stats_handler: Box<dyn EdenFsClientStatsHandler + Send + Sync>,
    ) {
        self.stats_handler = stats_handler;
    }

    async fn with_thrift_with_timeouts<F, Fut, T, E>(
        &self,
        conn_timeout: Option<Duration>,
        recv_timeout: Option<Duration>,
        f: F,
    ) -> std::result::Result<T, ConnectAndRequestError<E>>
    where
        F: Fn(&<StreamingEdenFsConnector as Connector>::Client) -> (Fut, EdenThriftMethod)
            + Send
            + Sync,
        Fut: Future<Output = Result<T, E>> + Send,
        T: Send,
        E: HasErrorHandlingStrategy + Debug + Display,
    {
        // Acquire a permit from the semaphore. This will block if we have too many concurrent requests.
        let _permit = self
            .semaphore
            .acquire()
            .await
            .expect("Eden I/O semaphore is never closed");

        let mut connection = (*self.connection.lock()).clone();
        let mut attempts = 0;
        let mut retries = 0;
        let mut sample = EdenSample::new();
        loop {
            attempts += 1;
            let start = Instant::now();
            let result = async {
                let client = connection.client.clone().await.map_err(|e| {
                    (
                        ConnectAndRequestError::ConnectionError(e),
                        EdenThriftMethod::Unknown,
                    )
                })?;
                let (fut, method) = f(&client);
                fut.await
                    .map(|res| (res, method))
                    .map_err(|e| (ConnectAndRequestError::RequestError(e), method))
            }
            .try_timed()
            .await;
            sample.add_int("wall_clock_duration_us", start.elapsed().as_micros() as i64);
            sample.add_int("attempts", attempts as i64);
            sample.add_int("retries", retries as i64);
            sample.add_string("use_case", self.use_case.name());
            sample.add_string("session_id", &self.session_id);
            sample.add_string("request_id", generate_id().as_str());
            sample.add_string("user", whoami::username());
            sample.add_string("host", whoami::fallible::hostname().unwrap_or_default());
            let (error, method) = match result {
                Ok((stats, (result, method))) => {
                    self.stats_handler.on_success(attempts, retries);
                    sample.add_int("success", true as i64);
                    sample.add_int("duration_us", stats.completion_time.as_micros() as i64);
                    sample.add_string("method", method.name());
                    let _ = SCUBA_CLIENT.log(sample); // Ideally log should be infalliable, but since its not we don't want to fail the request
                    break Ok(result);
                }
                Err(e) => e,
            };
            sample.add_string("method", method.name());
            match error.get_error_handling_strategy() {
                ErrorHandlingStrategy::Reconnect => {
                    // Our connection to EdenFS broke.
                    // This typically means Eden restarted. Just reconnect.
                    tracing::info!(
                        "Reconnecting ({}/{} attempts) to EdenFS after: {:#}",
                        attempts,
                        MAX_RETRY_ATTEMPTS,
                        error
                    );
                    let mut guard = self.connection.lock();
                    if guard.epoch == connection.epoch {
                        guard.client = self.connector.connect(conn_timeout, recv_timeout);
                        guard.epoch += 1;
                    }
                    connection = (*guard).clone();
                }
                ErrorHandlingStrategy::Retry => {
                    // Our request failed but needs retrying.
                    retries += 1;
                    tracing::info!(
                        "Retrying ({}/{} attempts) EdenFS request after: {:#}",
                        attempts,
                        MAX_RETRY_ATTEMPTS,
                        error
                    );
                }
                ErrorHandlingStrategy::Abort => {
                    sample.fail(format!("{:?}", error).as_str());
                    let _ = SCUBA_CLIENT.log(sample);
                    break Err(error);
                }
            };

            if attempts > MAX_RETRY_ATTEMPTS {
                sample.fail(format!("{:?}", error).as_str());
                sample.add_bool("max_retry_reached", true);
                let _ = SCUBA_CLIENT.log(sample);
                break Err(error);
            }
        }
    }
}

fn generate_id() -> String {
    thread_rng()
        .sample_iter(Alphanumeric)
        .take(8)
        .map(char::from)
        .collect()
}
