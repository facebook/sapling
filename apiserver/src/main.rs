// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(try_from)]

extern crate actix;
extern crate actix_web;
extern crate apiserver_thrift;
extern crate ascii;
extern crate blobrepo;
extern crate bookmarks;
extern crate bytes;
extern crate cachelib;
extern crate chrono;
#[macro_use]
extern crate clap;
#[macro_use]
extern crate cloned;
extern crate cmdlib;
extern crate context;
extern crate env_logger;
#[macro_use]
extern crate failure_ext as failure;
extern crate fb303;
extern crate futures;
#[macro_use]
extern crate futures_ext;
extern crate http;
extern crate mercurial_types;
extern crate metaconfig;
extern crate mononoke_api as api;
extern crate mononoke_types;
extern crate panichandler;
extern crate percent_encoding;
extern crate reachabilityindex;
extern crate rust_thrift;
extern crate scuba_ext;
extern crate secure_utils;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate slog;
extern crate slog_async;
extern crate slog_glog_fmt;
extern crate slog_logview;
extern crate slog_scope;
extern crate slog_stats;
extern crate slog_stdlog;
extern crate slog_term;
extern crate sql;
extern crate srserver;
extern crate time_ext;
extern crate tokio;
extern crate tokio_threadpool;

mod actor;
mod errors;
mod from_string;
mod middleware;
mod thrift;

use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

use bytes::Bytes;

use actix_web::{server, App, HttpRequest, HttpResponse, Json, State, http::header};
use blobrepo::BlobRepo;
use bookmarks::Bookmark;
use clap::Arg;
use context::CoreContext;
use failure::{err_msg, Result};
use futures::Future;
use http::uri::{Authority, Parts, PathAndQuery, Scheme, Uri};
use panichandler::Fate;
use percent_encoding::percent_decode;
use slog::{Drain, Level, Logger};
use slog_glog_fmt::{kv_categorizer, kv_defaults, GlogFormat};
use slog_logview::LogViewDrain;
use tokio::runtime::Runtime;

use mercurial_types::RepositoryId;
use mercurial_types::nodehash::HgChangesetId;
use metaconfig::RepoConfigs;
use scuba_ext::ScubaSampleBuilder;

use actor::{BatchRequest, Mononoke, MononokeQuery, MononokeRepoQuery, MononokeRepoResponse};
use errors::ErrorKind;

mod config {
    pub const SCUBA_TABLE: &str = "mononoke_apiserver";
}

#[derive(Deserialize)]
struct QueryInfo {
    repo: String,
    changeset: String,
    path: String,
}

#[derive(Deserialize)]
struct IsAncestorQueryInfo {
    repo: String,
    proposed_ancestor: String,
    proposed_descendent: String,
}

#[derive(Deserialize)]
struct HashQueryInfo {
    repo: String,
    hash: String,
}

#[derive(Deserialize)]
struct OidQueryInfo {
    repo: String,
    oid: String,
}

#[derive(Deserialize)]
struct LfsBatchInfo {
    repo: String,
}

// The argument of this function is because the trait `actix_web::FromRequest` is implemented
// for tuple (A, B, ...) (up to 9 elements) [1]. These arguments must implement
// `actix_web::FromRequest` as well so actix-web will try to extract them from `actix::HttpRequest`
// for us. In this case, the `State<HttpServerState>` and `Path<QueryInfo>`.
// [1] https://docs.rs/actix-web/0.6.11/actix_web/trait.FromRequest.html#impl-FromRequest%3CS%3E-3
fn get_raw_file(
    (state, info): (State<HttpServerState>, actix_web::Path<QueryInfo>),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    state.mononoke.send_query(MononokeQuery {
        repo: info.repo.clone(),
        kind: MononokeRepoQuery::GetRawFile {
            changeset: info.changeset.clone(),
            path: info.path.clone(),
        },
    })
}

fn is_ancestor(
    (state, info): (State<HttpServerState>, actix_web::Path<IsAncestorQueryInfo>),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    let proposed_ancestor_parsed = percent_decode(info.proposed_ancestor.as_bytes())
        .decode_utf8_lossy()
        .to_string();
    let proposed_descendent_parsed = percent_decode(info.proposed_descendent.as_bytes())
        .decode_utf8_lossy()
        .to_string();
    state.mononoke.send_query(MononokeQuery {
        repo: info.repo.clone(),
        kind: MononokeRepoQuery::IsAncestor {
            proposed_ancestor: proposed_ancestor_parsed,
            proposed_descendent: proposed_descendent_parsed,
        },
    })
}

