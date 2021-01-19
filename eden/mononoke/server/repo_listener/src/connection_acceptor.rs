/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::security_checker::ConnectionsSecurityChecker;
use session_id::generate_session_id;
use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

use anyhow::{anyhow, Context, Error, Result};
use bytes::Bytes;
use cached_config::{ConfigHandle, ConfigStore};
use cloned::cloned;
use failure_ext::SlogKVError;
use fbinit::FacebookInit;
use futures::{channel::oneshot, select_biased};
use futures_ext::BoxStream;
use futures_old::{stream, sync::mpsc, Stream};
use futures_util::compat::Stream01CompatExt;
use futures_util::future::FutureExt;
use futures_util::stream::{StreamExt, TryStreamExt};
use itertools::Itertools;
use lazy_static::lazy_static;
use live_commit_sync_config::CfgrLiveCommitSyncConfig;
use metaconfig_types::CommonConfig;
use openssl::ssl::SslAcceptor;
use permission_checker::{MononokeIdentity, MononokeIdentitySet};
use scribe_ext::Scribe;
use sha1::{Digest, Sha1};
use slog::{debug, error, info, warn, Logger};
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio_openssl::SslStream;
use tokio_util::codec::{FramedRead, FramedWrite};

use cmdlib::monitoring::ReadyFlagService;
use limits::types::MononokeThrottleLimits;
use sshrelay::{
    IoStream, Metadata, Preamble, Priority, SshDecoder, SshEncoder, SshEnvVars, SshMsg, Stdio,
};

use crate::errors::ErrorKind;
use crate::repo_handlers::RepoHandler;
use crate::request_handler::{create_conn_logger, request_handler};

use crate::netspeedtest::{
    create_http_header, handle_http_netspeedtest, parse_netspeedtest_http_params, NetSpeedTest,
};

#[cfg(fbcode_build)]
const HEADER_ENCODED_CLIENT_IDENTITY: &str = "x-fb-validated-client-encoded-identity";
#[cfg(fbcode_build)]
const HEADER_CLIENT_IP: &str = "tfb-orig-client-ip";

const HEADER_CLIENT_DEBUG: &str = "x-client-debug";
const HEADER_WEBSOCKET_KEY: &str = "sec-websocket-key";

// See https://tools.ietf.org/html/rfc6455#section-1.3
const WEBSOCKET_MAGIC_KEY: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

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

/// This function accepts connections, reads Preamble and routes first_line to a thread responsible for
/// a particular repo
pub async fn connection_acceptor(
    fb: FacebookInit,
    test_instance: bool,
    common_config: CommonConfig,
    sockname: String,
    service: ReadyFlagService,
    root_log: Logger,
    repo_handlers: HashMap<String, RepoHandler>,
    tls_acceptor: SslAcceptor,
    terminate_process: oneshot::Receiver<()>,
    config_store: &ConfigStore,
    scribe: Scribe,
) -> Result<()> {
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

    let maybe_live_commit_sync_config = CfgrLiveCommitSyncConfig::new(&root_log, &config_store)
        .map(Option::Some)
        .or_else(|e| if test_instance { Ok(None) } else { Err(e) })?;

    let security_checker = Arc::new(
        ConnectionsSecurityChecker::new(fb, common_config, &repo_handlers, &root_log).await?,
    );
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
                            logger.clone(),
                        )
                        .await {
                            Err(err) => error!(logger, "Failed to accept connection: {}", err.to_string(); SlogKVError(Error::from(err))),
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
    logger: Logger,
) -> Result<()> {
    let addr = sock.peer_addr()?;

    let ssl_socket = tokio_openssl::accept(&tls_acceptor, sock)
        .await
        .with_context(|| format!("tls accept failed: talking to '{}'", addr))?;

    let identities = match ssl_socket.ssl().peer_certificate() {
        Some(cert) => MononokeIdentity::try_from_x509(&cert),
        None => Err(ErrorKind::ConnectionNoClientCertificate.into()),
    }?;

    let mux_result = server_mux(
        addr.ip(),
        ssl_socket,
        &security_checker,
        &identities,
        &logger,
    )
    .await
    .with_context(|| format!("couldn't complete request: talking to '{}'", addr))?;

    match mux_result {
        MuxOutcome::Proceed(stdio, reponame, forwarding_join_handle) => {
            request_handler(
                fb,
                reponame,
                repo_handlers,
                security_checker,
                stdio,
                load_limiting_config,
                addr.ip(),
                maybe_live_commit_sync_config,
                scribe,
            )
            .await?;

            let _ = forwarding_join_handle.await?;
        }
        MuxOutcome::Close => {}
    }

    Ok(())
}

