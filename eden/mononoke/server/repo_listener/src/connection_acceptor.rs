/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use hostname::get_hostname;
use hyper::server::conn::Http;
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use bytes::Bytes;
use cached_config::ConfigStore;
use connection_security_checker::ConnectionSecurityChecker;
use edenapi_service::EdenApi;
use failure_ext::SlogKVError;
use fbinit::FacebookInit;
use futures::channel::oneshot;
use futures::future::Future;
use futures::select_biased;
use futures_01_ext::BoxStream;
use futures_ext::FbFutureExt;
use futures_old::stream;
use futures_old::sync::mpsc;
use futures_old::Stream;
use futures_util::compat::Stream01CompatExt;
use futures_util::future::AbortHandle;
use futures_util::future::FutureExt;
use futures_util::stream::StreamExt;
use futures_util::stream::TryStreamExt;
use lazy_static::lazy_static;
use metaconfig_types::CommonConfig;
use openssl::ssl::Ssl;
use openssl::ssl::SslAcceptor;
use permission_checker::AclProvider;
use permission_checker::MononokeIdentity;
use permission_checker::MononokeIdentitySet;
use rate_limiting::RateLimitEnvironment;
use scribe_ext::Scribe;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::debug;
use slog::error;
use slog::info;
use slog::warn;
use slog::Logger;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio_openssl::SslStream;
use tokio_util::codec::FramedRead;
use tokio_util::codec::FramedWrite;

use cmdlib::monitoring::ReadyFlagService;
use metadata::Metadata;
use qps::Qps;
use quiet_stream::QuietShutdownStream;
use sshrelay::IoStream;
use sshrelay::SshDecoder;
use sshrelay::SshEncoder;
use sshrelay::SshMsg;
use sshrelay::Stdio;
use stats::prelude::*;

use crate::errors::ErrorKind;
use crate::http_service::MononokeHttpService;
use crate::repo_handlers::RepoHandler;
use crate::request_handler::create_conn_logger;
use crate::request_handler::request_handler;
use crate::wireproto_sink::WireprotoSink;

define_stats! {
    prefix = "mononoke.connection_acceptor";
    http_accepted: timeseries(Sum),
}

pub trait MononokeStream: AsyncRead + AsyncWrite + Unpin + Send + Sync + 'static {}

impl<T> MononokeStream for T where T: AsyncRead + AsyncWrite + Unpin + Send + Sync + 'static {}

const KEEP_ALIVE_INTERVAL: Duration = Duration::from_millis(5000);
const CHUNK_SIZE: usize = 10000;
lazy_static! {
    static ref OPEN_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);
}

pub async fn wait_for_connections_closed(logger: &Logger) {
    loop {
        let conns = OPEN_CONNECTIONS.load(Ordering::Relaxed);
        if conns == 0 {
            break;
        }

        slog::info!(logger, "Waiting for {} connections to close", conns);
        tokio::time::sleep(Duration::new(1, 0)).await;
    }
}

