/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::security_checker::ConnectionsSecurityChecker;
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use anyhow::{Context, Error, Result};
use bytes::Bytes;
use cached_config::{ConfigHandle, ConfigStore};
use cloned::cloned;
use failure_ext::SlogKVError;
use fbinit::FacebookInit;
use futures::{channel::oneshot, select_biased};
use futures_old::{stream, sync::mpsc, Stream};
use futures_util::compat::Stream01CompatExt;
use futures_util::future::FutureExt;
use futures_util::stream::{StreamExt, TryStreamExt};
use lazy_static::lazy_static;
use live_commit_sync_config::CfgrLiveCommitSyncConfig;
use metaconfig_types::CommonConfig;
use openssl::ssl::SslAcceptor;
use permission_checker::MononokeIdentity;
use scribe_ext::Scribe;
use slog::{debug, error, Logger};
// use slog_kvfilter::KVFilter;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_util::codec::{FramedRead, FramedWrite};

use cmdlib::monitoring::ReadyFlagService;
use limits::types::MononokeThrottleLimits;
use sshrelay::{SshDecoder, SshEncoder, SshMsg, SshStream, Stdio};

use crate::errors::ErrorKind;
use crate::repo_handlers::RepoHandler;
use crate::request_handler::request_handler;

const CHUNK_SIZE: usize = 10000;
const CONFIGERATOR_LIMITS_CONFIG: &str = "scm/mononoke/loadshedding/limits";
lazy_static! {
    static ref OPEN_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);
}

pub async fn wait_for_connections_closed() {
    while OPEN_CONNECTIONS.load(Ordering::Relaxed) > 0 {
        tokio::time::delay_for(Duration::new(1, 0)).await;
    }
}

/// This function accepts connections, reads Preamble and routes request to a thread responsible for
/// a particular repo
pub async fn connection_acceptor(
    fb: FacebookInit,
    common_config: CommonConfig,
    sockname: String,
    service: ReadyFlagService,
    root_log: Logger,
    repo_handlers: HashMap<String, RepoHandler>,
    tls_acceptor: SslAcceptor,
    terminate_process: oneshot::Receiver<()>,
    config_store: Option<ConfigStore>,
    scribe: Scribe,
) -> Result<()> {
    let (load_limiting_config, maybe_live_commit_sync_config) = match config_store {
        Some(config_store) => {
            let load_limiting_config = {
                let config_loader = config_store
                    .get_config_handle(CONFIGERATOR_LIMITS_CONFIG.to_string())
                    .ok();
                config_loader.and_then(|config_loader| {
                    common_config
                        .loadlimiter_category
                        .clone()
                        .map(|category| (config_loader, category))
                })
            };

            let maybe_live_commit_sync_config =
                Some(CfgrLiveCommitSyncConfig::new(&root_log, &config_store)?);

            (load_limiting_config, maybe_live_commit_sync_config)
        }
        None => (None, None),
    };

    let security_checker = Arc::new(ConnectionsSecurityChecker::new(fb, common_config).await?);
    let repo_handlers = Arc::new(repo_handlers);
    let tls_acceptor = Arc::new(tls_acceptor);
    let addr: SocketAddr = sockname.parse()?;
    let mut listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("could not bind mononoke on '{}'", sockname))?;

    // Now that we are listening and ready to accept connections, report that we are alive.
    service.set_ready();

    let mut terminate_process = terminate_process.fuse();
    loop {
        select_biased! {
            _ = terminate_process => {
                debug!(root_log, "Received shutdown handler, stop accepting connections...");
                return Ok(());
            },
            sock_tuple = listener.accept().fuse() => match sock_tuple {
                Ok((stream, _)) => {
                    let logger = root_log.clone();
                    cloned!(
                        load_limiting_config,
                        maybe_live_commit_sync_config,
                        repo_handlers,
                        tls_acceptor,
                        security_checker,
                        scribe,
                    );

                    OPEN_CONNECTIONS.fetch_add(1, Ordering::Relaxed);
                    tokio::spawn(async move {
                        match accept(
                            fb,
                            stream,
                            repo_handlers,
                            tls_acceptor,
                            security_checker,
                            load_limiting_config,
                            maybe_live_commit_sync_config,
                            scribe,
                        )
                        .await {
                            Err(err) => error!(logger, "{}", err.to_string(); SlogKVError(Error::from(err))),
                            _ => {},
                        };
                        OPEN_CONNECTIONS.fetch_sub(1, Ordering::Relaxed);
                    });
                }
                Err(err) => {
                    error!(root_log, "{}", err.to_string(); SlogKVError(Error::from(err)));
                }
            },
        };
    }
}

