/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use anyhow::{bail, format_err, Error, Result};
use bytes::Bytes;
use cached_config::{ConfigHandle, ConfigStore};
use cloned::cloned;
use failure_ext::SlogKVError;
use fbinit::FacebookInit;
use futures::{FutureExt as NewFutureExt, TryFutureExt};
use futures_ext::{try_boxfuture, BoxFuture, BoxStream, FutureExt, StreamExt};
use futures_old::sync::mpsc;
use futures_old::{future, stream, Async, Future, IntoFuture, Poll, Sink, Stream};
use itertools::join;
use lazy_static::lazy_static;
use metaconfig_types::{AllowlistEntry, CommonConfig};
use openssl::ssl::SslAcceptor;
use permission_checker::{
    BoxMembershipChecker, BoxPermissionChecker, MembershipCheckerBuilder, MononokeIdentity,
    MononokeIdentitySet, PermissionCheckerBuilder,
};
use repo_client::CONFIGERATOR_PUSHREDIRECT_ENABLE;
use scuba_ext::ScubaSampleBuilderExt;
use slog::{crit, error, o, Drain, Level, Logger};
use slog_kvfilter::KVFilter;
use tokio_codec::{FramedRead, FramedWrite};
use tokio_io::{AsyncRead, AsyncWrite, IoStream};
use tokio_old::net::{TcpListener, TcpStream};
use tokio_openssl::SslAcceptorExt;

use cmdlib::monitoring::ReadyFlagService;
use limits::types::MononokeThrottleLimits;
use pushredirect_enable::types::MononokePushRedirectEnable;
use sshrelay::{SenderBytesWrite, SshDecoder, SshEncoder, SshMsg, SshStream, Stdio};

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
pub fn connection_acceptor(
    fb: FacebookInit,
    common_config: CommonConfig,
    sockname: String,
    service: ReadyFlagService,
    root_log: Logger,
    repo_handlers: HashMap<String, RepoHandler>,
    tls_acceptor: SslAcceptor,
    terminate_process: Arc<AtomicBool>,
    config_store: Option<ConfigStore>,
) -> BoxFuture<(), Error> {
    let repo_handlers = Arc::new(repo_handlers);
    let tls_acceptor = Arc::new(tls_acceptor);
    let listener = listener(sockname)
        .expect("failed to create listener")
        .map_err(Error::from);

    let (load_limiting_config, pushredirect_config) = match config_store {
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

            let pushredirect_config = Some(try_boxfuture!(
                config_store.get_config_handle(CONFIGERATOR_PUSHREDIRECT_ENABLE.to_string(),)
            ));
            (load_limiting_config, pushredirect_config)
        }
        None => (None, None),
    };

    ConnectionsSecurityChecker::new(fb, common_config)
        .boxed()
        .compat()
        .map(Arc::new)
        .map_err(|err| format_err!("error while creating security checker: {}", err))
        .and_then(move |security_checker| {
            // Now that we are listening and ready to accept connections, report that we are alive.
            service.set_ready();

            TakeUntilNotSet::new(listener.boxify(), terminate_process).for_each(move |sock| {
                // Accept the request without blocking the listener
                cloned!(
                    load_limiting_config,
                    pushredirect_config,
                    root_log,
                    repo_handlers,
                    tls_acceptor,
                    security_checker
                );
                OPEN_CONNECTIONS.fetch_add(1, Ordering::Relaxed);
                tokio_old::spawn(future::lazy(move || {
                    accept(
                        fb,
                        sock,
                        root_log,
                        repo_handlers,
                        tls_acceptor,
                        security_checker.clone(),
                        load_limiting_config.clone(),
                        pushredirect_config.clone(),
                    )
                    .then(|res| {
                        OPEN_CONNECTIONS.fetch_sub(1, Ordering::Relaxed);
                        res
                    })
                }));
                Ok(())
            })
        })
        .boxify()
}