pub async fn connection_acceptor(
    fb: FacebookInit,
    common_config: CommonConfig,
    sockname: String,
    service: ReadyFlagService,
    root_log: Logger,
    repo_handlers: HashMap<String, RepoHandler>,
    tls_acceptor: SslAcceptor,
    terminate_process: oneshot::Receiver<()>,
    rate_limiter: Option<RateLimitEnvironment>,
    scribe: Scribe,
    edenapi: EdenApi,
    will_exit: Arc<AtomicBool>,
    config_store: &ConfigStore,
    cslb_config: Option<String>,
    wireproto_scuba: MononokeScubaSampleBuilder,
    bound_addr_path: Option<PathBuf>,
    acl_provider: &dyn AclProvider,
) -> Result<()> {
    let enable_http_control_api = common_config.enable_http_control_api;

    let security_checker = ConnectionSecurityChecker::new(acl_provider, &common_config).await?;
    let addr: SocketAddr = sockname
        .parse()
        .with_context(|| format!("could not parse '{}'", sockname))?;
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("could not bind mononoke on '{}'", sockname))?;

    let mut terminate_process = terminate_process.fuse();

    let qps = match cslb_config {
        Some(config) => Some(Arc::new(
            Qps::new(fb, config, config_store).with_context(|| "Failed to initialize QPS")?,
        )),
        None => None,
    };

    // Now that we are listening and ready to accept connections, report that we are alive.
    service.set_ready();

    let bound_addr = listener.local_addr()?.to_string();
    debug!(root_log, "server is listening on {}", bound_addr);

    // Write out the bound address if requested, this is helpful in tests when using automatic binding with :0
    if let Some(bound_addr_path) = bound_addr_path {
        let mut writer = File::create(bound_addr_path)?;
        writer.write_all(bound_addr.as_bytes())?;
        writer.write_all(b"\n")?;
    }

    let acceptor = Arc::new(Acceptor {
        fb,
        tls_acceptor,
        repo_handlers,
        security_checker,
        rate_limiter,
        scribe,
        logger: root_log.clone(),
        edenapi,
        enable_http_control_api,
        server_hostname: get_hostname().unwrap_or_else(|_| "unknown_hostname".to_string()),
        will_exit,
        config_store: config_store.clone(),
        qps,
        wireproto_scuba,
        common_config,
    });

    loop {
        select_biased! {
            _ = terminate_process => {
                debug!(root_log, "Received shutdown handler, stop accepting connections...");
                return Ok(());
            },
            sock_tuple = listener.accept().fuse() => match sock_tuple {
                Ok((stream, addr)) => {
                    let conn = PendingConnection { acceptor: acceptor.clone(), addr };
                    let task = handle_connection(conn.clone(), stream);
                    conn.spawn_task(task, "Failed to handle_connection");
                }
                Err(err) => {
                    error!(root_log, "{}", err.to_string(); SlogKVError(Error::from(err)));
                }
            },
        };
    }
}

/// Our environment for accepting connections.
pub struct Acceptor {
    pub fb: FacebookInit,
    pub tls_acceptor: SslAcceptor,
    pub repo_handlers: HashMap<String, RepoHandler>,
    pub security_checker: ConnectionSecurityChecker,
    pub rate_limiter: Option<RateLimitEnvironment>,
    pub scribe: Scribe,
    pub logger: Logger,
    pub edenapi: EdenApi,
    pub enable_http_control_api: bool,
    pub server_hostname: String,
    pub will_exit: Arc<AtomicBool>,
    pub config_store: ConfigStore,
    pub qps: Option<Arc<Qps>>,
    pub wireproto_scuba: MononokeScubaSampleBuilder,
    pub common_config: CommonConfig,
}

/// Details for a socket we've just opened.
#[derive(Clone)]
pub struct PendingConnection {
    pub acceptor: Arc<Acceptor>,
    pub addr: SocketAddr,
}

/// A connection where we completed the initial TLS handshake.
#[derive(Clone)]
pub struct AcceptedConnection {
    pub pending: PendingConnection,
    pub is_trusted: bool,
    pub identities: Arc<MononokeIdentitySet>,
}

impl PendingConnection {
    /// Spawn a task that is dedicated to this connection. This will block server shutdown, and
    /// also log on error or cancellation.
    pub fn spawn_task(
        &self,
        task: impl Future<Output = Result<()>> + Send + 'static,
        label: &'static str,
    ) {
        let this = self.clone();

        OPEN_CONNECTIONS.fetch_add(1, Ordering::Relaxed);

        tokio::task::spawn(async move {
            let logger = &this.acceptor.logger;
            let res = task
                .on_cancel(|| warn!(logger, "connection to {} was cancelled", this.addr))
                .await
                .context(label)
                .with_context(|| format!("Failed to handle connection to {}", this.addr));

            if let Err(e) = res {
                error!(logger, "connection_acceptor error: {:#}", e);
            }

            OPEN_CONNECTIONS.fetch_sub(1, Ordering::Relaxed);
        });
    }
}

async fn handle_connection(conn: PendingConnection, sock: TcpStream) -> Result<()> {
    let ssl = Ssl::new(conn.acceptor.tls_acceptor.context()).context("Error creating Ssl")?;
    let ssl_socket = SslStream::new(ssl, sock).context("Error creating SslStream")?;
    let mut ssl_socket = Box::pin(ssl_socket);

    ssl_socket
        .as_mut()
        .accept()
        .await
        .context("Failed to perform tls handshake")?;

    let identities = match ssl_socket.ssl().peer_certificate() {
        Some(cert) => MononokeIdentity::try_from_x509(&cert),
        None => Err(ErrorKind::ConnectionNoClientCertificate.into()),
    }?;

    let is_trusted = conn
        .acceptor
        .security_checker
        .check_if_trusted(&identities)
        .await?;

    let conn = AcceptedConnection {
        pending: conn,
        is_trusted,
        identities: Arc::new(identities),
    };

    let ssl_socket = QuietShutdownStream::new(ssl_socket);

    handle_http(conn, ssl_socket)
        .await
        .context("Failed to handle_http")?;

    Ok(())
}

