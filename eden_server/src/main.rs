// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

/// Mononoke endpoint for Eden.
///
/// Uses rest API
///
/// # Request examples
/// ```
/// /REPO/cs/HASH/roottreemanifestid - returns root tree manifest node for the HASH
/// ```
extern crate ascii;
extern crate blobrepo;
extern crate clap;
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_cpupool;
extern crate futures_ext;
extern crate futures_stats;
extern crate hyper;
#[macro_use]
extern crate lazy_static;
extern crate mercurial_types;
extern crate native_tls;
extern crate openssl;
extern crate regex;
extern crate scuba;
extern crate secure_utils;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
#[macro_use]
extern crate slog;
extern crate slog_glog_fmt;
extern crate tokio_core;
extern crate tokio_tls;
extern crate toml;

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::string::ToString;
use std::sync::Arc;
use tokio_core::net::TcpListener;
use tokio_core::reactor::Core;

use blobrepo::{BlobRepo, BlobState, FilesBlobState, RocksBlobState, TestManifoldBlobState};
use clap::App;
use futures::{Future, IntoFuture, Stream};
use futures::sync::oneshot;
use futures_cpupool::CpuPool;
use futures_ext::{BoxFuture, FutureExt, StreamExt};
use futures_stats::{Stats, Timed};
use hyper::StatusCode;
use hyper::server::{Http, Request, Response, Service};
use mercurial_types::{NodeHash, Repo};
use native_tls::TlsAcceptor;
use native_tls::backend::openssl::TlsAcceptorBuilderExt;
use openssl::ssl::{SSL_VERIFY_FAIL_IF_NO_PEER_CERT, SSL_VERIFY_PEER};
use regex::{Captures, Regex};
use scuba::{ScubaClient, ScubaSample};
use slog::{Drain, Level, Logger};
use tokio_tls::TlsAcceptorExt;

pub use failure::{DisplayChain, Error, Result, ResultExt};

type NameToRepo<State> = HashMap<String, Arc<BlobRepo<State>>>;
type UrlParseFunc = fn(Captures) -> Result<ParsedUrl>;

struct Route(Regex, UrlParseFunc);

const SCUBA_TABLE: &'static str = "mononoke_eden_server";
const SCUBA_COL_POLL_TIME: &'static str = "poll_time_ns";
const SCUBA_COL_POLL_COUNT: &'static str = "poll_count";
const SCUBA_COL_HASH: &'static str = "hash";
const SCUBA_COL_OPERATION: &'static str = "operation";
const SCUBA_OPERATION_GET_TREE_CONTENT: &'static str = "get_tree_content";
const SCUBA_OPERATION_GET_MENIFEST: &'static str = "get_root_tree_manifest_id";
const SCUBA_OPERATION_GET_BLOB_CONTENT: &'static str = "get_blob_content";

fn parse_capture<T>(caps: &Captures, index: usize) -> Result<T>
where
    T: FromStr,
    <T as FromStr>::Err: ToString,
    Error: std::convert::From<<T as std::str::FromStr>::Err>,
{
    let s = caps.get(index)
        .expect("incorrect url parsing regex")
        .as_str();
    str::parse::<T>(s).map_err(Error::from)
}

fn parse_root_treemanifest_id_url(caps: Captures) -> Result<ParsedUrl> {
    let repo = parse_capture::<String>(&caps, 1)?;
    let hash = parse_capture::<NodeHash>(&caps, 2)?;
    Ok(ParsedUrl::RootTreeManifestId(repo, hash))
}

fn parse_tree_content_url(caps: Captures) -> Result<ParsedUrl> {
    let repo = parse_capture::<String>(&caps, 1)?;
    let hash = parse_capture::<NodeHash>(&caps, 2)?;
    Ok(ParsedUrl::TreeContent(repo, hash))
}

fn parse_blob_content_url(caps: Captures) -> Result<ParsedUrl> {
    let repo = parse_capture::<String>(&caps, 1)?;
    let hash = parse_capture::<NodeHash>(&caps, 2)?;
    Ok(ParsedUrl::BlobContent(repo, hash))
}

/// Generic url-handling function
/// Accepts vector of tuples (regex, url handling function)
/// If url matches regex then url handling function is called
fn parse_url(url: &str, routes: &[Route]) -> Result<ParsedUrl> {
    for &Route(ref regex, parse_func) in routes {
        if let Some(caps) = regex.captures(url) {
            return parse_func(caps);
        }
    }
    Err(failure::err_msg("malformed url"))
}

enum ParsedUrl {
    RootTreeManifestId(String, NodeHash),
    TreeContent(String, NodeHash),
    BlobContent(String, NodeHash),
}

