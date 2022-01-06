/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::env::var;
use std::io as std_io;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::time::Duration;

use anyhow::{bail, format_err, Context, Error, Result};
use bytes::Bytes;
use clap::ArgMatches;
use dns_lookup::lookup_addr;
use failure_ext::{err_downcast_ref, SlogKVError};
use fbinit::FacebookInit;
use futures::compat::Future01CompatExt;
use futures::sink::SinkExt;
use futures::stream::{self, StreamExt, TryStreamExt};
use futures::{pin_mut, select};
use futures_ext::StreamExt as OtherStreamExt;
use futures_old::{future, Sink};
use futures_stats::TimedFutureExt;
use futures_util::future::FutureExt;
use hostname::get_hostname;
use libc::c_ulong;
use openssl::{
    nid::Nid,
    ssl::{SslConnector, SslMethod, SslVerifyMode},
    x509::{X509StoreContextRef, X509VerifyResult},
};
use permission_checker::MononokeIdentity;
use scuba_ext::MononokeScubaSampleBuilder;
use secure_utils::{build_identity, read_x509};
use session_id::generate_session_id;
use slog::{debug, error, o, Drain, Logger};
use sshrelay::{IoStream, Preamble, Priority, SshDecoder, SshEncoder, SshEnvVars, SshMsg};
use std::str::FromStr;
use tokio::io::{self, BufReader, Stderr, Stdin, Stdout};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_openssl::SslStream;
use tokio_util::codec::{BytesCodec, FramedRead, FramedWrite};
use users::get_current_username;

const X509_R_CERT_ALREADY_IN_HASH_TABLE: c_ulong = 185057381;
const BUFSZ: usize = 8192;
const NUMBUFS: usize = 50000; // This seems high
pub const ARG_COMMON_NAME: &str = "common-name";
pub const ARG_SERVER_CERT_IDENTITY: &str = "server-cert-identity";

// Wait for up to 1sec to let Scuba flush its data to the server.
const SCUBA_TIMEOUT: Duration = Duration::from_millis(1000);

pub async fn cmd(
    fb: Option<FacebookInit>,
    main: &ArgMatches<'_>,
    sub: &ArgMatches<'_>,
) -> Result<(), Error> {
    if sub.is_present("stdio") {
        if let Some(repo) = main.value_of("repository") {
            let query_string = main.value_of("query-string").unwrap_or("");
            let mononoke_path = sub.value_of("mononoke-path").unwrap();
            let priority = main
                .value_of("priority")
                .map(|p| p.parse())
                .transpose()?
                .unwrap_or(Priority::Default);
            let show_session_output = !main.is_present("no-session-output");

            let cert = sub
                .value_of("cert")
                .expect("certificate file is not specified");
            let private_key = sub
                .value_of("private-key")
                .expect("private key file is not specified");
            let ca_pem = sub
                .value_of("ca-pem")
                .expect("Cental authority pem file is not specified");
            let expected_server_identity = ExpectedServerIdentity::new(sub)?;
            let insecure = sub.is_present("insecure");
            let is_remote_proxy = main.is_present("remote-proxy");
            let scuba_table = main.value_of("scuba-table");
            let mock_username = sub.value_of("mock-username");
            let client_debug = sub.is_present("client-debug");

            let mut scuba_logger = match fb {
                Some(fb) => MononokeScubaSampleBuilder::with_opt_table(
                    fb,
                    scuba_table.map(|v| v.to_owned()),
                ),
                None => MononokeScubaSampleBuilder::with_discard(),
            };
            scuba_logger.add_common_server_data();

            let client_logger = {
                let drain = slog_term::PlainSyncDecorator::new(std::io::stderr());
                let drain = slog_term::FullFormat::new(drain).build();
                Logger::root(drain.ignore_res(), o!())
            };


            return StdioRelay {
                path: mononoke_path,
                repo,
                query_string,
                cert,
                private_key,
                ca_pem,
                expected_server_identity,
                insecure,
                is_remote_proxy,
                scuba_logger,
                mock_username,
                show_session_output,
                priority,
                client_debug,
                client_logger,
            }
            .run()
            .await;
        }
        return Err(format_err!("Missing repository"));
    }
    return Err(format_err!("Only stdio server is supported"));
}

