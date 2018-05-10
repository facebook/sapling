// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::net::SocketAddr;

use bytes::Bytes;
use futures::{future, stream, Future, Sink, Stream};

use tokio_core::reactor::Core;
use tokio_io::AsyncRead;
use tokio_io::codec::{FramedRead, FramedWrite};

use tokio::net::TcpStream;

use clap::ArgMatches;

use errors::*;

use futures_ext::StreamExt;
use sshrelay::{Preamble, SshDecoder, SshEncoder, SshMsg, SshStream};

mod fdio;

pub fn cmd(main: &ArgMatches, sub: &ArgMatches) -> Result<()> {
    if sub.is_present("stdio") {
        if let Some(repo) = main.value_of("repository") {
            let mononoke_path = sub.value_of("mononoke-path").unwrap();
            return ssh_relay(mononoke_path, repo);
        }
        bail_msg!("Missing repository");
    }
    bail_msg!("Only stdio server is supported");
}

fn ssh_relay<P: AsRef<str>>(path: P, repo: &str) -> Result<()> {
    let path = path.as_ref();

    let mut reactor = Core::new()?;

    // Get Streams for stdin/out/err
    let stdin = fdio::stdin();
    let stdout = fdio::stdout();
    let stderr = fdio::stderr();

    let addr: SocketAddr = path.parse()?;
    // Open socket
    let socket = TcpStream::connect(&addr)
        .map_err(|err| format_err!("connecting to Mononoke {} socket '{}' failed", path, err));

    let socket = reactor.run(socket)?;

    // Wrap the socket with the ssh codec
    let (socket_read, socket_write) = socket.split();
    let rx = FramedRead::new(socket_read, SshDecoder::new());
    let tx = FramedWrite::new(socket_write, SshEncoder::new());

    let preamble = Preamble::new(String::from(repo));
    let preamble = stream::once(Ok(SshMsg::new(SshStream::Preamble(preamble), Bytes::new())));

    // Start a task to copy from stdin to the socket
    let stdin_future = preamble
        .chain(stdin.map(|buf| SshMsg::new(SshStream::Stdin, buf)))
        .forward(tx)
        .map_err(Error::from)
        .map(|_| ());

    // A task to copy from the socket, then use streamfork() to split the
    // input between stdout and stderr.
    let stdout_future = rx.streamfork(
        // a sink each for stdout and stderr, prefixed with With to remove the
        // SshMsg framing and expose the raw data
        stdout.with(|m| future::ok::<_, Error>(SshMsg::data(m))),
        stderr.with(|m| future::ok::<_, Error>(SshMsg::data(m))),
        |msg| -> Result<bool> {
            // Select a sink based on the stream
            match msg.stream() {
                SshStream::Stdout => Ok(false),
                SshStream::Stderr => Ok(true),
                bad => bail_msg!("Bad stream: {:?}", bad),
            }
        },
    ).map(|_| ());

    // Run the reactor to completion and collect the results from the tasks
    match reactor.run(stdout_future.select(stdin_future)) {
        Ok(_) => Ok(()),
        Err((e, _)) => Err(e),
    }
}
