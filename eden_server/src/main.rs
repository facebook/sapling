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
extern crate bytes;
extern crate clap;
#[macro_use]
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
extern crate time_ext;
extern crate tokio_core;
extern crate tokio_proto;
extern crate tokio_tls;
extern crate toml;

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;
use std::str::FromStr;
use std::string::ToString;
use std::sync::Arc;

use blobrepo::BlobRepo;
use bytes::Bytes;
use clap::App;
use futures::{stream, Future, IntoFuture, Stream};
use futures_cpupool::CpuPool;
use futures_ext::{BoxFuture, FutureExt};
use futures_stats::{Stats, Timed};
use hyper::StatusCode;
use hyper::server::{Http, Request, Response, Service};
use mercurial_types::{Changeset, FileType, HgNodeHash, RepositoryId};
use mercurial_types::nodehash::HgChangesetId;
use native_tls::TlsAcceptor;
use native_tls::backend::openssl::TlsAcceptorBuilderExt;
use openssl::ssl::{SSL_VERIFY_FAIL_IF_NO_PEER_CERT, SSL_VERIFY_PEER};
use regex::{Captures, Regex};
use scuba::{ScubaClient, ScubaSample};
use slog::{Drain, Level, Logger};
use time_ext::DurationExt;
use tokio_proto::TcpServer;
use tokio_tls::proto;

pub use failure::{DisplayChain, Error, Result, ResultExt};

type NameToRepo = HashMap<String, Arc<BlobRepo>>;
type UrlParseFunc = fn(Captures) -> Result<ParsedUrl>;

struct Route(Regex, UrlParseFunc);

const SCUBA_TABLE: &'static str = "mononoke_eden_server";
const SCUBA_COL_ELAPSED_TIME: &'static str = "time_elapsed_ms";
const SCUBA_COL_POLL_TIME: &'static str = "poll_time_us";
const SCUBA_COL_POLL_COUNT: &'static str = "poll_count";
const SCUBA_COL_HASH: &'static str = "hash";
const SCUBA_COL_HOSTNAME: &'static str = "hostname";
const SCUBA_COL_OPERATION: &'static str = "operation";
const SCUBA_COL_REPO: &'static str = "repo";
const SCUBA_OPERATION_GET_TREE_CONTENT: &'static str = "get_tree_content";
const SCUBA_OPERATION_GET_TREE_CONTENT_LIGHT: &'static str = "get_tree_content_light";
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
    let hash = parse_capture::<HgNodeHash>(&caps, 2)?;
    Ok(ParsedUrl::RootTreeHgManifestId(repo, hash))
}

fn parse_tree_content_url(caps: Captures) -> Result<ParsedUrl> {
    let repo = parse_capture::<String>(&caps, 1)?;
    let hash = parse_capture::<HgNodeHash>(&caps, 2)?;
    Ok(ParsedUrl::TreeContent(repo, hash))
}

fn parse_tree_content_light_url(caps: Captures) -> Result<ParsedUrl> {
    let repo = parse_capture::<String>(&caps, 1)?;
    let hash = parse_capture::<HgNodeHash>(&caps, 2)?;
    Ok(ParsedUrl::TreeContentLight(repo, hash))
}

fn parse_blob_content_url(caps: Captures) -> Result<ParsedUrl> {
    let repo = parse_capture::<String>(&caps, 1)?;
    let hash = parse_capture::<HgNodeHash>(&caps, 2)?;
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
    bail_msg!("malformed url")
}

enum ParsedUrl {
    RootTreeHgManifestId(String, HgNodeHash),
    TreeContent(String, HgNodeHash),
    TreeContentLight(String, HgNodeHash),
    BlobContent(String, HgNodeHash),
}

lazy_static! {
    static ref ROUTES: Vec<Route> = {
        vec![
            // Workaround for https://github.com/rust-lang/rust/issues/20178
            (r"^/(\w+)/cs/(\w+)/roottreemanifestid/?$",
            parse_root_treemanifest_id_url as UrlParseFunc),
            (r"^/(\w+)/treenode/(\w+)/?$", parse_tree_content_url as UrlParseFunc),
            (r"^/(\w+)/treenode_simple/(\w+)/?$", parse_tree_content_light_url as UrlParseFunc),
            (r"^/(\w+)/blob/(\w+)/?$", parse_blob_content_url as UrlParseFunc),
        ].into_iter().map(|(re, func)| Route(Regex::new(re).expect("bad regex"), func)).collect()
    };
}

// Earlier versions of mercurial_types::Type had the same definition as MetadataType. These
// instances would get serialized and sent over the JSON API. Convert to this private enum
// to keep the API the same.
#[derive(Clone, Copy, Debug, Serialize)]
enum MetadataType {
    File,
    Executable,
    Symlink,
    Tree,
}

impl From<mercurial_types::Type> for MetadataType {
    fn from(ty: mercurial_types::Type) -> Self {
        match ty {
            mercurial_types::Type::File(FileType::Regular) => MetadataType::File,
            mercurial_types::Type::File(FileType::Symlink) => MetadataType::Symlink,
            mercurial_types::Type::File(FileType::Executable) => MetadataType::Executable,
            mercurial_types::Type::Tree => MetadataType::Tree,
        }
    }
}
#[derive(Serialize)]
struct TreeMetadata {
    hash: HgNodeHash,
    path: PathBuf,
    #[serde(rename = "type")]
    ty: MetadataType,
    size: Option<usize>,
}