async fn handle_http<S: MononokeStream>(conn: AcceptedConnection, stream: S) -> Result<()> {
    STATS::http_accepted.add_value(1);

    let svc = MononokeHttpService::<S>::new(conn);

    // NOTE: We don't select h2 in alpn, so we only expect HTTP/1.1 here.
    Http::new()
        .http1_only(true)
        .serve_connection(stream, svc)
        .with_upgrades()
        .await
        .context("Failed to serve_connection")?;

    Ok(())
}

pub async fn handle_wireproto<R, W>(
    conn: AcceptedConnection,
    framed: FramedConn<R, W>,
    reponame: String,
    metadata: Metadata,
) -> Result<()>
where
    R: AsyncRead + Send + std::marker::Unpin + 'static,
    W: AsyncWrite + Send + std::marker::Unpin + 'static,
{
    let metadata = Arc::new(metadata);

    let ChannelConn {
        stdin,
        stdout,
        stderr,
        logger,
        keep_alive,
        join_handle,
    } = ChannelConn::setup(framed, conn.clone(), metadata.clone());

    if metadata.client_debug() {
        info!(&logger, "{:#?}", metadata; "remote" => "true");
    }

    // Don't let the logger hold onto the channel. This is a bit fragile (but at least it breaks
    // tests deterministically).
    drop(logger);

    let stdio = Stdio {
        metadata,
        stdin,
        stdout,
        stderr,
    };

    // Don't immediately return error here, we need to cleanup our
    // handlers like keep alive, otherwise they will run forever.
    let result = request_handler(
        conn.pending.acceptor.fb,
        reponame,
        &conn.pending.acceptor.repo_handlers,
        &conn.pending.acceptor.security_checker,
        stdio,
        conn.pending.acceptor.rate_limiter.clone(),
        conn.pending.addr.ip(),
        conn.pending.acceptor.scribe.clone(),
        conn.pending.acceptor.qps.clone(),
    )
    .await
    .context("Failed to execute request_handler");

    // Shutdown our keepalive handler
    keep_alive.abort();

    join_handle
        .await
        .context("Failed to join ChannelConn")?
        .context("Failed to close ChannelConn")?;

    result
}

pub struct FramedConn<R, W> {
    rd: FramedRead<R, SshDecoder>,
    wr: FramedWrite<W, SshEncoder>,
}

impl<R, W> FramedConn<R, W>
where
    R: AsyncRead + Send + std::marker::Unpin + 'static,
    W: AsyncWrite + Send + std::marker::Unpin + 'static,
{
    pub fn setup(rd: R, wr: W, compression_writes: Option<i32>) -> Result<Self> {
        // NOTE: FramedRead does buffering, so no need to wrap with a BufReader here.
        let rd = FramedRead::new(rd, SshDecoder::new());
        let wr = FramedWrite::new(wr, SshEncoder::new(compression_writes)?);
        Ok(Self { rd, wr })
    }
}

pub struct ChannelConn {
    stdin: BoxStream<Bytes, io::Error>,
    stdout: mpsc::Sender<Bytes>,
    stderr: mpsc::UnboundedSender<Bytes>,
    logger: Logger,
    keep_alive: AbortHandle,
    join_handle: JoinHandle<Result<(), io::Error>>,
}