lazy_static! {
    static ref ROUTES: Vec<Route> = {
        vec![
            // Workaround for https://github.com/rust-lang/rust/issues/20178
            (r"^/(\w+)/cs/(\w+)/roottreemanifestid/?$",
            parse_root_treemanifest_id_url as UrlParseFunc),
            (r"^/(\w+)/treenode/(\w+)/?$", parse_tree_content_url as UrlParseFunc),
            (r"^/(\w+)/blob/(\w+)/?$", parse_blob_content_url as UrlParseFunc),
        ].into_iter().map(|(re, func)| Route(Regex::new(re).expect("bad regex"), func)).collect()
    };
}


#[derive(Serialize)]
struct TreeMetadata {
    hash: NodeHash,
    path: PathBuf,
    #[serde(rename = "type")] ty: mercurial_types::Type,
    size: Option<usize>,
}

impl TreeMetadata {
    fn new(size: Option<usize>, entry: Box<mercurial_types::Entry>) -> TreeMetadata {
        TreeMetadata {
            hash: entry.get_hash().clone(),
            path: PathBuf::from(OsString::from_vec(entry.get_mpath().to_vec())),
            ty: entry.get_type(),
            size,
        }
    }

    fn from_entry(entry: Box<mercurial_types::Entry>) -> BoxFuture<TreeMetadata, Error> {
        if entry.get_type() == mercurial_types::Type::Tree {
            // No need to calculate the size of the directory
            Ok(TreeMetadata::new(None, entry)).into_future().boxify()
        } else {
            entry
                .get_size()
                .map(|size| TreeMetadata::new(size, entry))
                .boxify()
        }
    }
}

struct EdenServer<State> {
    name_to_repo: NameToRepo<State>,
    cpupool: Arc<CpuPool>,
    logger: Logger,
    scuba: Arc<ScubaClient>,
}

impl<State> EdenServer<State>
where
    EdenServer<State>: Service,
    State: BlobState,
{
    fn new(
        name_to_repo: NameToRepo<State>,
        cpupool: Arc<CpuPool>,
        logger: Logger,
    ) -> EdenServer<State> {
        EdenServer {
            name_to_repo,
            cpupool,
            logger,
            scuba: Arc::new(ScubaClient::new(SCUBA_TABLE)),
        }
    }

    fn get_root_tree_manifest_id(
        &self,
        reponame: String,
        hash: &NodeHash,
    ) -> Box<futures::Future<Item = Vec<u8>, Error = Error> + Send> {
        let repo = match self.name_to_repo.get(&reponame) {
            Some(repo) => repo,
            None => {
                return futures::future::err(failure::err_msg("unknown repo")).boxify();
            }
        };
        repo.get_changeset_by_nodeid(&hash)
            .map(|cs| cs.manifestid().to_string().into_bytes())
            .from_err()
            .boxify()
    }

    fn get_tree_content(
        &self,
        reponame: String,
        hash: &NodeHash,
    ) -> Box<futures::Stream<Item = TreeMetadata, Error = Error> + Send> {
        let repo = match self.name_to_repo.get(&reponame) {
            Some(repo) => repo,
            None => {
                return futures::stream::once(Err(failure::err_msg("unknown repo"))).boxify();
            }
        };

        let cpupool = self.cpupool.clone();
        repo.get_manifest_by_nodeid(&hash)
            .map(|manifest| manifest.list())
            .flatten_stream()
            .map(move |entry| cpupool.spawn(TreeMetadata::from_entry(entry)))
            .buffer_unordered(100) // Schedules 100 futures on cpupool
            .from_err()
            .boxify()
    }

    fn get_blob_content(
        &self,
        reponame: String,
        hash: &NodeHash,
    ) -> Box<futures::Future<Item = Vec<u8>, Error = Error> + Send> {
        let repo = match self.name_to_repo.get(&reponame) {
            Some(repo) => repo,
            None => {
                return futures::future::err(failure::err_msg("unknown repo")).boxify();
            }
        };

        repo.get_blob(hash)
            .from_err()
            .and_then(|content| futures::future::ok(content))
            .boxify()
    }
}

/// Add values from the given Stats struct to the given Scuba sample.
fn add_common_stats(sample: &mut ScubaSample, stats: &Stats) {
    sample.add(
        SCUBA_COL_POLL_TIME,
        stats.completion_time.num_milliseconds(),
    );
    if let Some(nanos) = stats.poll_time.num_nanoseconds() {
        sample.add(SCUBA_COL_POLL_TIME, nanos);
    }
    sample.add(SCUBA_COL_POLL_COUNT, stats.poll_count);
}

