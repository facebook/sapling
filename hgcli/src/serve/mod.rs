// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::net::SocketAddr;

use bytes::Bytes;
use futures::{future, stream, Future, Sink, Stream};

use native_tls::TlsConnector;
use native_tls::backend::openssl::TlsConnectorBuilderExt;
use tokio_core::reactor::Core;
use tokio_io::AsyncRead;
use tokio_io::codec::{FramedRead, FramedWrite};
use tokio_tls::TlsConnectorExt;

use tokio::net::TcpStream;

use clap::ArgMatches;

use errors::*;

use futures_ext::StreamExt;
use secure_utils::build_pkcs12;
use sshrelay::{Preamble, SshDecoder, SshEncoder, SshMsg, SshStream};

mod fdio;

pub fn cmd(main: &ArgMatches, sub: &ArgMatches) -> Result<()> {
    if sub.is_present("stdio") {
        if let Some(repo) = main.value_of("repository") {
            let mononoke_path = sub.value_of("mononoke-path").unwrap();

            let cert = sub.value_of("cert")
                .expect("certificate file is not specified")
                .to_string();
            let private_key = sub.value_of("private-key")
                .expect("private key file is not specified")
                .to_string();
            let ca_pem = sub.value_of("ca-pem")
                .expect("Cental authority pem file is not specified")
                .to_string();
            let common_name = sub.value_of("common-name")
                .expect("expected SSL common name of the Mononoke server")
                .to_string();

            return stdio_relay(mononoke_path, repo, cert, private_key, ca_pem, common_name);
        }
        bail_msg!("Missing repository");
    }
    bail_msg!("Only stdio server is supported");
}

fn stdio_relay<P: AsRef<str>>(
    path: P,
    repo: &str,
    cert: String,
    private_key: String,
    ca_pem: String,
    ssl_common_name: String,
) -> Result<()> {
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

    let pkcs12 = build_pkcs12(cert, private_key)?;
    let mut connector_builder = TlsConnector::builder()?;
    connector_builder.identity(pkcs12)?;
    {
        let sslcontextbuilder = connector_builder.builder_mut();

        sslcontextbuilder.set_ca_file(ca_pem)?;
    }
    let connector = connector_builder.build()?;

    let socket = reactor.run(socket.and_then(move |socket| {
        let async_connector = connector.connect_async(&ssl_common_name, socket);
        async_connector.map_err(|err| format_err!("async connect error {}", err))
    }))?;

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
