// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::env::var;
use std::net::{IpAddr, SocketAddr};

use bytes::Bytes;
use futures::{future, stream, Future, Sink, Stream};
use slog::{Drain, Logger};
use slog_term;

use dns_lookup::lookup_addr;
use native_tls::TlsConnector;
use native_tls::backend::openssl::TlsConnectorBuilderExt;
use tokio_io::AsyncRead;
use tokio_io::codec::{FramedRead, FramedWrite};
use tokio_tls::{TlsConnectorExt, TlsStream};
use uuid;

use tokio::net::TcpStream;

use clap::ArgMatches;

use errors::*;

use failure::SlogKVError;
use fbwhoami;
use futures_ext::{BoxFuture, FutureExt, StreamExt};
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use secure_utils::build_pkcs12;
use sshrelay::{Preamble, SenderBytesWrite, SshDecoder, SshEncoder, SshMsg, SshStream, Stdio};

mod fdio;

pub fn cmd(main: &ArgMatches, sub: &ArgMatches) -> BoxFuture<(), Error> {
    if sub.is_present("stdio") {
        if let Some(repo) = main.value_of("repository") {
            let mononoke_path = sub.value_of("mononoke-path").unwrap();

            let cert = sub.value_of("cert")
                .expect("certificate file is not specified");
            let private_key = sub.value_of("private-key")
                .expect("private key file is not specified");
            let ca_pem = sub.value_of("ca-pem")
                .expect("Cental authority pem file is not specified");
            let common_name = sub.value_of("common-name")
                .expect("expected SSL common name of the Mononoke server");
            let is_remote_proxy = main.is_present("remote-proxy");
            let scuba_table = main.value_of("scuba-table");

            return StdioRelay {
                path: mononoke_path,
                repo,
                cert,
                private_key,
                ca_pem,
                ssl_common_name: common_name,
                is_remote_proxy,
                scuba_table,
            }.run();
        }
        return future::err(format_err!("Missing repository")).boxify();
    }
    return future::err(format_err!("Only stdio server is supported")).boxify();
}

struct StdioRelay<'a> {
    path: &'a str,
    repo: &'a str,
    cert: &'a str,
    private_key: &'a str,
    ca_pem: &'a str,
    ssl_common_name: &'a str,
    is_remote_proxy: bool,
    scuba_table: Option<&'a str>,
}

impl<'a> StdioRelay<'a> {
    fn run(self) -> BoxFuture<(), Error> {
        let mut scuba_logger =
            ScubaSampleBuilder::with_opt_table(self.scuba_table.map(|v| v.to_owned()));

        let session_uuid = uuid::Uuid::new_v4();
        let unix_username = var("USER").ok();
        let source_hostname = if self.is_remote_proxy {
            // hgcli is run as remote proxy so grab from ssh the information about what host has
            // connected to this proxy and save it as source_hostname
            var("SSH_CONNECTION")
                .ok()
                .and_then(|line| {
                    line.split_whitespace()
                        .next()
                        .and_then(|ip| ip.parse::<IpAddr>().ok())
                })
                .and_then(|ip| lookup_addr(&ip).ok())
        } else {
            // hgcli is run locally, so the source_hostname is the host it is currently running on
            fbwhoami::FbWhoAmI::new()
                .ok()
                .and_then(|who| who.get_name())
                .map(|hostname| hostname.to_owned())
        };

        let preamble = Preamble::new(
            self.repo.to_owned(),
            session_uuid.clone(),
            unix_username,
            source_hostname,
        );

        scuba_logger.add_preamble(&preamble);

        let stdio = Stdio {
            preamble,
            stdin: fdio::stdin(),
            stdout: fdio::stdout(),
            stderr: fdio::stderr(),
        };

        let client_logger = {
            let stderr_write = SenderBytesWrite {
                chan: stdio.stderr.clone().wait(),
            };
            let drain = slog_term::PlainSyncDecorator::new(stderr_write);
            let drain = slog_term::FullFormat::new(drain).build();
            Logger::root(drain.fuse(), o!())
        };

        debug!(
            client_logger,
            "Session with Mononoke started with uuid: {}", session_uuid
        );
        scuba_logger.log_with_msg("Hgcli proxy - connected", None);

        self.internal_run(stdio)
            .map_err(move |err| {
                let err_msg = format!("{:#?}", err);
                scuba_logger.log_with_msg("Hgcli proxy - error", err_msg.clone());
                error!(client_logger, "Error in hgcli proxy"; SlogKVError(err));
                format_err!("{}", err_msg)
            })
            .boxify()
    }

    fn establish_connection(&self) -> impl Future<Item = TlsStream<TcpStream>, Error = Error> {
        let path = self.path.to_owned();
        let ssl_common_name = self.ssl_common_name.to_owned();

        let addr: SocketAddr = try_boxfuture!(path.parse());
        // Open socket
        let socket = TcpStream::connect(&addr).map_err(move |err| {
            format_err!("connecting to Mononoke {} socket '{}' failed", path, err)
        });

        let pkcs12 = try_boxfuture!(build_pkcs12(
            self.cert.to_owned(),
            self.private_key.to_owned(),
        ));
        let mut connector_builder = try_boxfuture!(TlsConnector::builder());
        try_boxfuture!(connector_builder.identity(pkcs12));
        {
            let sslcontextbuilder = connector_builder.builder_mut();

            try_boxfuture!(sslcontextbuilder.set_ca_file(self.ca_pem.to_owned()));
        }
        let connector = try_boxfuture!(connector_builder.build());

        socket
            .and_then(move |socket| {
                let async_connector = connector.connect_async(&ssl_common_name, socket);
                async_connector.map_err(|err| format_err!("async connect error {}", err))
            })
            .boxify()
    }

    fn internal_run(self, stdio: Stdio) -> impl Future<Item = (), Error = Error> {
        let Stdio {
            preamble,
            stdin,
            stdout,
            stderr,
        } = stdio;

        self.establish_connection().and_then(|socket| {
            // Wrap the socket with the ssh codec
            let (socket_read, socket_write) = socket.split();
            let rx = FramedRead::new(socket_read, SshDecoder::new());
            let tx = FramedWrite::new(socket_write, SshEncoder::new());

            let preamble =
                stream::once(Ok(SshMsg::new(SshStream::Preamble(preamble), Bytes::new())));

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

            stdout_future
                .select(stdin_future)
                .map(|_| ())
                .map_err(|(err, _)| err)
        })
    }
}