enum MuxOutcome {
    Proceed(
        Stdio,
        String,
        JoinHandle<std::result::Result<(), std::io::Error>>,
    ),
    Close,
}

// As a server, given a stream to a client, return an Io pair with stdin/stdout, and an
// auxillary sink for stderr.
async fn server_mux(
    addr: IpAddr,
    s: SslStream<TcpStream>,
    security_checker: &Arc<ConnectionsSecurityChecker>,
    tls_identities: &MononokeIdentitySet,
    logger: &Logger,
) -> Result<MuxOutcome> {
    let is_trusted = security_checker.check_if_trusted(&tls_identities).await?;
    let (mut rx, mut tx) = tokio::io::split(s);

    // Elaborate scheme to workaround lack of peek() on AsyncRead
    let mut peek_buf = vec![0; 4];
    rx.read_exact(&mut peek_buf[..]).await?;
    let is_http = match peek_buf.as_slice() {
        // For non-HTTP connection this can never start with GET or POST as these
        // are wrapped in NetString encoding and prefixed with a type, so
        // should start with:
        // <number>:\x00
        //
        // For example:
        // 7:\x00hello\n,
        b"GET " | b"POST" => true,
        _ => false,
    };
    let buf_rx = std::io::Cursor::new(peek_buf).chain(BufReader::new(rx));

    let (reponame, maybe_metadata, client_debug, channels) = if is_http {
        let mut persistent_http_buf_rx = buf_rx;
        loop {
            // Max 8KB of headers is in line with common HTTP servers
            // https://www.tutorialspoint.com/What-is-the-maximum-size-of-HTTP-header-values
            let mut limited_reader = persistent_http_buf_rx.take(8192);
            match process_http_headers(&mut limited_reader).await {
                Ok(HttpParse::IoStream(reponame, headers)) => {
                    let websocket_key = calculate_websocket_accept(&headers);
                    let maybe_http_metadata =
                        match try_convert_headers_to_metadata(is_trusted, &headers).await {
                            Ok(value) => value,
                            Err(e) => {
                                tx.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await?;
                                return Err(e);
                            }
                        };

                    let mut res = create_http_header(
                        "101 Mononoke Peer Upgrade",
                        vec![
                            ("Connection: Upgrade", "websocket"),
                            ("Sec-WebSocket-Accept", &websocket_key),
                        ],
                    );
                    res.push_str("\r\n");
                    tx.write_all(res.as_bytes()).await?;

                    let conn = FramedConn::setup(limited_reader.into_inner(), tx);
                    let channels = ChannelConn::setup(conn);
                    break (
                        reponame,
                        maybe_http_metadata,
                        headers.contains_key(HEADER_CLIENT_DEBUG),
                        channels,
                    );
                }
                Ok(HttpParse::NotFound) => {
                    tx.write_all(b"HTTP/1.1 404 Not Found\r\n\r\n").await?;
                    return Ok(MuxOutcome::Close);
                }
                Ok(HttpParse::BadRequest(msg)) => {
                    let len = msg.len();
                    let mut header = create_http_header(
                        "400 Bad Request",
                        vec![("Content-Length", &len.to_string())],
                    );
                    header.push_str("\r\n");
                    header.push_str(&msg);
                    tx.write_all(header.as_bytes()).await?;
                    return Ok(MuxOutcome::Close);
                }
                Ok(HttpParse::HealthCheck) => {
                    tx.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nI_AM_ALIVE")
                        .await?;
                    return Ok(MuxOutcome::Close);
                }
                Ok(HttpParse::NetSpeedTest(params, headers)) => {
                    let mut inner_rx = limited_reader.into_inner();
                    if let Err(err) = handle_http_netspeedtest(&mut inner_rx, &mut tx, params).await
                    {
                        error!(logger, "netspeedtest: {}", err);
                        return Ok(MuxOutcome::Close);
                    }

                    if headers.get("connection") == Some(&"close".to_string()) {
                        return Ok(MuxOutcome::Close);
                    }
                    persistent_http_buf_rx = inner_rx;
                }
                Err(e) => {
                    tx.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await?;
                    return Err(e);
                }
            }
        }
    } else {
        let mut conn = FramedConn::setup(buf_rx, tx);

        let preamble = match conn.rd.next().await.transpose()? {
            Some(maybe_preamble) => {
                if let IoStream::Preamble(preamble) = maybe_preamble.stream() {
                    preamble
                } else {
                    return Err(ErrorKind::NoConnectionPreamble.into());
                }
            }
            None => {
                return Err(ErrorKind::NoConnectionPreamble.into());
            }
        };

        let channels = ChannelConn::setup(conn);

        let metadata = if is_trusted {
            // Relayed through trusted proxy. Proxy authenticates end client and generates
            // preamble so we can trust it. Use identity provided in preamble.
            Some(try_convert_preamble_to_metadata(&preamble, addr, &channels.logger).await?)
        } else {
            None
        };

        (preamble.reponame.clone(), metadata, false, channels)
    };

    let metadata = if let Some(metadata) = maybe_metadata {
        metadata
    } else {
        // Most likely client is not trusted. Use TLS connection
        // cert as identity.
        Metadata::new(
            Some(&generate_session_id().to_string()),
            is_trusted,
            tls_identities.clone(),
            Priority::Default,
            client_debug,
            Some(addr),
        )
        .await
    };

    if metadata.client_debug() {
        info!(&channels.logger, "{:#?}", metadata; "remote" => "true");
    }

    Ok(MuxOutcome::Proceed(
        Stdio {
            metadata,
            stdin: channels.stdin,
            stdout: channels.stdout,
            stderr: channels.stderr,
        },
        reponame,
        channels.join_handle,
    ))
}

