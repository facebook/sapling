// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;

use failure::SlogKVError;
use futures::{Future, IntoFuture, Sink, Stream};
use futures::sync::mpsc;
use futures_ext::{BoxFuture, FutureExt, StreamExt};
use openssl::ssl::SslAcceptor;
use slog::Logger;
use tokio::net::{TcpListener, TcpStream};
use tokio_codec::{FramedRead, FramedWrite};
use tokio_core::reactor::{Core, Remote};
use tokio_io::{AsyncRead, AsyncWrite, IoStream};
use tokio_openssl::SslAcceptorExt;

use sshrelay::{SshDecoder, SshEncoder, SshMsg, SshStream, Stdio};

use errors::*;

/// This function accepts connections, reads Preamble and routes request to a thread responsible for
/// a particular repo
pub fn connection_acceptor(
    sockname: &str,
    root_log: Logger,
    repo_senders: HashMap<String, mpsc::Sender<(Stdio, SocketAddr)>>,
    tls_acceptor: SslAcceptor,
) -> ! {
    let mut core = Core::new().expect("failed to create tokio core");
    let remote = core.remote();
    let connection_acceptor = listener(sockname)
        .expect("failed to create listener")
        .map_err(Error::from)
        .and_then({
            let root_log = root_log.clone();
            move |sock| {
                let addr = match sock.peer_addr() {
                    Ok(addr) => addr,
                    Err(err) => {
                        crit!(root_log, "Failed to get peer addr"; SlogKVError(Error::from(err)));
                        return Ok(None).into_future().left_future();
                    }
                };
                tls_acceptor
                    .accept_async(sock)
                    .then({
                        let remote = remote.clone();
                        let root_log = root_log.clone();
                        move |sock| match sock {
                            Ok(sock) => ssh_server_mux(sock, remote.clone())
                                .map(move |stdio| Some((stdio, addr)))
                                .or_else({
                                    let root_log = root_log.clone();
                                    move |err| {
                                        error!(root_log, "Error while reading preamble: {}", err);
                                        Ok(None)
                                    }
                                })
                                .left_future(),
                            Err(err) => {
                                error!(root_log, "Error while reading preamble: {}", err);
                                Ok(None).into_future().right_future()
                            }
                        }
                    })
                    .right_future()
            }
        })
        .for_each(move |maybe_stdio| {
            if maybe_stdio.is_none() {
                return Ok(()).into_future().boxify();
            }
            let (stdio, addr) = maybe_stdio.unwrap();
            match repo_senders.get(&stdio.preamble.reponame) {
                Some(sender) => sender
                    .clone()
                    .send((stdio, addr))
                    .map(|_| ())
                    .or_else({
                        let root_log = root_log.clone();
                        move |err| {
                            error!(
                                root_log,
                                "Failed to send request to a repo processing thread: {}", err
                            );
                            Ok(())
                        }
                    })
                    .boxify(),
                None => {
                    error!(root_log, "Unknown repo: {}", stdio.preamble.reponame);
                    Ok(()).into_future().boxify()
                }
            }
        });

    core.run(connection_acceptor)
        .expect("failure while running listener on tokio core");

    // The server is an infinite stream of connections
    unreachable!();
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
fn ssh_server_mux<S>(s: S, remote: Remote) -> BoxFuture<Stdio, Error>
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
                remote.spawn(|_handle| fwd.discard());

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