impl TreeMetadata {
    fn new(size: Option<usize>, entry: Box<mercurial_types::Entry>) -> TreeMetadata {
        let name = match entry.get_name() {
            Some(name) => name.as_bytes(),
            None => b"",
        };

        TreeMetadata {
            hash: entry.get_hash().into_nodehash().clone(),
            path: PathBuf::from(OsString::from_vec(Vec::from(name))),
            ty: entry.get_type().into(),
            size,
        }
    }

    fn from_entry(
        entry: Box<mercurial_types::Entry>,
        options: &TreeMetadataOptions,
    ) -> BoxFuture<TreeMetadata, Error> {
        if entry.get_type() == mercurial_types::Type::Tree || !options.fetch_size {
            // No need to calculate the size of the directory or if size wasn't requested
            Ok(TreeMetadata::new(None, entry)).into_future().boxify()
        } else {
            entry
                .get_size()
                .map(|size| TreeMetadata::new(size, entry))
                .boxify()
        }
    }
}

struct TreeMetadataOptions {
    fetch_size: bool,
}

struct EdenServer {
    name_to_repo: NameToRepo,
    cpupool: Arc<CpuPool>,
    logger: Logger,
    scuba: Arc<ScubaClient>,
}

impl EdenServer
where
    EdenServer: Service,
{
    fn new(name_to_repo: NameToRepo, cpupool: Arc<CpuPool>, logger: Logger) -> EdenServer {
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
        changesetid: &HgChangesetId,
    ) -> Box<futures::Future<Item = Bytes, Error = Error> + Send> {
        let repo = match self.name_to_repo.get(&reponame) {
            Some(repo) => repo,
            None => {
                return futures::future::err(failure::err_msg("unknown repo")).boxify();
            }
        };
        repo.get_changeset_by_changesetid(&changesetid)
            .map(|cs| {
                Bytes::from(
                    cs.manifestid()
                        .clone()
                        .into_nodehash()
                        .to_string()
                        .into_bytes(),
                )
            })
            .from_err()
            .boxify()
    }

    fn get_tree_content(
        &self,
        reponame: String,
        hash: &HgNodeHash,
        options: TreeMetadataOptions,
    ) -> Box<futures::Future<Item = Bytes, Error = Error> + Send> {
        let repo = match self.name_to_repo.get(&reponame) {
            Some(repo) => repo,
            None => {
                return futures::future::err(failure::err_msg("unknown repo")).boxify();
            }
        };

        let cpupool = self.cpupool.clone();
        repo.get_manifest_by_nodeid(&hash)
            .map(|manifest| stream::iter_ok(manifest.list()))
            .flatten_stream()
            .map(move |entry| cpupool.spawn(TreeMetadata::from_entry(entry, &options)))
            .buffer_unordered(100) // Schedules 100 futures on cpupool
            .from_err()
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
                Bytes::from(x.to_string().into_bytes())
            })
            .boxify()
    }

    fn get_blob_content(
        &self,
        reponame: String,
        hash: &HgNodeHash,
    ) -> Box<futures::Future<Item = Bytes, Error = Error> + Send> {
        let repo = match self.name_to_repo.get(&reponame) {
            Some(repo) => repo,
            None => {
                return futures::future::err(failure::err_msg("unknown repo")).boxify();
            }
        };

        repo.get_file_content(hash)
            .from_err()
            .and_then(|content| futures::future::ok(content.into_bytes()))
            .boxify()
    }
}

/// Add values from the given Stats struct to the given Scuba sample.
fn add_common_stats(sample: &mut ScubaSample, stats: &Stats) {
    sample.add(
        SCUBA_COL_ELAPSED_TIME,
        stats.completion_time.as_millis_unchecked(),
    );
    sample.add(SCUBA_COL_POLL_TIME, stats.poll_time.as_micros_unchecked());
    sample.add(SCUBA_COL_POLL_COUNT, stats.poll_count);
}

impl Service for EdenServer {
    type Request = Request;
    type Response = Response;
    type Error = hyper::Error;
    type Future = futures_ext::BoxFuture<Self::Response, Self::Error>;