fn accept(
    fb: FacebookInit,
    sock: TcpStream,
    root_log: Logger,
    repo_handlers: Arc<HashMap<String, RepoHandler>>,
    tls_acceptor: Arc<SslAcceptor>,
    security_checker: Arc<ConnectionsSecurityChecker>,
    load_limiting_config: Option<(ConfigHandle<MononokeThrottleLimits>, String)>,
    pushredirect_config: Option<ConfigHandle<MononokePushRedirectEnable>>,
) -> impl Future<Item = (), Error = ()> {
    let addr = sock.peer_addr();

    tls_acceptor
        .accept_async(sock)
        .map_err({
            cloned!(root_log);
            move |err| {
                error!(
                    root_log,
                    "Error while establishing tls connection";
                    SlogKVError(Error::from(err)),
                )
            }
        })
        .and_then({
            cloned!(root_log);
            move |sock| {
                let identities = match sock.get_ref().ssl().peer_certificate() {
                    Some(cert) => MononokeIdentity::try_from_x509(&cert),
                    None => Err(ErrorKind::ConnectionNoClientCertificate.into()),
                };

                let identities = identities.map_err({
                    cloned!(root_log);
                    move |err| {
                        error!(
                            root_log,
                            "failed to get identities from certificate"; SlogKVError(err),
                        )
                    }
                });

                ssh_server_mux(sock)
                    .map_err({
                        cloned!(root_log);
                        move |err| {
                            error!(
                                root_log,
                                "Error while reading preamble";
                                SlogKVError(err),
                            )
                        }
                    })
                    .join(identities)
            }
        })
        .join(addr.into_future().map_err({
            cloned!(root_log);
            move |err| {
                crit!(
                    root_log,
                    "Failed to get peer addr"; SlogKVError(Error::from(err)),
                )
            }
        }))
        .and_then(move |((stdio, identities), addr)| {
            repo_handlers
                .get(&stdio.preamble.reponame)
                .cloned()
                .ok_or_else(|| {
                    error!(root_log, "Unknown repo: {}", stdio.preamble.reponame);
                    let tmp_conn_logger = create_conn_logger(&stdio);
                    error!(
                        tmp_conn_logger,
                        "Requested repo \"{}\" does not exist or disabled", stdio.preamble.reponame
                    )
                })
                .into_future()
                .and_then(move |mut handler| {
                    handler
                        .scuba
                        .add_preamble(&stdio.preamble)
                        .add("client_ip", addr.to_string())
                        .add("client_identities", join(identities.iter(), ","));

                    (async move {
                        security_checker
                            .check_if_connections_allowed(&identities)
                            .await
                    })
                    .boxed()
                    .compat()
                    .map_err({
                        cloned!(root_log);
                        move |err| {
                            error!(
                                root_log,
                                "failed to check if connection is allowed"; SlogKVError(err),
                            )
                        }
                    })
                    .and_then(move |is_allowed| {
                        if is_allowed {
                            request_handler(
                                fb,
                                handler,
                                stdio,
                                load_limiting_config,
                                pushredirect_config,
                                addr.ip(),
                            )
                            .map(Ok)
                            .boxed()
                            .compat()
                            .left_future()
                        } else {
                            let err: Error = ErrorKind::AuthorizationFailed.into();
                            let tmp_conn_log = create_conn_logger(&stdio);
                            // Log to scuba
                            handler
                                .scuba
                                .log_with_msg("Authorization failed", format!("{}", err));
                            // This log goes to the user
                            error!(tmp_conn_log, "Authorization failed: {}", err);
                            // This log goes to the server stdout/stderr
                            error!(root_log, "Authorization failed"; SlogKVError(err));
                            future::err(()).right_future()
                        }
                    })
                })
        })
        .boxify()
}

fn create_conn_logger(stdio: &Stdio) -> Logger {
    let stderr_write = SenderBytesWrite {
        chan: stdio.stderr.clone().wait(),
    };
    let drain = slog_term::PlainSyncDecorator::new(stderr_write);
    let drain = slog_term::FullFormat::new(drain).build();
    let drain = KVFilter::new(drain, Level::Critical);
    Logger::root(drain.ignore_res(), o!())
}

struct ConnectionsSecurityChecker {
    tier_permchecker: BoxPermissionChecker,
    allowlisted_checker: BoxMembershipChecker,
}

