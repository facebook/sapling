// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(try_from)]

extern crate actix;
extern crate actix_web;
extern crate blobrepo;
extern crate bookmarks;
extern crate bytes;
extern crate clap;
#[macro_use]
extern crate cloned;
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
extern crate mercurial_types;
extern crate metaconfig;
extern crate mononoke_api as api;
extern crate mononoke_types;
extern crate reachabilityindex;
extern crate scuba_ext;
extern crate secure_utils;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate slog;
extern crate slog_glog_fmt;
extern crate slog_logview;
extern crate slog_scope;
extern crate slog_stats;
extern crate slog_stdlog;
extern crate slog_term;
extern crate time_ext;

mod actor;
mod errors;
mod from_string;
mod middleware;

use std::path::Path;
use std::str::FromStr;

use actix::{Actor, Addr};
use actix_web::{http, server, App, HttpRequest, HttpResponse, State};
use blobrepo::BlobRepo;
use bookmarks::Bookmark;
use clap::Arg;
use failure::{err_msg, Result};
use futures::Future;
use slog::{Drain, Level, Logger};
use slog_glog_fmt::{kv_categorizer, kv_defaults, GlogFormat};
use slog_logview::LogViewDrain;

use mercurial_types::RepositoryId;
use mercurial_types::nodehash::HgChangesetId;
use metaconfig::RepoConfigs;
use scuba_ext::ScubaSampleBuilder;

use actor::{unwrap_request, MononokeActor, MononokeQuery, MononokeRepoQuery, MononokeRepoResponse};
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

// The argument of this function is because the trait `actix_web::FromRequest` is implemented
// for tuple (A, B, ...) (up to 9 elements) [1]. These arguments must implement
// `actix_web::FromRequest` as well so actix-web will try to extract them from `actix::HttpRequest`
// for us. In this case, the `State<HttpServerState>` and `Path<QueryInfo>`.
// [1] https://docs.rs/actix-web/0.6.11/actix_web/trait.FromRequest.html#impl-FromRequest%3CS%3E-3
fn get_raw_file(
    (state, info): (State<HttpServerState>, actix_web::Path<QueryInfo>),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    unwrap_request(state.mononoke.send(MononokeQuery {
        repo: info.repo.clone(),
        kind: MononokeRepoQuery::GetRawFile {
            changeset: info.changeset.clone(),
            path: info.path.clone(),
        },
    }))
}

fn is_ancestor(
    (state, info): (State<HttpServerState>, actix_web::Path<IsAncestorQueryInfo>),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    unwrap_request(state.mononoke.send(MononokeQuery {
        repo: info.repo.clone(),
        kind: MononokeRepoQuery::IsAncestor {
            proposed_ancestor: info.proposed_ancestor.clone(),
            proposed_descendent: info.proposed_descendent.clone(),
        },
    }))
}

fn list_directory(
    (state, info): (State<HttpServerState>, actix_web::Path<QueryInfo>),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    unwrap_request(state.mononoke.send(MononokeQuery {
        repo: info.repo.clone(),
        kind: MononokeRepoQuery::ListDirectory {
            changeset: info.changeset.clone(),
            path: info.path.clone(),
        },
    }))
}

fn get_blob_content(
    (state, info): (State<HttpServerState>, actix_web::Path<HashQueryInfo>),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    unwrap_request(state.mononoke.send(MononokeQuery {
        repo: info.repo.clone(),
        kind: MononokeRepoQuery::GetBlobContent {
            hash: info.hash.clone(),
        },
    }))
}

fn setup_logger(debug: bool) -> Logger {
    let level = if debug { Level::Debug } else { Level::Info };

    let decorator = slog_term::TermDecorator::new().build();
    let stderr_drain = GlogFormat::new(decorator, kv_categorizer::FacebookCategorizer);
    // TODO: (zeyi) T30501634 replace this with slog-async for better performance
    let stderr_drain = std::sync::Mutex::new(stderr_drain).fuse();
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
    logger: &Logger,
    path: P,
    bookmark: Option<&str>,
    hash: Option<&str>,
) -> Result<RepoConfigs> {
    let config_repo = BlobRepo::new_rocksdb(
        logger.new(o!["repo" => "Config repo"]),
        path.as_ref(),
        RepositoryId::new(0),
    )?;

    let changeset: HgChangesetId = bookmark
        .ok_or_else(|| err_msg(""))
        .and_then(|bookmark| {
            Bookmark::new(bookmark).and_then(|bookmark| config_repo.get_bookmark(&bookmark).wait())
        })
        .and_then(|bookmark| bookmark.ok_or_else(|| err_msg("bookmark not found")))
        .or_else(|_| {
            hash.ok_or_else(|| err_msg("must provide either bookmark or hash"))
                .and_then(|r| HgChangesetId::from_str(r))
        })?;

    info!(logger, "Reading config from commit: {:?}", changeset);

    RepoConfigs::read_config_repo(config_repo, changeset)
        .from_err()
        .wait()
}

#[derive(Clone)]
struct HttpServerState {
    mononoke: Addr<MononokeActor>,
    logger: Logger,
}

fn main() -> Result<()> {
    let matches = clap::App::new("Mononoke API Server")
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
        .get_matches();

    let host = matches.value_of("http-host").unwrap_or("127.0.0.1");
    let port = matches.value_of("http-port").unwrap_or("8000");

    let root_logger = setup_logger(matches.is_present("debug"));
    let actix_logger = root_logger.clone();
    let mononoke_logger = root_logger.clone();

    // These guards have to be placed in main or they would be destoried once the function ends
    let global_logger = root_logger.clone();

    let (_scope_guard, _log_guard) = if matches.is_present("stdlog") {
        (
            Some(slog_scope::set_global_logger(global_logger)),
            slog_stdlog::init().ok(),
        )
    } else {
        (None, None)
    };

    let sys = actix::System::new("mononoke-apiserver");

    let repo_configs = create_config(
        &root_logger,
        matches
            .value_of("config-path")
            .expect("must set config-path"),
        matches.value_of("config-bookmark"),
        matches.value_of("config-commit"),
    )?;

    let addr =
        MononokeActor::create(move |_| MononokeActor::new(mononoke_logger.clone(), repo_configs));
    let state = HttpServerState {
        mononoke: addr,
        logger: actix_logger.clone(),
    };

    let with_scuba = matches.is_present("with-scuba");
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
                "/status",
                http::Method::GET,
                |req: HttpRequest<HttpServerState>| {
                    // removing ScubaSampleBuilder will disable scuba logging for this request.
                    req.extensions_mut().remove::<ScubaSampleBuilder>();
                    HttpResponse::Ok().body("ok")
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
            })
    });

    let address = format!("{}:{}", host, port);

    let server = if let Some(cert) = matches.value_of("ssl-certificate") {
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
        let ssl = secure_utils::build_tls_acceptor_builder(ssl)?;

        server.bind_ssl(address, ssl)?
    } else {
        server.bind(address)?
    };

    let address = server.addrs()[0];

    server.start();

    if matches.is_present("ssl-private-key") {
        info!(root_logger, "Listening to https://{}", address);
    } else {
        info!(root_logger, "Listening to http://{}", address);
    }

    let _ = sys.run();

    Ok(())
}