fn list_directory(
    (state, info): (State<HttpServerState>, actix_web::Path<QueryInfo>),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    state.mononoke.send_query(MononokeQuery {
        repo: info.repo.clone(),
        kind: MononokeRepoQuery::ListDirectory {
            changeset: info.changeset.clone(),
            path: info.path.clone(),
        },
    })
}

fn get_blob_content(
    (state, info): (State<HttpServerState>, actix_web::Path<HashQueryInfo>),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    state.mononoke.send_query(MononokeQuery {
        repo: info.repo.clone(),
        kind: MononokeRepoQuery::GetBlobContent {
            hash: info.hash.clone(),
        },
    })
}

fn get_tree(
    (state, info): (State<HttpServerState>, actix_web::Path<HashQueryInfo>),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    state.mononoke.send_query(MononokeQuery {
        repo: info.repo.clone(),
        kind: MononokeRepoQuery::GetTree {
            hash: info.hash.clone(),
        },
    })
}

fn get_changeset(
    (state, info): (State<HttpServerState>, actix_web::Path<HashQueryInfo>),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    state.mononoke.send_query(MononokeQuery {
        repo: info.repo.clone(),
        kind: MononokeRepoQuery::GetChangeset {
            hash: info.hash.clone(),
        },
    })
}

fn download_large_file(
    (state, info): (State<HttpServerState>, actix_web::Path<OidQueryInfo>),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    state.mononoke.send_query(MononokeQuery {
        repo: info.repo.clone(),
        kind: MononokeRepoQuery::DownloadLargeFile {
            oid: info.oid.clone(),
        },
    })
}

fn lfs_batch(
    (state, req_json, info, req): (
        State<HttpServerState>,
        Json<BatchRequest>,
        actix_web::Path<LfsBatchInfo>,
        HttpRequest<HttpServerState>,
    ),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    let host_url = req.headers().get(header::HOST);
    let scheme = if state.use_ssl {
        Scheme::HTTPS
    } else {
        Scheme::HTTP
    };
    let lfs_url = host_url
        .and_then(|url_header_value| url_header_value.to_str().ok())
        .and_then(|url| Authority::from_shared(Bytes::from(url)).ok())
        .and_then(|url| {
            let path_and_query = PathAndQuery::from_shared(Bytes::from("")).ok();

            let mut parts = Parts::default();
            parts.scheme = Some(scheme);
            parts.authority = Some(url);
            parts.path_and_query = path_and_query;

            Uri::from_parts(parts).ok()
        });

    state.mononoke.send_query(MononokeQuery {
        repo: info.repo.clone(),
        kind: MononokeRepoQuery::LfsBatch {
            req: req_json.into_inner(),
            repo_name: info.repo.clone(),
            lfs_url,
        },
    })
}

// TODO(anastasiyaz): T32937714 Bytes -> Streaming
fn upload_large_file(
    (state, body, info): (State<HttpServerState>, Bytes, actix_web::Path<OidQueryInfo>),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    state.mononoke.send_query(MononokeQuery {
        repo: info.repo.clone(),
        kind: MononokeRepoQuery::UploadLargeFile {
            oid: info.oid.clone(),
            body: body,
        },
    })
}

fn setup_logger(debug: bool) -> Logger {
    let level = if debug { Level::Debug } else { Level::Info };

    let decorator = slog_term::TermDecorator::new().build();
    let stderr_drain = GlogFormat::new(decorator, kv_categorizer::FacebookCategorizer).fuse();
    let stderr_drain = slog_async::Async::new(stderr_drain).build().fuse();
    let logview_drain = LogViewDrain::new("errorlog_mononoke_apiserver");

    let drain = slog::Duplicate::new(stderr_drain, logview_drain);
    let drain = slog_stats::StatsDrain::new(drain);
    let drain = drain.filter_level(level);

    Logger::root(
        drain.fuse(),
        o!(kv_defaults::FacebookKV::new().expect("Failed to initialize logging")),
    )
}

