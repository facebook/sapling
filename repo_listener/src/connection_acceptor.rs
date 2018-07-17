// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use failure::SlogKVError;
use futures::{future, Future, IntoFuture, Stream};
use futures::sync::mpsc;
use futures_ext::{BoxFuture, FutureExt, StreamExt};
use openssl::ssl::SslAcceptor;
use slog::Logger;
use tokio;
use tokio::net::{TcpListener, TcpStream};
use tokio_codec::{FramedRead, FramedWrite};
use tokio_io::{AsyncRead, AsyncWrite, IoStream};
use tokio_openssl::SslAcceptorExt;

use sshrelay::{SshDecoder, SshEncoder, SshMsg, SshStream, Stdio};

use errors::*;
use repo_handlers::RepoHandler;
use request_handler::request_handler;

/// This function accepts connections, reads Preamble and routes request to a thread responsible for
/// a particular repo
pub fn connection_acceptor(
    sockname: String,
    root_log: Logger,
    repo_handlers: HashMap<String, RepoHandler>,
    tls_acceptor: SslAcceptor,
) -> BoxFuture<(), Error> {
    let repo_handlers = Arc::new(repo_handlers);
    let tls_acceptor = Arc::new(tls_acceptor);

    listener(sockname)
        .expect("failed to create listener")
        .map_err(Error::from)
        .for_each(move |sock| {
            // Accept the request without blocking the listener
            cloned!(root_log, repo_handlers, tls_acceptor);
            tokio::spawn(future::lazy(move || {
                accept(sock, root_log, repo_handlers, tls_acceptor)
            }));
            Ok(())
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
                .ok_or_else(|| error!(root_log, "Unknown repo: {}", stdio.preamble.reponame))
                .into_future()
                .and_then(move |handler| request_handler(handler.clone(), stdio, addr))
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

                let orx = orx.map(|v| SshMsg::new(SshStream::Stdout, v));
                let erx = erx.map(|v| SshMsg::new(SshStream::Stderr, v));

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