#[derive(Clone)]
enum ExpectedServerIdentity {
    CommonName(String),
    Identity(MononokeIdentity),
}

impl ExpectedServerIdentity {
    fn new(matches: &ArgMatches<'_>) -> Result<Self, Error> {
        let maybe_common_name = matches.value_of(ARG_COMMON_NAME);
        let maybe_server_cert_identity = matches.value_of(ARG_SERVER_CERT_IDENTITY);

        match (maybe_common_name, maybe_server_cert_identity) {
            (Some(common_name), None) => {
                Ok(ExpectedServerIdentity::CommonName(common_name.to_string()))
            }
            (None, Some(server_cert_identity)) => Ok(ExpectedServerIdentity::Identity(
                MononokeIdentity::from_str(server_cert_identity)?,
            )),
            (Some(_), Some(_)) => Err(format_err!(
                "both common-name and server-cert-identity are specified"
            )),
            (None, None) => Err(format_err!(
                "neither common-name nor server-cert-identity are specified"
            )),
        }
    }

    fn maybe_get_common_name(&self) -> Option<&str> {
        use ExpectedServerIdentity::*;

        match self {
            CommonName(common_name) => Some(&common_name),
            Identity(_) => None,
        }
    }
}

struct StdioRelay<'a> {
    path: &'a str,
    repo: &'a str,
    query_string: &'a str,
    cert: &'a str,
    private_key: &'a str,
    ca_pem: &'a str,
    expected_server_identity: ExpectedServerIdentity,
    insecure: bool,
    is_remote_proxy: bool,
    scuba_logger: MononokeScubaSampleBuilder,
    mock_username: Option<&'a str>,
    show_session_output: bool,
    priority: Priority,
    client_debug: bool,
    client_logger: Logger,
}

impl<'a> StdioRelay<'a> {
    async fn run(mut self) -> Result<(), Error> {
        let session_uuid = generate_session_id();
        let unix_username = if let Some(mock_username) = self.mock_username {
            Some(mock_username.to_string())
        } else {
            get_current_username().and_then(|os_str| os_str.into_string().ok())
        };
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
            get_hostname().ok()
        };

        let mut preamble = Preamble::new(
            self.repo.to_owned(),
            session_uuid.clone(),
            unix_username,
            source_hostname,
            SshEnvVars::new_from_env(),
        );

        if self.client_debug {
            preamble
                .misc
                .insert("client_debug".to_string(), "true".to_string());
        }

        self.priority.add_to_preamble(&mut preamble);

        self.scuba_logger.add_preamble(&preamble);

        let stdin = io::stdin();
        let stdout = io::stdout();
        let stderr = io::stderr();

        if self.show_session_output {
            // This message is parsed on various places by Sandcastle to determine it was served by
            // Mononoke. This message should remain exactly like this, therefor we serve Sandcastle
            // and use the fallback scenario for when query string is empty to show this message. Once
            // hg-ssh-wrapper everywhere is updated to always pass along the query string, we can make
            // this a non-optional parameter and show the user friendly message on empty query string.
            if self.query_string.is_empty() || self.query_string.contains("sandcastle") {
                debug!(
                    self.client_logger,
                    "Session with Mononoke started with uuid: {}", session_uuid
                );
            } else {
                eprintln!("mononoke session: {}", session_uuid);
            }
        }

        self.scuba_logger
            .log_with_msg("Hgcli proxy - Connected", None);