impl<State> Service for EdenServer<State>
where
    State: BlobState,
{
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Future = futures_ext::BoxFuture<Self::Response, Self::Error>;

    fn call(&self, req: Request) -> Self::Future {
        debug!(self.logger, "request: {}", req.uri().path());

        let scuba = self.scuba.clone();
        let mut sample = ScubaSample::new();

        let mut resp = Response::new();
        let parsed_req = match parse_url(req.uri().path(), &ROUTES) {
            Ok(req) => req,
            Err(err) => {
                resp.set_body(err.to_string());
                resp.set_status(StatusCode::NotFound);
                return futures::future::ok(resp).boxify();
            }
        };

        let result_future = match parsed_req {
            ParsedUrl::RootTreeManifestId(reponame, hash) => {
                sample.add(SCUBA_COL_HASH, hash.to_string());
                sample.add(SCUBA_COL_OPERATION, SCUBA_OPERATION_GET_MENIFEST);
                self.get_root_tree_manifest_id(reponame, &hash)
            }
            ParsedUrl::TreeContent(reponame, hash) => {
                sample.add(SCUBA_COL_HASH, hash.to_string());
                sample.add(SCUBA_COL_OPERATION, SCUBA_OPERATION_GET_TREE_CONTENT);

                self.get_tree_content(reponame, &hash)
                    .map(|metadata| {
                        let err_msg = format!(
                            "failed to get metadata for {}",
                            metadata.path.to_string_lossy()
                        );
                        serde_json::to_value(&metadata).unwrap_or(err_msg.into())
                    })
                    .collect()
                    .map(|entries| {
                        let x: serde_json::Value = entries.into();
                        x.to_string().into_bytes()
                    })
                    .boxify()
            }
            ParsedUrl::BlobContent(reponame, hash) => {
                sample.add(SCUBA_COL_HASH, hash.to_string());
                sample.add(SCUBA_COL_OPERATION, SCUBA_OPERATION_GET_BLOB_CONTENT);
                self.get_blob_content(reponame, &hash)
            }
        };

        result_future
            .then(|res| {
                match res {
                    Ok(output) => {
                        resp.set_body(output);
                    }
                    Err(e) => {
                        let error_msg = format!("{}", DisplayChain::from(&e));
                        resp.set_body(error_msg);
                        resp.set_status(StatusCode::NotFound);
                    }
                };
                futures::future::ok(resp)
            })
            .timed(move |stats, _| {
                add_common_stats(&mut sample, &stats);
                scuba.log(&sample);
            })
            .boxify()
    }
}


// Builds an acceptor that has `accept_async()` method that handles tls handshake
// and returns decrypted stream.
fn build_tls_acceptor<P>(
    cert_pem_file: P,
    private_key_pem_file: P,
    ca_pem_file: P,
) -> Result<TlsAcceptor>
where
    P: AsRef<Path>,
{
    let pkcs12 = secure_utils::build_pkcs12(cert_pem_file, private_key_pem_file)
        .context("failed to build pkcs12")?;
    let mut tlsacceptor_builder = TlsAcceptor::builder(pkcs12)?;

    // Set up client authentication
    {
        let sslacceptorbuilder = tlsacceptor_builder.builder_mut();
        let sslcontextbuilder = sslacceptorbuilder.builder_mut();

        sslcontextbuilder
            .set_ca_file(ca_pem_file)
            .context("cannot set CA file")?;

        // SSL_VERIFY_PEER checks client certificate if it was supplied.
        // Connection is terminated if certificate verification fails.
        // SSL_VERIFY_FAIL_IF_NO_PEER_CERT terminates the connection if client did not return
        // certificate.
        // More about it - https://wiki.openssl.org/index.php/Manual:SSL_CTX_set_verify(3)
        sslcontextbuilder.set_verify(SSL_VERIFY_PEER | SSL_VERIFY_FAIL_IF_NO_PEER_CERT);
    }
    tlsacceptor_builder.build().map_err(Error::from)
}

