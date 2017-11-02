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
#[macro_use]
extern crate error_chain;
extern crate futures;
extern crate futures_cpupool;
extern crate futures_ext;
extern crate hyper;
#[macro_use]
extern crate lazy_static;
extern crate mercurial_types;
extern crate regex;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
#[macro_use]
extern crate slog;
extern crate slog_glog_fmt;
extern crate tokio_core;

use std::collections::HashMap;
use std::error;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::string::ToString;
use std::sync::Arc;
use tokio_core::reactor::Core;

use blobrepo::{BlobRepo, BlobState, FilesBlobState, RocksBlobState, TestManifoldBlobState};
use clap::App;
use error_chain::ChainedError;
use futures::{Future, IntoFuture, Stream};
use futures::sync::oneshot;
use futures_cpupool::CpuPool;
use futures_ext::{BoxFuture, FutureExt, StreamExt};
use hyper::StatusCode;
use hyper::server::{Http, Request, Response, Service};
use mercurial_types::{NodeHash, Repo};
use regex::{Captures, Regex};
use slog::{Drain, Level, Logger};

mod errors;

use errors::*;

type NameToRepo<State> = HashMap<String, Arc<BlobRepo<State>>>;
type UrlParseFunc = fn(Captures) -> Result<ParsedUrl>;

struct Route(Regex, UrlParseFunc);

fn parse_capture<T>(caps: &Captures, index: usize) -> Result<T>
where
    T: FromStr,
    <T as FromStr>::Err: ToString,
    errors::Error: std::convert::From<<T as std::str::FromStr>::Err>,
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
    Err("malformed url".into())
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
            (r"^/(\w+)/cs/(\w+)/roottreemanifestid$",
            parse_root_treemanifest_id_url as UrlParseFunc),
            (r"^/(\w+)/treenode/(\w+)/$", parse_tree_content_url as UrlParseFunc),
            (r"^/(\w+)/blob/(\w+)/$", parse_blob_content_url as UrlParseFunc),
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
    fn new<E>(size: Option<usize>, entry: Box<mercurial_types::Entry<Error = E>>) -> TreeMetadata
    where
        E: error::Error + Send + 'static,
    {
        TreeMetadata {
            hash: entry.get_hash().clone(),
            path: PathBuf::from(OsString::from_vec(entry.get_mpath().to_vec())),
            ty: entry.get_type(),
            size,
        }
    }

    fn from_entry(
        entry: Box<mercurial_types::Entry<Error = blobrepo::Error>>,
    ) -> BoxFuture<TreeMetadata, blobrepo::Error> {
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
                return futures::future::err("unknown repo".into()).boxify();
            }
        };
        repo.get_changeset_by_nodeid(&hash)
            .map(|cs| cs.manifestid().to_string().into_bytes())
            .map_err(Error::from)
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
                return futures::stream::once(Err("unknown repo".into())).boxify();
            }
        };

        let cpupool = self.cpupool.clone();
        repo.get_manifest_by_nodeid(&hash)
            .map(|manifest| manifest.list())
            .flatten_stream()
            .map(move |entry| cpupool.spawn(TreeMetadata::from_entry(entry)))
            .buffer_unordered(100) // Schedules 100 futures on cpupool
            .map_err(Error::from)
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
                return futures::future::err("unknown repo".into()).boxify();
            }
        };

        repo.get_file_blob(hash)
            .map_err(Error::from)
            .and_then(|content| futures::future::ok(content))
            .boxify()
    }
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
                self.get_root_tree_manifest_id(reponame, &hash)
            }
            ParsedUrl::TreeContent(reponame, hash) => self.get_tree_content(reponame, &hash)
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
                .boxify(),
            ParsedUrl::BlobContent(reponame, hash) => self.get_blob_content(reponame, &hash),
        };
        result_future
            .then(|res| {
                match res {
                    Ok(output) => {
                        resp.set_body(output);
                    }
                    Err(e) => {
                        let error_msg = format!("{}", e.display_chain());
                        resp.set_body(error_msg);
                        resp.set_status(StatusCode::NotFound);
                    }
                };
                futures::future::ok(resp)
            })
            .boxify()
    }
}

fn start_server<State>(addr: &str, reponame: String, state: State, logger: Logger)
where
    State: BlobState,
{
    let addr = addr.parse().expect("Failed to parse address");
    let mut map = HashMap::new();
    let repo = BlobRepo::new(state);
    map.insert(reponame, Arc::new(repo));

    info!(logger, "started eden server");
    let cpupool = Arc::new(CpuPool::new_num_cpus());
    let func = move || Ok(EdenServer::new(map.clone(), cpupool.clone(), logger.clone()));
    let server = Http::new().bind(&addr, func).expect("Failed to run server");
    server.run().expect("Error while running service");
}

fn main() {
    let matches = App::new("Mononoke server for Eden")
        .version("0.1")
        .about("Http server that can answers a few Eden requests")
        .args_from_usage(
            "--addr=[ADDRESS] 'Sets a listen address in the form IP:PORT'
             --blobrepo-folder=[FOLDER] 'folder with blobrepo data'
             --reponame=[REPONAME] 'Name of the repository'
            -d, --debug              'print debug level output'
            ",
        )
        .arg(
            clap::Arg::with_name("repotype")
                .long("repotype")
                .short("T")
                .takes_value(true)
                .possible_values(&["files", "rocksdb", "manifold"])
                .required(true)
                .help("repo type"),
        )
        .get_matches();
    let addr = matches.value_of("addr").unwrap_or("127.0.0.1:3000");
    let blobrepo_folder = matches.value_of("blobrepo-folder").map(Path::new);
    let reponame = matches
        .value_of("reponame")
        .expect("Please specify a reponame")
        .to_string();

    let root_logger = {
        let level = if matches.is_present("debug") {
            Level::Debug
        } else {
            Level::Info
        };

        let drain = slog_glog_fmt::default_drain().filter_level(level).fuse();
        Logger::root(drain, o![])
    };

    match matches
        .value_of("repotype")
        .expect("required argument 'repotype' is not provided")
    {
        "files" => start_server(
            addr,
            reponame,
            FilesBlobState::new(&blobrepo_folder
                .expect("Please specify a path to the blobrepo"))
                .expect("couldn't open blob state"),
            root_logger.clone(),
        ),
        "rocksdb" => start_server(
            addr,
            reponame,
            RocksBlobState::new(&blobrepo_folder
                .expect("Please specify a path to the blobrepo"))
                .expect("couldn't open blob state"),
            root_logger.clone(),
        ),
        "manifold" => {
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
                addr,
                reponame,
                TestManifoldBlobState::new(&remote)
                    .expect("couldn't open blob state"),
                root_logger.clone(),
            )
        }
        bad => panic!("unknown blobrepo type {:?}", bad),
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