        let (stats, result) = self
            .internal_run(preamble, stdin, stdout, stderr)
            .timed()
            .await;
        self.scuba_logger.add_future_stats(&stats);
        match result {
            Ok(_) => self
                .scuba_logger
                .log_with_msg("Hgcli proxy - Success", None),
            Err(err) => {
                self.scuba_logger
                    .log_with_msg("Hgcli proxy - Failure", format!("{:#?}", err));
                error!(self.client_logger, "Error in hgcli proxy"; SlogKVError(err));
            }
        }
        self.scuba_logger.flush(SCUBA_TIMEOUT);
        Ok(())
    }

    async fn establish_connection(&self) -> Result<Pin<Box<SslStream<TcpStream>>>, Error> {
        let path = self.path.to_owned();
        let expected_server_identity = self.expected_server_identity.clone();
        let client_logger = self.client_logger.clone();
        let scuba_logger = self.scuba_logger.clone();

        let connector = {
            let mut connector = SslConnector::builder(SslMethod::tls())?;

            if self.insecure {
                connector.set_verify(SslVerifyMode::NONE);
            } else {
                connector.set_verify_callback(
                    SslVerifyMode::PEER,
                    move |preverify_ok, x509_ctx_ref| {
                        // error_depth is the depth of the certificate that we need to check,
                        // and we are interested in doing additional verification only for the
                        // cert with depth == 0 (i.e. it's the actual certificate of the server
                        // and not another certificate in the chain).
                        if !preverify_ok || x509_ctx_ref.error_depth() != 0 {
                            return preverify_ok;
                        }


                        let verification_result = match expected_server_identity {
                            ExpectedServerIdentity::CommonName(ref common_name) => {
                                verify_common_name(&common_name, x509_ctx_ref)
                            }
                            ExpectedServerIdentity::Identity(ref identity) => {
                                #[cfg(fbcode_build)]
                                {
                                    verify_service_identity::verify_service_identity(
                                        identity,
                                        x509_ctx_ref,
                                    )
                                }
                                #[cfg(not(fbcode_build))]
                                {
                                    let _ = identity;
                                    Err("verifying service identity is not supported".to_string())
                                }
                            }
                        };

                        match verification_result {
                            Ok(()) => true,
                            Err(err_msg) => {
                                error!(client_logger, "{}", err_msg);
                                scuba_logger.clone().log_with_msg(
                                    "Hgcli proxy - certificate verification failure",
                                    err_msg,
                                );
                                x509_ctx_ref.set_error(X509VerifyResult::APPLICATION_VERIFICATION);
                                false
                            }
                        }
                    },
                );
            }

            let pkcs12 = build_identity(self.cert.to_owned(), self.private_key.to_owned())?;
            connector.set_certificate(&pkcs12.cert)?;
            connector.set_private_key(&pkcs12.pkey)?;

            connector.set_alpn_protos(&alpn::alpn_format(alpn::HGCLI_ALPN)?)?;

            // add root certificate

            connector
                .cert_store_mut()
                .add_cert(read_x509(self.ca_pem)?)
                .or_else(|err| {
                    let mut failed = true;
                    {
                        let errors = err.errors();
                        if errors.len() == 1 {
                            if errors[0].code() == X509_R_CERT_ALREADY_IN_HASH_TABLE {
                                // Do not fail if certificate has already been added since it's
                                // not really an error
                                failed = false;
                            }
                        }
                    }
                    if failed {
                        let err: Error = err.into();
                        Err(err)
                    } else {
                        Ok(())
                    }
                })?;

            connector.build()
        };

        let addr: SocketAddr = path.parse()?;
        let sock = TcpStream::connect(&addr)
            .await
            .with_context(|| format!("failed: connecting to '{}'", path))?;

        let mut configured_connector = connector.configure()?;
        // Don't verify the hostname since we have a callback that verifies the
        // common name
        configured_connector.set_verify_hostname(false);
        let common_name = match self.expected_server_identity.maybe_get_common_name() {
            Some(common_name) => common_name,
            None => {
                configured_connector.set_use_server_name_indication(false);
                ""
            }
        };

        let ssl = configured_connector.into_ssl(&common_name)?;

        let ssl = SslStream::new(ssl, sock)?;
        let mut ssl = Box::pin(ssl);

        ssl.as_mut()
            .connect()
            .await
            .with_context(|| format!("tls failed: talking to '{}'", path))?;

        Ok(ssl)
    }

    async fn internal_run(
        &self,
        preamble: Preamble,
        stdin: Stdin,
        stdout: Stdout,
        stderr: Stderr,
    ) -> Result<(), Error> {
        let socket = timeout(Duration::from_secs(15), self.establish_connection())
            .await
            .with_context(|| format!("timed out: connecting to '{}'", self.path))??;

        // Wrap the socket with the ssh codec
        let (socket_read, socket_write) = tokio::io::split(socket);
        let rx = FramedRead::new(socket_read, SshDecoder::new());
        let tx = FramedWrite::new(socket_write, SshEncoder::new(None)?);

        let preamble =
            stream::once(async { Ok(SshMsg::new(IoStream::Preamble(preamble), Bytes::new())) });

        // Start a task to copy from stdin to the socket
        let stdin = BufReader::with_capacity(BUFSZ, stdin);
        let stdin = FramedRead::new(stdin, BytesCodec::new());
        let stdin_future = preamble
            .chain(stdin.map_ok(|buf| SshMsg::new(IoStream::Stdin, buf.freeze())))
            .forward(tx.buffer(NUMBUFS));

        // A task to copy from the socket, then use streamfork() to split the
        // input between stdout and stderr.
        let stdout = FramedWrite::new(stdout, BytesCodec::new()).buffer(NUMBUFS);
        let stderr = FramedWrite::new(stderr, BytesCodec::new()).buffer(NUMBUFS);

        let stdout_future = rx
            .compat()
            .streamfork(
                // a sink each for stdout and stderr, prefixed with With to remove the
                // SshMsg framing and expose the raw data
                stdout
                    .compat()
                    .with(|m: SshMsg| future::ok::<_, Error>(m.data())),
                stderr
                    .compat()
                    .with(|m: SshMsg| future::ok::<_, Error>(m.data())),
                |msg| -> Result<bool> {
                    // Select a sink based on the stream
                    match msg.stream() {
                        IoStream::Stdout => Ok(false),
                        IoStream::Stderr => Ok(true),
                        bad => bail!("Bad stream: {:?}", bad),
                    }
                },
            )
            .compat();

        pin_mut!(stdin_future, stdout_future);

        let res = select! {
            res = stdout_future.fuse() => match res {
                Ok(_) => Ok(()),
                Err(err) => {
                    // TODO(stash): T39586884 "Connection reset" can happen in case
                    // of error on the Mononoke server
                    let res = err_downcast_ref!(
                        err,
                        ioerr: std_io::Error => ioerr.kind() == ::std::io::ErrorKind::ConnectionReset,
                    );
                    match res {
                        Some(true) => Ok(()),
                        _ => Err(err),
                    }
                }
            },
            _res = stdin_future.fuse() => Ok(()),
        };

        res
    }
}