fn create_config<P: AsRef<Path>>(
    runtime: &mut tokio::runtime::Runtime,
    logger: &Logger,
    path: P,
    bookmark: Option<&str>,
    hash: Option<&str>,
) -> Result<RepoConfigs> {
    // TODO(T37478150, luk) This is not a test case, will be fixed in later diffs
    let ctx = CoreContext::test_mock();
    let config_repo = BlobRepo::new_rocksdb(
        logger.new(o!["repo" => "Config repo"]),
        path.as_ref(),
        RepositoryId::new(0),
    )?;

    let changeset: HgChangesetId = bookmark
        .ok_or_else(|| err_msg(""))
        .and_then(|bookmark| {
            let bookmark = Bookmark::new(bookmark)?;
            runtime.block_on(config_repo.get_bookmark(ctx, &bookmark))
        })
        .and_then(|bookmark| bookmark.ok_or_else(|| err_msg("bookmark not found")))
        .or_else(|e| {
            hash.ok_or_else(|| format_err!("must provide either bookmark or hash ({})", e))
                .and_then(|r| HgChangesetId::from_str(r))
        })?;

    info!(logger, "Reading config from commit: {:?}", changeset);

    RepoConfigs::read_config_repo(config_repo, changeset)
        .from_err()
        .wait()
}

#[derive(Clone)]
struct HttpServerState {
    mononoke: Arc<Mononoke>,
    logger: Logger,
    use_ssl: bool,
}