    fn call(&self, req: Request) -> Self::Future {
        debug!(self.logger, "request: {}", req.uri().path());

        let scuba = self.scuba.clone();
        let mut sample = ScubaSample::new();
        sample.add(SCUBA_COL_HOSTNAME, req.uri().host().unwrap_or("unknown"));

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
            ParsedUrl::RootTreeHgManifestId(reponame, hash) => {
                sample.add(SCUBA_COL_HASH, hash.to_string());
                sample.add(SCUBA_COL_OPERATION, SCUBA_OPERATION_GET_MENIFEST);
                sample.add(SCUBA_COL_REPO, reponame.clone());
                self.get_root_tree_manifest_id(reponame, &HgChangesetId::new(hash))
            }
            ParsedUrl::TreeContent(reponame, hash) => {
                sample.add(SCUBA_COL_HASH, hash.to_string());
                sample.add(SCUBA_COL_OPERATION, SCUBA_OPERATION_GET_TREE_CONTENT);
                sample.add(SCUBA_COL_REPO, reponame.clone());

                let options = TreeMetadataOptions { fetch_size: true };
                self.get_tree_content(reponame, &hash, options).boxify()
            }
            ParsedUrl::TreeContentLight(reponame, hash) => {
                sample.add(SCUBA_COL_HASH, hash.to_string());
                sample.add(SCUBA_COL_OPERATION, SCUBA_OPERATION_GET_TREE_CONTENT_LIGHT);
                sample.add(SCUBA_COL_REPO, reponame.clone());

                let options = TreeMetadataOptions { fetch_size: false };
                self.get_tree_content(reponame, &hash, options).boxify()
            }
            ParsedUrl::BlobContent(reponame, hash) => {
                sample.add(SCUBA_COL_HASH, hash.to_string());
                sample.add(SCUBA_COL_OPERATION, SCUBA_OPERATION_GET_BLOB_CONTENT);
                sample.add(SCUBA_COL_REPO, reponame.clone());
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
                Ok(())
            })
            .boxify()
    }
}

// Builds an acceptor that has `accept_async()` method that handles tls handshake
// and returns decrypted stream.
fn build_tls_acceptor(ssl: Ssl) -> Result<TlsAcceptor> {
    let pkcs12 =
        secure_utils::build_pkcs12(ssl.cert, ssl.private_key).context("failed to build pkcs12")?;
    let mut tlsacceptor_builder = TlsAcceptor::builder(pkcs12)?;

    // Set up client authentication
    {
        let sslcontextbuilder = tlsacceptor_builder.builder_mut();

        sslcontextbuilder
            .set_ca_file(ssl.ca_pem_file)
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

fn start_server(addr: &str, reponame: String, repo: BlobRepo, logger: Logger, ssl: Ssl) {
    let addr = addr.parse().expect("Failed to parse address");
    let mut map = HashMap::new();
    map.insert(reponame, Arc::new(repo));

    let tlsacceptor = build_tls_acceptor(ssl);
    let tlsacceptor = match tlsacceptor {
        Ok(tlsacceptor) => tlsacceptor,
        Err(err) => {
            error!(logger, "{}", DisplayChain::from(&err));
            return;
        }
    };

    let cpupool = Arc::new(CpuPool::new_num_cpus());
    let protoserver = proto::Server::new(Http::new(), tlsacceptor);
    let tcpserver = TcpServer::new(protoserver, addr);

    info!(logger, "started eden server");
    tcpserver.serve(move || {
        Ok(EdenServer::new(
            map.clone(),
            cpupool.clone(),
            logger.clone(),
        ))
    });
}

/// Types of repositories supported
#[derive(Clone, Debug, Deserialize)]
enum RawRepoType {
    #[serde(rename = "blob:rocks")] BlobRocks,
    #[serde(rename = "blob:manifold")] BlobManifold,
}

#[derive(Debug, Deserialize)]
struct Ssl {
    cert: String,
    private_key: String,
    ca_pem_file: String,
}

#[derive(Debug, Deserialize)]
struct RawRepoConfig {
    path: Option<PathBuf>,
    manifold_bucket: Option<String>,
    manifold_prefix: Option<String>,
    db_address: Option<String>,
    repotype: RawRepoType,
    reponame: String,
    addr: String,
    ssl: Ssl,
    repoid: i32,
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
        RawRepoType::BlobRocks => {
            let path = config.path.expect("Please specify a path to the blobrepo");
            let repo_logger = root_logger.new(o!("repo" => format!("{}", path.display())));
            start_server(
                &config.addr,
                config.reponame,
                BlobRepo::new_rocksdb(repo_logger, &path, RepositoryId::new(config.repoid))
                    .expect("couldn't open blob state"),
                root_logger.clone(),
                config.ssl,
            )
        }
        RawRepoType::BlobManifold => {
            let bucket = config
                .manifold_bucket
                .expect("manifold bucket is not specified");
            let repo_logger = root_logger.new(o!("repo" => format!("{}", bucket)));
            // TODO(stash): bump it when we need it
            let blobstore_cache_size = 100_000_000;
            let changesets_cache_size = 100_000_000;
            let filenodes_cache_size = 100_000_000;
            let io_thread_num = 5;
            let max_concurrent_requests_per_io_thread = 4;
            start_server(
                &config.addr,
                config.reponame,
                BlobRepo::new_manifold(
                    repo_logger,
                    bucket,
                    &config.manifold_prefix.unwrap_or("".into()),
                    RepositoryId::new(config.repoid),
                    &config.db_address.expect("db tier needs to be specified"),
                    blobstore_cache_size,
                    changesets_cache_size,
                    filenodes_cache_size,
                    io_thread_num,
                    max_concurrent_requests_per_io_thread,
                ).expect("couldn't open blob state"),
                root_logger.clone(),
                config.ssl,
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