fn start_server<State, P>(
    addr: &str,
    reponame: String,
    state: State,
    logger: Logger,
    cert: P,
    private_key: P,
    ca_pem_file: P,
) where
    State: BlobState,
    P: AsRef<Path>,
{
    let addr = addr.parse().expect("Failed to parse address");
    let mut map = HashMap::new();
    let repo = BlobRepo::new(state);
    map.insert(reponame, Arc::new(repo));

    let tlsacceptor = build_tls_acceptor(cert, private_key, ca_pem_file);
    let tlsacceptor = match tlsacceptor {
        Ok(tlsacceptor) => tlsacceptor,
        Err(err) => {
            error!(logger, "{}", DisplayChain::from(&err));
            return;
        }
    };

    let mut core = Core::new().expect("cannot create http server core");
    let handle = core.handle();
    let listener = TcpListener::bind(&addr, &handle).expect("cannot bind to the address");
    let incoming = listener.incoming().from_err::<Error>();

    info!(logger, "started eden server");
    let cpupool = Arc::new(CpuPool::new_num_cpus());
    let http_server = Http::new();
    let conns = incoming.for_each(|stream_addr| {
        let (tcp_stream, remote_addr) = stream_addr;
        let http_server = http_server.clone();
        let handle = handle.clone();
        let service = EdenServer::new(map.clone(), cpupool.clone(), logger.clone());
        let logger = logger.clone();
        tlsacceptor.accept_async(tcp_stream).then(move |stream| {
            match stream {
                Ok(stream) => {
                    http_server.bind_connection(&handle, stream, remote_addr, service);
                }
                Err(err) => error!(logger, "accept async failed {}", err),
            };
            Ok(())
        })
    });

    core.run(conns).expect("http server main loop failed");
}

/// Types of repositories supported
#[derive(Clone, Debug, Deserialize)]
enum RawRepoType {
    #[serde(rename = "blob:files")] BlobFiles,
    #[serde(rename = "blob:rocks")] BlobRocks,
    #[serde(rename = "blob:manifold")] BlobManifold,
}

#[derive(Debug, Deserialize)]
struct RawRepoConfig {
    path: Option<PathBuf>,
    manifold_bucket: Option<String>,
    repotype: RawRepoType,
    reponame: String,
    addr: String,
    cert: String,
    private_key: String,
    ca_pem_file: String,
}

fn main() {
    let matches = App::new("Mononoke server for Eden")
        .version("0.1")
        .about("Http server that can answers a few Eden requests")
        .args_from_usage(
            "--config-file=[FILE] 'Toml config file path'
            -d, --debug              'print debug level output'
            ",
        )
        .get_matches();
    let config_file = matches
        .value_of("config-file")
        .expect("config file is not specified");
    let mut config_bytes: Vec<u8> = vec![];
    File::open(config_file)
        .expect("cannot open config file")
        .read_to_end(&mut config_bytes)
        .expect("reading config file failed");
    let config =
        toml::from_slice::<RawRepoConfig>(&config_bytes).expect("reading config file failed");

    let root_logger = {
        let level = if matches.is_present("debug") {
            Level::Debug
        } else {
            Level::Info
        };

        let drain = slog_glog_fmt::default_drain().filter_level(level).fuse();
        Logger::root(drain, o![])
    };

    match config.repotype {
        RawRepoType::BlobFiles => start_server(
            &config.addr,
            config.reponame,
            FilesBlobState::new(&config.path.expect("Please specify a path to the blobrepo"))
                .expect("couldn't open blob state"),
            root_logger.clone(),
            config.cert,
            config.private_key,
            config.ca_pem_file,
        ),
        RawRepoType::BlobRocks => start_server(
            &config.addr,
            config.reponame,
            RocksBlobState::new(&config.path.expect("Please specify a path to the blobrepo"))
                .expect("couldn't open blob state"),
            root_logger.clone(),
            config.cert,
            config.private_key,
            config.ca_pem_file,
        ),
        RawRepoType::BlobManifold => {
            let (sender, receiver) = oneshot::channel();
            // manifold requires a separate detached thread to do the IO, that's why we create a
            // separate thread to handle it.
            std::thread::spawn(move || {
                let mut core = Core::new().expect("cannot create core for manifold");
                sender
                    .send(core.remote())
                    .expect("cannot send remote handle for manifold");
                loop {
                    // loop infinitely; it will be stopped when the whole server is stopped
                    core.turn(None);
                }
            });
            let remote = receiver
                .wait()
                .expect("cannot get remote handle for manifold");
            start_server(
                &config.addr,
                config.reponame,
                TestManifoldBlobState::new(
                    config
                        .manifold_bucket
                        .expect("manifold bucket is not specified"),
                    &remote,
                ).expect("couldn't open blob state"),
                root_logger.clone(),
                config.cert,
                config.private_key,
                config.ca_pem_file,
            )
        }
    };
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_url_parsing() {
        let routes = &ROUTES;
        assert!(parse_url("badurl", &routes).is_err());

        let hash = std::iter::repeat("a").take(40).collect::<String>();
        let correct_url = format!("/repo/cs/{}/roottreemanifestid", hash);
        assert!(parse_url(&correct_url, &routes).is_ok());

        let badhash = std::iter::repeat("x").take(40).collect::<String>();
        let incorrect_url = format!("/repo/cs/{}/roottreemanifestid", badhash);
        assert!(parse_url(&incorrect_url, &routes).is_err());
    }
}
