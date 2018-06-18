// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate actix;
extern crate actix_web;
extern crate blobrepo;
extern crate clap;
extern crate failure_ext as failure;
extern crate futures;
#[macro_use]
extern crate slog;
extern crate slog_glog_fmt;
extern crate slog_logview;
extern crate slog_stats;
extern crate slog_term;
extern crate time_ext;

mod actor;
mod middleware;

use actix::{Actor, Addr, Syn};
use actix_web::{http, server, App, HttpRequest, HttpResponse};
use actix_web::error::ResponseError;
use futures::Future;
use slog::{Drain, Level, Logger};
use slog_glog_fmt::{kv_categorizer, kv_defaults, GlogFormat};
use slog_logview::LogViewDrain;

use actor::{MononokeActor, MononokeQuery};

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

    mononoke
        .send(MononokeQuery::GetBlobContent {
            repo: repo.clone(),
            hash: hash.clone(),
        })
        .map(move |result| match result {
            Ok(response) => HttpResponse::Ok().content_type("text/plain").body(response),
            Err(err) => err.compat().error_response(),
        })
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

#[derive(Clone)]
struct HttpServerState {
    mononoke: Addr<Syn, MononokeActor>,
    logger: Logger,
}

fn main() {
    let matches = clap::App::new("Mononoke API Server")
        .version("0.0.1")
        .about("An API server serves requests for Mononoke")
        .args_from_usage(
            "
            -H, --http-host [HOST]  'HTTP host to listen to'
            -p, --http-port [PORT]  'HTTP port to listen to'
            -d, --debug             'print debug level output'
            ",
        )
        .get_matches();

    let host = matches.value_of("http-host").unwrap_or("127.0.0.1");
    let port = matches.value_of("http-port").unwrap_or("8000");

    let root_logger = setup_logger(matches.is_present("debug"));
    let actix_logger = root_logger.clone();

    let sys = actix::System::new("mononoke-apiserver");

    let addr = MononokeActor.start();
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
    }).bind(format!("{}:{}", host, port))
        .unwrap();
    let address = server.addrs()[0];

    server.start();
    info!(root_logger, "Listening to http://{}", address);
    let _ = sys.run();
}