// See https://tools.ietf.org/html/rfc6455#section-1.3
fn calculate_websocket_accept(headers: &HashMap<String, String>) -> String {
    let mut sha1 = Sha1::new();

    // This is OK to fall back to empty, because we only need to give
    // this header, if it's asked for. In case of hg<->mononoke with
    // no Proxygen in between, this header will be missing and the result
    // ignored.
    sha1.input(
        headers
            .get(HEADER_WEBSOCKET_KEY)
            .map(|s| s.to_owned())
            .unwrap_or_default()
            .as_bytes(),
    );
    sha1.input(WEBSOCKET_MAGIC_KEY.as_bytes());
    let hash: [u8; 20] = sha1.result().into();
    base64::encode(&hash)
}

struct FramedConn<R, W> {
    rd: FramedRead<R, SshDecoder>,
    wr: FramedWrite<W, SshEncoder>,
}

impl<R, W> FramedConn<R, W>
where
    R: AsyncRead + Send + Sync + std::marker::Unpin + 'static,
    W: AsyncWrite + Send + Sync + std::marker::Unpin + 'static,
{
    pub fn setup(rd: R, wr: W) -> Self {
        let rd = FramedRead::new(rd, SshDecoder::new());
        let wr = FramedWrite::new(wr, SshEncoder::new());
        Self { rd, wr }
    }
}