impl ConnectionsSecurityChecker {
    async fn new(fb: FacebookInit, common_config: CommonConfig) -> Result<Self> {
        let mut allowlisted_identities = MononokeIdentitySet::new();
        let mut tier_permchecker = None;

        for allowlist_entry in common_config.security_config {
            match allowlist_entry {
                AllowlistEntry::HardcodedIdentity { ty, data } => {
                    allowlisted_identities.insert(MononokeIdentity::new(&ty, &data)?);
                }
                AllowlistEntry::Tier(tier) => {
                    if tier_permchecker.is_some() {
                        bail!("invalid config: only one PermissionChecker for tier is allowed");
                    }
                    tier_permchecker =
                        Some(PermissionCheckerBuilder::acl_for_tier(fb, &tier).await?);
                }
            }
        }

        Ok(Self {
            tier_permchecker: tier_permchecker
                .unwrap_or_else(|| PermissionCheckerBuilder::always_reject()),
            allowlisted_checker: MembershipCheckerBuilder::allowlist_checker(
                allowlisted_identities,
            ),
        })
    }

    async fn check_if_connections_allowed(&self, identities: &MononokeIdentitySet) -> Result<bool> {
        let action = "tupperware";
        Ok(self.allowlisted_checker.is_member(&identities).await?
            || self
                .tier_permchecker
                .check_set(&identities, &[action])
                .await?)
    }
}

fn listener<P>(sockname: P) -> io::Result<IoStream<TcpStream>>
where
    P: AsRef<str>,
{
    let sockname = sockname.as_ref();
    let listener;
    let addr: SocketAddr = sockname.parse().unwrap();

    // First bind the socket. If the socket already exists then try connecting to it;
    // if there's no connection then replace it with a new one. (This assumes that simply
    // connecting is a no-op).
    loop {
        match TcpListener::bind(&addr) {
            Ok(l) => {
                listener = l;
                break;
            }
            Err(err) => {
                return Err(err);
            }
        }
    }

    Ok(listener.incoming().boxify())
}

// As a server, given a stream to a client, return an Io pair with stdin/stdout, and an
// auxillary sink for stderr.
fn ssh_server_mux<S>(s: S) -> BoxFuture<Stdio, Error>
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (rx, tx) = s.split();
    let wr = FramedWrite::new(tx, SshEncoder::new());
    let rd = FramedRead::new(rx, SshDecoder::new());

    rd.into_future()
        .map_err(|_err| ErrorKind::ConnectionError.into())
        .and_then(move |(maybe_preamble, rd)| {
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

            let stdin = rd
                .filter_map(|s| {
                    if s.stream() == SshStream::Stdin {
                        Some(s.data())
                    } else {
                        None
                    }
                })
                .boxify();

            let (stdout, stderr) = {
                let (otx, orx) = mpsc::channel(1);
                let (etx, erx) = mpsc::channel(1);

                let orx = orx
                    .map(|blob: Bytes| split_bytes_in_chunk(blob, CHUNK_SIZE))
                    .flatten()
                    .map(|v| SshMsg::new(SshStream::Stdout, v));
                let erx = erx
                    .map(|blob: Bytes| split_bytes_in_chunk(blob, CHUNK_SIZE))
                    .flatten()
                    .map(|v| SshMsg::new(SshStream::Stderr, v));

                // Glue them together
                let fwd = orx
                    .select(erx)
                    .map_err(|()| io::Error::new(io::ErrorKind::Other, "huh?"))
                    .forward(wr);

                // spawn a task for forwarding stdout/err into stream
                tokio_old::spawn(fwd.discard());

                (otx, etx)
            };

            Ok(Stdio {
                preamble,
                stdin,
                stdout,
                stderr,
            })
        })
        .boxify()
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

// Stream wrapper that stops when a flag is set
// It does it by periodically checking the flag's value
struct TakeUntilNotSet<T> {
    periodic_checker: BoxStream<(), Error>,
    input: BoxStream<T, Error>,
    flag: Arc<AtomicBool>,
}

impl<T> TakeUntilNotSet<T> {
    fn new(input: BoxStream<T, Error>, flag: Arc<AtomicBool>) -> Self {
        Self {
            periodic_checker: tokio_timer::Interval::new_interval(Duration::new(1, 0))
                .map(|_| ())
                .map_err(Error::msg)
                .boxify(),
            input,
            flag,
        }
    }
}

impl<T> Stream for TakeUntilNotSet<T> {
    type Item = T;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.flag.load(Ordering::Relaxed) {
            return Ok(Async::Ready(None));
        }

        match self.periodic_checker.poll()? {
            Async::NotReady | Async::Ready(Some(())) => {}
            Async::Ready(None) => {
                unreachable!("infinite loop finished?");
            }
        };

        self.input.poll()
    }
}
