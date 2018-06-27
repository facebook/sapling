// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate actix;
extern crate actix_web;
extern crate blobrepo;
extern crate bookmarks;
extern crate bytes;
extern crate clap;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
extern crate mercurial_types;
extern crate metaconfig;
#[macro_use]
extern crate slog;
extern crate slog_glog_fmt;
extern crate slog_logview;
extern crate slog_stats;
extern crate slog_term;
extern crate time_ext;

mod actor;
mod errors;
mod middleware;

use std::path::Path;
use std::str::FromStr;

use actix::{Actor, Addr, Syn};
use actix_web::{http, server, App, Body, HttpRequest, HttpResponse};
use actix_web::error::ResponseError;
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

use actor::{unwrap_request, MononokeActor, MononokeQuery, MononokeRepoQuery, MononokeRepoResponse};
use errors::ErrorKind;

mod parameters {
    pub const REPO: &str = "repo";
    pub const HASH: &str = "hash";
}

fn get_blob_content(
    req: HttpRequest<HttpServerState>,
) -> impl Future<Item = HttpResponse, Error = impl ResponseError> {
    let matches = req.match_info();
    let repo = matches
        .get(parameters::REPO)
        .expect("repo is required")
        .to_string();
    let hash = matches
        .get(parameters::HASH)
        .expect("hash is required")
        .to_string();

    let state = req.state();

    let mononoke = &state.mononoke;

    let request = mononoke.send(MononokeQuery {
        repo: repo,
        kind: MononokeRepoQuery::GetBlobContent { hash: hash },
    });

    unwrap_request(request)
        .map(|response| match response {
            MononokeRepoResponse::GetBlobContent { content } => HttpResponse::Ok()
                .content_type("application/octet-stream")
                .body(Body::Binary(content.into())),
        })
        .map_err(|e| -> ErrorKind { e.into() })
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
    mononoke: Addr<Syn, MononokeActor>,
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
        .arg(Arg::with_name("debug").short("p").long("debug"))
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
        .get_matches();

    let host = matches.value_of("http-host").unwrap_or("127.0.0.1");
    let port = matches.value_of("http-port").unwrap_or("8000");

    let root_logger = setup_logger(matches.is_present("debug"));
    let actix_logger = root_logger.clone();
    let mononoke_logger = root_logger.clone();

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

    let server = server::new(move || {
        App::with_state(state.clone())
            .middleware(middleware::SLogger::new(actix_logger.clone()))
            .scope("/{repo}", |repo| {
                repo.resource("/blob/{hash}", |r| {
                    r.method(http::Method::GET).a(get_blob_content)
                })
            })
    }).bind(format!("{}:{}", host, port))?;
    let address = server.addrs()[0];

    server.start();
    info!(root_logger, "Listening to http://{}", address);
    let _ = sys.run();

    Ok(())
}