async fn accept(
    fb: FacebookInit,
    sock: TcpStream,
    repo_handlers: Arc<HashMap<String, RepoHandler>>,
    tls_acceptor: Arc<SslAcceptor>,
    security_checker: Arc<ConnectionsSecurityChecker>,
    load_limiting_config: Option<(ConfigHandle<MononokeThrottleLimits>, String)>,
    maybe_live_commit_sync_config: Option<CfgrLiveCommitSyncConfig>,
    scribe: Scribe,
) -> Result<()> {
    let addr = sock.peer_addr()?;

    let ssl_socket = tokio_openssl::accept(&tls_acceptor, sock)
        .await
        .with_context(|| format!("tls accept failed: talking to '{}'", addr))?;

    let identities = match ssl_socket.ssl().peer_certificate() {
        Some(cert) => MononokeIdentity::try_from_x509(&cert),
        None => Err(ErrorKind::ConnectionNoClientCertificate.into()),
    }?;

    let (stdio, forwarding_join_handle) = ssh_server_mux(ssl_socket)
        .await
        .with_context(|| format!("reading preamble failed: talking to '{}'", addr))?;

    request_handler(
        fb,
        repo_handlers,
        security_checker,
        identities,
        stdio,
        load_limiting_config,
        addr.ip(),
        maybe_live_commit_sync_config,
        scribe,
    )
    .await?;

    let _ = forwarding_join_handle.await?;
    Ok(())
}

// As a server, given a stream to a client, return an Io pair with stdin/stdout, and an
// auxillary sink for stderr.
async fn ssh_server_mux<S>(
    s: S,
) -> Result<(Stdio, JoinHandle<std::result::Result<(), std::io::Error>>)>
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (rx, tx) = tokio::io::split(s);
    let wr = FramedWrite::new(tx, SshEncoder::new());
    let mut rd = FramedRead::new(rx, SshDecoder::new());

    let maybe_preamble = rd.next().await.transpose()?;
    let preamble = match maybe_preamble {
        Some(maybe_preamble) => {
            if let SshStream::Preamble(preamble) = maybe_preamble.stream() {
                preamble
            } else {
                return Err(ErrorKind::NoConnectionPreamble.into());
            }
        }
        None => {
            return Err(ErrorKind::NoConnectionPreamble.into());
        }
    };

    let stdin = Box::new(rd.compat().filter_map(|s| {
        if s.stream() == SshStream::Stdin {
            Some(s.data())
        } else {
            None
        }
    }));

    let (stdout, stderr, join_handle) = {
        let (otx, orx) = mpsc::channel(1);
        let (etx, erx) = mpsc::channel(1);

        let orx = orx
            .map(|blob| split_bytes_in_chunk(blob, CHUNK_SIZE))
            .flatten()
            .map(|v| SshMsg::new(SshStream::Stdout, v));
        let erx = erx
            .map(|blob| split_bytes_in_chunk(blob, CHUNK_SIZE))
            .flatten()
            .map(|v| SshMsg::new(SshStream::Stderr, v));

        // Glue them together
        let fwd = orx
            .select(erx)
            .compat()
            .map_err(|()| io::Error::new(io::ErrorKind::Other, "huh?"))
            .forward(wr);

        // spawn a task for forwarding stdout/err into stream
        let join_handle = tokio::spawn(fwd);

        (otx, etx, join_handle)
    };

    Ok((
        Stdio {
            preamble,
            stdin,
            stdout,
            stderr,
        },
        join_handle,
    ))
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