impl ChannelConn {
    pub fn setup<R, W>(
        framed: FramedConn<R, W>,
        conn: AcceptedConnection,
        metadata: Arc<Metadata>,
    ) -> Self
    where
        R: AsyncRead + Send + std::marker::Unpin + 'static,
        W: AsyncWrite + Send + std::marker::Unpin + 'static,
    {
        let FramedConn { rd, wr } = framed;

        let stdin = Box::new(rd.compat().filter_map(|s| {
            if s.stream() == IoStream::Stdin {
                Some(s.data())
            } else {
                None
            }
        }));

        let (stdout, stderr, keep_alive, join_handle) = {
            let (otx, orx) = mpsc::channel(1);
            let (etx, erx) = mpsc::unbounded();
            let (ktx, krx) = mpsc::unbounded();

            let orx = orx
                .map(|blob| split_bytes_in_chunk(blob, CHUNK_SIZE))
                .flatten()
                .map(|v| SshMsg::new(IoStream::Stdout, v));
            let erx = erx
                .map(|blob| split_bytes_in_chunk(blob, CHUNK_SIZE))
                .flatten()
                .map(|v| SshMsg::new(IoStream::Stderr, v));
            let krx = krx.map(|v| SshMsg::new(IoStream::Stderr, v));

            // Glue them together
            let fwd = async move {
                let wr = WireprotoSink::new(wr);

                futures::pin_mut!(wr);

                let res = orx
                    .select(erx)
                    .select(krx)
                    .compat()
                    .map_err(|()| io::Error::new(io::ErrorKind::Other, "huh?"))
                    .forward(wr.as_mut())
                    .await;

                if let Err(e) = res.as_ref() {
                    let projected_wr = wr.as_mut().project();
                    let data = projected_wr.data;

                    let mut scuba = conn.pending.acceptor.wireproto_scuba.clone();
                    scuba.add_metadata(&metadata);
                    scuba.add_opt(
                        "last_successful_flush",
                        data.last_successful_flush.map(|dt| dt.timestamp()),
                    );
                    scuba.add_opt(
                        "last_successful_io",
                        data.last_successful_io.map(|dt| dt.timestamp()),
                    );
                    scuba.add_opt(
                        "last_failed_io",
                        data.last_failed_io.map(|dt| dt.timestamp()),
                    );
                    scuba.add("stdout_bytes", data.stdout.bytes);
                    scuba.add("stdout_messages", data.stdout.messages);
                    scuba.add("stderr_bytes", data.stderr.bytes);
                    scuba.add("stderr_messages", data.stderr.messages);
                    scuba.log_with_msg("Forwarding failed", format!("{:#}", e));
                }

                Ok(())
            };

            let keep_alive_sender = async move {
                loop {
                    tokio::time::sleep(KEEP_ALIVE_INTERVAL).await;
                    if ktx.unbounded_send(Bytes::new()).is_err() {
                        break;
                    }
                }
            };
            let (keep_alive_sender, keep_alive_abort) =
                futures::future::abortable(keep_alive_sender);

            // spawn a task for sending keepalive messages
            tokio::spawn(keep_alive_sender);

            // spawn a task for forwarding stdout/err into stream
            let join_handle = tokio::spawn(fwd);

            // NOTE: This might seem useless, but it's not. When you spawn a task, Tokio puts it on
            // a "LIFO slot" associated with the current thread. While the task is in the LIFO
            // slot, it is not eligible to be run by other threads. If the thread that just spawned
            // `fwd` above goes and does some expensive synchronous work in `request_handler` (we'd
            // like to avoid that but sometimes that happens), then that will delay `fwd`. This
            // means that notably keepalives will not be sent (you can repro by putting a
            // `std::thread::sleep` right after we spawn `fwd`). To mitigate this, we spawn another
            // dummy taks here. This task will take `fwd`'s place in the LIFO slot, thus pushing
            // `fwd` onto a task queue where other runtime threads can claim it. This way, even if
            // this thread goes do some expensive CPU-bound work, we won't delay keepalives.
            tokio::spawn(async {});

            (otx, etx, keep_alive_abort, join_handle)
        };

        let logger = create_conn_logger(stderr.clone(), None, None);

        ChannelConn {
            stdin,
            stdout,
            stderr,
            logger,
            keep_alive,
            join_handle,
        }
    }
}

// TODO(stash): T33775046 we had to chunk responses because hgcli
// can't cope with big chunks
fn split_bytes_in_chunk<E>(blob: Bytes, chunksize: usize) -> impl Stream<Item = Bytes, Error = E> {
    stream::unfold(blob, move |mut remain| {
        let len = remain.len();
        if len > 0 {
            let ret = remain.split_to(::std::cmp::min(chunksize, len));
            Some(Ok((ret, remain)))
        } else {
            None
        }
    })
}
