// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![feature(async_await)]
#![feature(async_closure)]
#![deny(warnings)]

use clap::Arg;
use failure::{err_msg, Error};
use futures_preview::{FutureExt, TryFutureExt};
use futures_util::{compat::Future01CompatExt, try_future::try_join_all};
use gotham::{
    handler::HandlerFuture,
    init_server,
    middleware::state::StateMiddleware,
    pipeline::{single::single_pipeline, single_middleware},
    router::{
        builder::{build_router, DefineSingleRoute, DrawRoutes},
        Router,
    },
    state::State,
};
use std::collections::HashMap;
use tokio;

use blobrepo_factory::open_blobrepo;
use metaconfig_parser::RepoConfigs;
use mononoke_types::RepositoryId;

use cmdlib::args;

use lfs_server_context::{LfsServerContext, ServerUris};

mod batch;
mod download;
mod errors;
mod lfs_server_context;
mod protocol;
mod str_serialized;
mod upload;
#[macro_use]
mod http;

// TODO: left to do here:
// - HTTPS
// - Logging
// - VIP-level routing (won't happen in this code base, though)
// - Verify that we are talking HTTP/2 to upstream
// - Make upstream optional for tests?

const ARG_SELF_URL: &str = "self-url";
const ARG_UPSTREAM_URL: &str = "upstream-url";
const ARG_LISTEN_HOST: &str = "listen-host";
const ARG_LISTEN_PORT: &str = "listen-port";

// These 3 methods are wrappers to go from async fn's to the implementations Gotham expects.
fn batch_handler(state: State) -> Box<HandlerFuture> {
    Box::new(batch::batch(state).boxed().compat())
}

fn download_handler(state: State) -> Box<HandlerFuture> {
    Box::new(download::download(state).boxed().compat())
}

fn upload_handler(state: State) -> Box<HandlerFuture> {
    Box::new(upload::upload(state).boxed().compat())
}

fn router(lfs_ctx: LfsServerContext) -> Router {
    let middleware = StateMiddleware::new(lfs_ctx);
    let pipeline = single_middleware(middleware);
    let (chain, pipelines) = single_pipeline(pipeline);

    build_router(chain, pipelines, |route| {
        route
            .post("/:repository/objects/batch")
            .with_path_extractor::<batch::BatchParams>()
            .to(batch_handler);

        route
            .get("/:repository/download/:content_id")
            .with_path_extractor::<download::DownloadParams>()
            .to(download_handler);

        route
            .put("/:repository/upload/:oid/:size")
            .with_path_extractor::<upload::UploadParams>()
            .to(upload_handler);
    })
}

fn main() -> Result<(), Error> {
    let app = args::MononokeApp {
        hide_advanced_args: true,
        default_glog: true,
    }
    .build("Mononoke LFS Server")
    .arg(
        Arg::with_name(ARG_LISTEN_HOST)
            .long("--listen-host")
            .takes_value(true)
            .default_value("127.0.0.1")
            .help("The host to listen on locally"),
    )
    .arg(
        Arg::with_name(ARG_LISTEN_PORT)
            .long("--listen-port")
            .takes_value(true)
            .default_value("8001")
            .help("The port to listen on locally"),
    )
    .arg(
        Arg::with_name(ARG_SELF_URL)
            .takes_value(true)
            .required(true)
            .help("The base URL for this server"),
    )
    .arg(
        Arg::with_name(ARG_UPSTREAM_URL)
            .takes_value(true)
            .required(true)
            .help("The base URL for an upstream server"),
    );
    let app = args::add_fb303_args(app);

    let matches = app.get_matches();

    let caching = args::init_cachelib(&matches);
    let logger = args::get_logger(&matches);
    let myrouter_port = args::parse_myrouter_port(&matches);

    let listen_host = matches.value_of(ARG_LISTEN_HOST).unwrap();
    let listen_port = matches.value_of(ARG_LISTEN_PORT).unwrap();
    let addr = format!("{}:{}", listen_host, listen_port);

    let server = ServerUris::new(
        matches.value_of(ARG_SELF_URL).unwrap(),
        matches.value_of(ARG_UPSTREAM_URL).unwrap(),
    )?;

    let RepoConfigs {
        metaconfig: _,
        repos,
        common,
    } = args::read_configs(&matches)?;

    let futs = repos.into_iter().map(|(name, config)| {
        open_blobrepo(
            config.storage_config.clone(),
            RepositoryId::new(config.repoid),
            myrouter_port,
            caching,
            config.bookmarks_cache_ttl,
            config.redaction,
            common.scuba_censored_table.clone(),
            config.filestore.clone(),
            logger.clone(),
        )
        .compat()
        .map(|repo| repo.map(|repo| (name, repo)))
    });

    let mut runtime = tokio::runtime::Runtime::new()?;

    let repos: HashMap<_, _> = runtime
        .block_on(try_join_all(futs).compat())?
        .into_iter()
        .collect();

    let root = router(LfsServerContext::new(logger, repos, server)?);

    let server = init_server(addr, root)
        .compat()
        .map_err(|_| err_msg("Server failed"));

    runtime.block_on(server.compat())?;

    Ok(())
}