fn main() -> Result<()> {
    env_logger::init();
    panichandler::set_panichandler(Fate::Abort);

    let matches = cmdlib::args::add_cachelib_args(
        clap::App::new("Mononoke API Server")
            .version("0.0.1")
            .about("An API server serves requests for Mononoke")
            .arg(
                Arg::with_name("http-host")
                    .short("H")
                    .long("http-host")
                    .value_name("HOST")
                    .default_value("127.0.0.1")
                    .help("HTTP host to listen to"),
            )
            .arg(
                Arg::with_name("http-port")
                    .short("p")
                    .long("http-port")
                    .value_name("PORT")
                    .default_value("8000")
                    .help("HTTP port to listen to"),
            )
            .arg(
                Arg::with_name("thrift-port")
                    .long("thrift-port")
                    .value_name("PORT")
                    .help("Thrift port"),
            )
            .arg(Arg::with_name("with-scuba").long("with-scuba"))
            .arg(Arg::with_name("debug").short("p").long("debug"))
            .arg(
                Arg::with_name("stdlog")
                    .long("stdlog")
                    .help("print logs from third-party crates"),
            )
            .arg(
                Arg::with_name("config-path")
                    .long("config-path")
                    .value_name("PATH")
                    .required(true)
                    .help("directory of the config repository"),
            )
            .arg(
                Arg::with_name("config-bookmark")
                    .long("config-bookmark")
                    .value_name("BOOKMARK")
                    .required_unless("config-commit")
                    .help("bookmark of the config repository"),
            )
            .arg(
                Arg::with_name("config-commit")
                    .long("config-commit")
                    .value_name("HASH")
                    .required_unless("config-bookmark")
                    .help("commit hash of the config repository"),
            )
            .arg(
                Arg::with_name("ssl-certificate")
                    .long("ssl-certificate")
                    .value_name("PATH")
                    .help("path to the ssl certificate file"),
            )
            .arg(
                Arg::with_name("ssl-private-key")
                    .long("ssl-private-key")
                    .value_name("PATH")
                    .help("path to the ssl private key file")
                    .requires("ssl-ca"),
            )
            .arg(
                Arg::with_name("ssl-ca")
                    .long("ssl-ca")
                    .value_name("PATH")
                    .help("path to the ssl ca file"),
            )
            .arg(
                Arg::with_name("myrouter-port")
                    .long("myrouter-port")
                    .value_name("PORT")
                    .help("port for local myrouter instance"),
            ),
        false, /* hide_advanced_args */
    ).get_matches();
    cmdlib::args::init_cachelib(&matches);

    let host = matches.value_of("http-host").unwrap_or("127.0.0.1");
    let port = matches.value_of("http-port").unwrap_or("8000");
    let thrift_port = value_t!(matches.value_of("thrift-port"), i32);
    let debug = matches.is_present("debug");
    let stdlog = matches.is_present("stdlog");
    let config_path = matches
        .value_of("config-path")
        .expect("must set config path");
    let with_scuba = matches.is_present("with-scuba");

    let myrouter_port = match matches.value_of("myrouter-port") {
        Some(port) => Some(
            port.parse::<u16>()
                .expect("Provided --myrouter-port is not u16"),
        ),
        None => None,
    };

    let address = format!("{}:{}", host, port);

    let root_logger = setup_logger(debug);
    let actix_logger = root_logger.clone();
    let mononoke_logger = root_logger.clone();
    let thrift_logger = root_logger.clone();

    // These guards have to be placed in main or they would be destoried once the function ends
    let global_logger = root_logger.clone();

    let (_scope_guard, _log_guard) = if stdlog {
        (
            Some(slog_scope::set_global_logger(global_logger)),
            slog_stdlog::init().ok(),
        )
    } else {
        (None, None)
    };

    let mut runtime = Runtime::new().expect("tokio runtime for blocking jobs");
    let repo_configs = create_config(
        &mut runtime,
        &root_logger,
        config_path,
        matches.value_of("config-bookmark"),
        matches.value_of("config-commit"),
    )?;

    let ssl_acceptor = if let Some(cert) = matches.value_of("ssl-certificate") {
        let cert = cert.to_string();
        let private_key = matches
            .value_of("ssl-private-key")
            .expect("must specify ssl private key")
            .to_string();
        let ca_pem = matches
            .value_of("ssl-ca")
            .expect("must specify CA")
            .to_string();

        let ssl = secure_utils::SslConfig {
            cert,
            private_key,
            ca_pem,
        };
        Some(secure_utils::build_tls_acceptor_builder(ssl)?)
    } else {
        None
    };
    let use_ssl = ssl_acceptor.is_some();
    let sys = actix::System::new("mononoke-apiserver");
    let executor = runtime.executor();
    let mononoke = runtime.block_on(Mononoke::new(
        mononoke_logger.clone(),
        repo_configs,
        myrouter_port,
        executor,
    ))?;
    let mononoke = Arc::new(mononoke);

    if let Ok(port) = thrift_port {
        thrift::make_thrift(thrift_logger, host.to_string(), port, mononoke.clone());
    }

    let state = HttpServerState {
        mononoke,
        logger: actix_logger.clone(),
        use_ssl,
    };

    let server = server::new(move || {
        App::with_state(state.clone())
            .middleware(middleware::SLogger::new(actix_logger.clone()))
            .middleware({
                if with_scuba {
                    middleware::ScubaMiddleware::new(
                        Some(config::SCUBA_TABLE.into()),
                        actix_logger.clone(),
                    )
                } else {
                    middleware::ScubaMiddleware::new(None, actix_logger.clone())
                }
            })
            .route(
                "/health_check",
                http::Method::GET,
                |req: HttpRequest<HttpServerState>| {
                    // removing ScubaSampleBuilder will disable scuba logging for this request.
                    req.extensions_mut().remove::<ScubaSampleBuilder>();
                    HttpResponse::Ok().body("I_AM_ALIVE")
                },
            )
            .scope("/{repo}", |repo| {
                repo.resource("/raw/{changeset}/{path:.*}", |r| {
                    r.method(http::Method::GET).with_async(get_raw_file)
                }).resource(
                        "/is_ancestor/{proposed_ancestor}/{proposed_descendent}",
                        |r| r.method(http::Method::GET).with_async(is_ancestor),
                    )
                    .resource("/list/{changeset}/{path:.*}", |r| {
                        r.method(http::Method::GET).with_async(list_directory)
                    })
                    .resource("/blob/{hash}", |r| {
                        r.method(http::Method::GET).with_async(get_blob_content)
                    })
                    .resource("/tree/{hash}", |r| {
                        r.method(http::Method::GET).with_async(get_tree)
                    })
                    .resource("/changeset/{hash}", |r| {
                        r.method(http::Method::GET).with_async(get_changeset)
                    })
                    .resource("/lfs/download/{oid}", |r| {
                        r.method(http::Method::GET).with_async(download_large_file)
                    })
                    .resource("/objects/batch", |r| {
                        r.method(http::Method::POST).with_async(lfs_batch)
                    })
                    .resource("/lfs/upload/{oid}", |r| {
                        r.method(http::Method::PUT).with_async(upload_large_file)
                    })
            })
    });

    let server = if let Some(acceptor) = ssl_acceptor {
        server.bind_ssl(address, acceptor)?
    } else {
        server.bind(address)?
    };

    let address = server.addrs()[0];

    server.start();

    if use_ssl {
        info!(root_logger, "Listening to https://{}", address);
    } else {
        info!(root_logger, "Listening to http://{}", address);
    }

    let _ = sys.run();

    Ok(())
}
