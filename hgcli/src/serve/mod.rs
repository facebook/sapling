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
use tokio_core::reactor::Core;
use tokio_io::AsyncRead;
use tokio_io::codec::{FramedRead, FramedWrite};
use tokio_tls::TlsConnectorExt;
use uuid;

use tokio::net::TcpStream;

use clap::ArgMatches;

use errors::*;

use failure::SlogKVError;
use fbwhoami;
use futures_ext::StreamExt;
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use secure_utils::build_pkcs12;
use sshrelay::{Preamble, SenderBytesWrite, SshDecoder, SshEncoder, SshMsg, SshStream, Stdio};

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
        bail_msg!("Missing repository");
    }
    bail_msg!("Only stdio server is supported");
}

struct StdioRelay<'a> {
    path: &'a str,
    repo: &'a str,
    cert: String,
    private_key: String,
    ca_pem: String,
    ssl_common_name: String,
    is_remote_proxy: bool,
    scuba_table: Option<&'a str>,
}

impl<'a> StdioRelay<'a> {
    fn run(self) -> Result<()> {
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

        let res = self.internal_run(stdio);
        match res {
            Ok(_) => res,
            Err(err) => {
                let err_msg = format!("{:#?}", err);
                scuba_logger.log_with_msg("Hgcli proxy - error", err_msg.clone());
                error!(client_logger, "Error in hgcli proxy"; SlogKVError(err));
                Err(format_err!("{}", err_msg))
            }
        }
    }

    fn internal_run(self, stdio: Stdio) -> Result<()> {
        let StdioRelay {
            path,
            cert,
            private_key,
            ca_pem,
            ssl_common_name,
            ..
        } = self;

        let Stdio {
            preamble,
            stdin,
            stdout,
            stderr,
        } = stdio;

        let mut reactor = Core::new()?;

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
}
