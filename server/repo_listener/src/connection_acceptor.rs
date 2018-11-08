// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, atomic::{AtomicBool, AtomicUsize, Ordering}};
use std::time::Duration;

use bytes::Bytes;
use failure::{err_msg, SlogKVError};
use futures::{future, stream, Async, Future, IntoFuture, Poll, Sink, Stream};
use futures::sync::mpsc;
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use openssl::ssl::SslAcceptor;
use slog::{Drain, Level, Logger};
use slog_kvfilter::KVFilter;
use tokio;
use tokio::net::{TcpListener, TcpStream};
use tokio_codec::{FramedRead, FramedWrite};
use tokio_io::{AsyncRead, AsyncWrite, IoStream};
use tokio_openssl::SslAcceptorExt;
use tokio_timer;

use sshrelay::{SenderBytesWrite, SshDecoder, SshEncoder, SshMsg, SshStream, Stdio};

use errors::*;
use repo_handlers::RepoHandler;
use request_handler::request_handler;

const CHUNK_SIZE: usize = 10000;

lazy_static! {
    static ref OPEN_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);
}

/// This function accepts connections, reads Preamble and routes request to a thread responsible for
/// a particular repo
pub fn connection_acceptor(
    sockname: String,
    root_log: Logger,
    repo_handlers: HashMap<String, RepoHandler>,
    tls_acceptor: SslAcceptor,
    terminate_process: &'static AtomicBool,
) -> BoxFuture<(), Error> {
    let repo_handlers = Arc::new(repo_handlers);
    let tls_acceptor = Arc::new(tls_acceptor);
    let listener = listener(sockname)
        .expect("failed to create listener")
        .map_err(Error::from);

    TakeUntilNotSet::new(listener.boxify(), terminate_process)
        .for_each(move |sock| {
            // Accept the request without blocking the listener
            cloned!(root_log, repo_handlers, tls_acceptor);
            OPEN_CONNECTIONS.fetch_add(1, Ordering::Relaxed);
            tokio::spawn(future::lazy(move || {
                accept(sock, root_log, repo_handlers, tls_acceptor).then(|res| {
                    OPEN_CONNECTIONS.fetch_sub(1, Ordering::Relaxed);
                    res
                })
            }));
            Ok(())
        })
        .and_then(|()| {
            // A termination signal was sent to the server, and we give open
            // connections time to finish. Note that some connections can be
            // very long, so the best scenario is to send SIGTERM first, then
            // wait for some time and send SIGKILL if server is still alive.
            stream::repeat(())
                .and_then(|()| tokio_timer::sleep(Duration::new(1, 0)).from_err())
                .take_while(|()| Ok(OPEN_CONNECTIONS.load(Ordering::Relaxed) != 0))
                .for_each(|()| Ok(()))
        })
        .boxify()
}

fn accept(
    sock: TcpStream,
    root_log: Logger,
    repo_handlers: Arc<HashMap<String, RepoHandler>>,
    tls_acceptor: Arc<SslAcceptor>,
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
                ssh_server_mux(sock).map_err(move |err| {
                    error!(
                        root_log,
                        "Error while reading preamble";
                        SlogKVError(Error::from(err)),
                    )
                })
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
        .and_then(move |(stdio, addr)| {
            repo_handlers
                .get(&stdio.preamble.reponame)
                .cloned()
                .ok_or_else(|| {
                    error!(root_log, "Unknown repo: {}", stdio.preamble.reponame);
                    let tmp_conn_logger = {
                        let stderr_write = SenderBytesWrite {
                            chan: stdio.stderr.clone().wait(),
                        };
                        let drain = slog_term::PlainSyncDecorator::new(stderr_write);
                        let drain = slog_term::FullFormat::new(drain).build();
                        let drain = KVFilter::new(drain, Level::Critical);
                        Logger::root(drain.ignore_res(), o!())
                    };
                    error!(
                        tmp_conn_logger,
                        "Requested repo \"{}\" does not exist or disabled", stdio.preamble.reponame
                    )
                })
                .into_future()
                .and_then(move |handler| {
                    request_handler(handler.clone(), stdio, addr, handler.repo.hook_manager())
                })
        })
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

            let stdin = rd.filter_map(|s| {
                if s.stream() == SshStream::Stdin {
                    Some(s.data())
                } else {
                    None
                }
            }).boxify();

            let (stdout, stderr) = {
                let (otx, orx) = mpsc::channel(1);
                let (etx, erx) = mpsc::channel(1);

                let orx = orx.map(|blob| split_bytes_in_chunk(blob, CHUNK_SIZE))
                    .flatten()
                    .map(|v| SshMsg::new(SshStream::Stdout, v));
                let erx = erx.map(|blob| split_bytes_in_chunk(blob, CHUNK_SIZE))
                    .flatten()
                    .map(|v| SshMsg::new(SshStream::Stderr, v));

                // Glue them together
                let fwd = orx.select(erx)
                    .map_err(|()| io::Error::new(io::ErrorKind::Other, "huh?"))
                    .forward(wr);

                // spawn a task for forwarding stdout/err into stream
                tokio::spawn(fwd.discard());

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
    flag: &'static AtomicBool,
}

impl<T> TakeUntilNotSet<T> {
    fn new(input: BoxStream<T, Error>, flag: &'static AtomicBool) -> Self {
        Self {
            periodic_checker: tokio_timer::Interval::new_interval(Duration::new(1, 0))
                .map(|_| ())
                .map_err(|e| err_msg(format!("{}", e)))
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