fn verify_common_name(
    ssl_common_name: &str,
    x509_ctx_ref: &mut X509StoreContextRef,
) -> Result<(), String> {
    let cert = match x509_ctx_ref.current_cert() {
        Some(cert) => cert,
        None => {
            let err_msg = "certificate to verify not found";
            return Err(err_msg.to_string());
        }
    };

    // Check that we have the correct common name
    let name = cert.subject_name();
    let mut entries = name.entries_by_nid(Nid::COMMONNAME);
    match entries.next() {
        Some(entry) => match entry.data().as_utf8() {
            Ok(s) => {
                let s: &str = s.as_ref();
                if ssl_common_name == s {
                    Ok(())
                } else {
                    let err_msg = format!(
                        "invalid common name. Expected {}, found {}",
                        ssl_common_name, s
                    );
                    Err(err_msg)
                }
            }
            Err(_) => {
                let err_msg = "cannot parse common name as utf-8";
                Err(err_msg.to_string())
            }
        },
        None => {
            let err_msg = "common name not found in certificate";
            Err(err_msg.to_string())
        }
    }
}

#[cfg(fbcode_build)]
mod verify_service_identity {
    use super::*;
    use identity::Identity;
    use identity_ext::x509::get_identities;

    pub fn verify_service_identity(
        expected_identity: &MononokeIdentity,
        x509_ctx_ref: &mut X509StoreContextRef,
    ) -> Result<(), String> {
        let cert = match x509_ctx_ref.current_cert() {
            Some(cert) => cert,
            None => {
                let err_msg = "certificate to verify not found";
                return Err(err_msg.to_string());
            }
        };

        let identities = get_identities(cert)
            .map_err(|err| format!("failed get identities from certificate: {:#}", err))?;

        let expected_identity = Identity::from(expected_identity);
        if identities.contains(&expected_identity) {
            Ok(())
        } else {
            let err_msg = format!(
                "couldn't find identity {} in certificate that has identities {:?}",
                expected_identity, identities
            );
            Err(err_msg)
        }
    }
}