struct ChannelConn {
    stdin: BoxStream<Bytes, io::Error>,
    stdout: mpsc::Sender<Bytes>,
    stderr: mpsc::UnboundedSender<Bytes>,
    logger: Logger,
    join_handle: JoinHandle<Result<(), io::Error>>,
}

impl ChannelConn {
    fn setup<R, W>(conn: FramedConn<R, W>) -> Self
    where
        R: AsyncRead + Send + Sync + std::marker::Unpin + 'static,
        W: AsyncWrite + Send + Sync + std::marker::Unpin + 'static,
    {
        let FramedConn { rd, wr } = conn;

        let stdin = Box::new(rd.compat().filter_map(|s| {
            if s.stream() == IoStream::Stdin {
                Some(s.data())
            } else {
                None
            }
        }));

        let (stdout, stderr, join_handle) = {
            let (otx, orx) = mpsc::channel(1);
            let (etx, erx) = mpsc::unbounded();

            let orx = orx
                .map(|blob| split_bytes_in_chunk(blob, CHUNK_SIZE))
                .flatten()
                .map(|v| SshMsg::new(IoStream::Stdout, v));
            let erx = erx
                .map(|blob| split_bytes_in_chunk(blob, CHUNK_SIZE))
                .flatten()
                .map(|v| SshMsg::new(IoStream::Stderr, v));

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

        let logger = create_conn_logger(stderr.clone(), None, None);

        ChannelConn {
            stdin,
            stdout,
            stderr,
            logger,
            join_handle,
        }
    }
}

enum HttpParse {
    IoStream(String, HashMap<String, String>),
    NetSpeedTest(NetSpeedTest, HashMap<String, String>),
    NotFound,
    HealthCheck,
    BadRequest(String),
}

async fn process_http_headers<R>(limited_reader: &mut R) -> Result<HttpParse>
where
    R: AsyncBufRead + std::marker::Unpin,
{
    let mut headers: HashMap<String, String> = HashMap::new();
    let mut first_line: Option<String> = None;

    loop {
        // read_line breaks on \n, which is included in the buffer, so any
        // newline characters will need to be stripped from the buffer. Depending
        // on the client, \n can be prepended with \r, which is the recommended HTTP
        // specification, so take this in to account
        let mut line_buf = String::new();
        limited_reader.read_line(&mut line_buf).await?;
        if line_buf.ends_with('\n') {
            line_buf.pop();
        }
        if line_buf.ends_with('\r') {
            line_buf.pop();
        }

        if first_line.is_none() {
            first_line = Some(line_buf.to_ascii_lowercase());
        } else if !line_buf.is_empty() {
            let (key, value) = line_buf
                .splitn(2, ':')
                .map(|s| s.trim())
                .collect_tuple()
                .ok_or_else(|| anyhow!("invalid header tuple: {}", line_buf))?;
            headers.insert(key.to_ascii_lowercase(), value.to_string());
        } else {
            // HTTP headers are closed by an empty line, so break once we have all our headers
            break;
        }
    }

    if let Some(first_line) = first_line {
        if headers.get("upgrade").map(|s| s.to_ascii_lowercase()) == Some("websocket".to_string()) {
            let reponame = first_line
                .split_ascii_whitespace()
                .nth(1)
                .map(|s| s.trim_matches('/').to_string())
                .ok_or_else(|| anyhow!("missing reponame from request"))?;

            return Ok(HttpParse::IoStream(reponame, headers));
        }

        let mut tokens = first_line.split_ascii_whitespace();
        let method = tokens.next().map(|s| s.to_ascii_uppercase());
        let path = tokens.next();

        if method.as_deref() == Some("GET")
            && (path.as_deref() == Some("/") || path.as_deref() == Some("/health_check"))
        {
            return Ok(HttpParse::HealthCheck);
        }

        if path.as_deref() == Some("/netspeedtest") {
            match parse_netspeedtest_http_params(&headers, method) {
                Ok(params) => {
                    return Ok(HttpParse::NetSpeedTest(params, headers));
                }
                Err(err) => {
                    return Ok(HttpParse::BadRequest(format!("netspeedtest: {}", err)));
                }
            }
        }

        return Ok(HttpParse::NotFound);
    }

    Err(anyhow!("invalid http request"))
}

#[cfg(fbcode_build)]
async fn try_convert_headers_to_metadata(
    is_trusted: bool,
    headers: &HashMap<String, String>,
) -> Result<Option<Metadata>> {
    use percent_encoding::percent_decode;

    if !is_trusted {
        return Ok(None);
    }

    if let (Some(encoded_identities), Some(client_address)) = (
        headers.get(HEADER_ENCODED_CLIENT_IDENTITY),
        headers.get(HEADER_CLIENT_IP),
    ) {
        let json_identities = percent_decode(encoded_identities.as_bytes()).decode_utf8()?;
        let identities = MononokeIdentity::try_from_json_encoded(&json_identities)?;
        let ip_addr = client_address.parse::<IpAddr>()?;

        // In the case of HTTP proxied/trusted requests we only have the
        // guarantee that we can trust the forwarded credentials. Beyond
        // this point we can't trust anything else, ACL checks have not
        // been performed, so set 'is_trusted' to 'false' here to enforce
        // further checks.
        Ok(Some(
            Metadata::new(
                Some(&generate_session_id().to_string()),
                false,
                identities,
                Priority::Default,
                headers.contains_key(HEADER_CLIENT_DEBUG),
                Some(ip_addr),
            )
            .await,
        ))
    } else {
        Ok(None)
    }
}

#[cfg(not(fbcode_build))]
async fn try_convert_headers_to_metadata(
    _is_trusted: bool,
    _headers: &HashMap<String, String>,
) -> Result<Option<Metadata>> {
    Ok(None)
}

async fn try_convert_preamble_to_metadata(
    preamble: &Preamble,
    addr: IpAddr,
    conn_log: &Logger,
) -> Result<Metadata> {
    let vars = SshEnvVars::from_map(&preamble.misc);
    let client_ip = match vars.ssh_client {
        Some(ssh_client) => ssh_client
            .split_whitespace()
            .next()
            .and_then(|ip| ip.parse::<IpAddr>().ok())
            .unwrap_or(addr),
        None => addr,
    };

    let priority = match Priority::extract_from_preamble(&preamble) {
        Ok(Some(p)) => {
            info!(&conn_log, "Using priority: {}", p; "remote" => "true");
            p
        }
        Ok(None) => Priority::Default,
        Err(e) => {
            warn!(&conn_log, "Could not parse priority: {}", e; "remote" => "true");
            Priority::Default
        }
    };

    let identity = {
        #[cfg(fbcode_build)]
        {
            // SSH Connections are either authentication via ssh certificate principals or
            // via some form of keyboard-interactive. In the case of certificates we should always
            // rely on these. If they are not present, we should fallback to use the unix username
            // as the primary principal.
            let ssh_identities = match vars.ssh_cert_principals {
                Some(ssh_identities) => ssh_identities,
                None => preamble
                    .unix_name()
                    .ok_or_else(|| anyhow!("missing username and principals from preamble"))?
                    .to_string(),
            };

            MononokeIdentity::try_from_ssh_encoded(&ssh_identities)?
        }
        #[cfg(not(fbcode_build))]
        {
            use maplit::btreeset;
            btreeset! { MononokeIdentity::new(
                "USER",
               preamble
                    .unix_name()
                    .ok_or_else(|| anyhow!("missing username from preamble"))?
                    .to_string(),
            )?}
        }
    };

    Ok(Metadata::new(
        preamble.misc.get("session_uuid"),
        true,
        identity,
        priority,
        preamble
            .misc
            .get("client_debug")
            .map(|debug| debug.parse::<bool>().unwrap_or_default())
            .unwrap_or_default(),
        Some(client_ip),
    )
    .await)
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
