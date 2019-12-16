/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use std::sync::Arc;

use actix_web::{server, App, HttpRequest, HttpResponse, Path, Query, State};
use anyhow::Result;
use bytes::Bytes;
use clap::{value_t, Arg};
use fbinit::FacebookInit;
use futures::{future::err, Future};
use futures_ext::FutureExt;
use tokio::runtime::Runtime;

use blobrepo_factory::Caching;
use context::CoreContext;
use metaconfig_parser::RepoConfigs;
use mononoke_api::Mononoke as NewMononoke;
use panichandler::Fate;
use percent_encoding::percent_decode;
use scuba_ext::ScubaSampleBuilder;
use serde_derive::Deserialize;
use slog::{info, Logger};
use stats::schedule_stats_aggregation;

mod actor;
mod cache;
mod errors;
mod from_string;
mod middleware;
mod thrift;

use crate::actor::{Mononoke, MononokeQuery, MononokeRepoQuery, MononokeRepoResponse, Revision};
use crate::cache::CacheManager;
use crate::errors::ErrorKind;

mod config {
    pub const SCUBA_TABLE: &str = "mononoke_apiserver";
    pub const MAX_PAYLOAD_SIZE: usize = 1024 * 1024 * 1024;
}

fn get_ctx(req: &HttpRequest<HttpServerState>, state: &State<HttpServerState>) -> CoreContext {
    match req.extensions().get::<CoreContext>() {
        Some(ctx) => ctx.clone(),
        None => CoreContext::new_with_logger(state.fb, state.logger.clone()),
    }
}

#[derive(Deserialize)]
struct GetRawFileParams {
    repo: String,
    changeset: String,
    path: String,
}

// The argument of this function is because the trait `actix_web::FromRequest` is implemented
// for tuple (A, B, ...) (up to 9 elements) [1]. These arguments must implement
// `actix_web::FromRequest` as well so actix-web will try to extract them from `actix::HttpRequest`
// for us. In this case, the `State<HttpServerState>` and `Path<GetRawFileParams>`.
// [1] https://docs.rs/actix-web/0.6.11/actix_web/trait.FromRequest.html#impl-FromRequest%3CS%3E-3
fn get_raw_file(
    (state, params, req): (
        State<HttpServerState>,
        Path<GetRawFileParams>,
        HttpRequest<HttpServerState>,
    ),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    let params = params.into_inner();
    state.mononoke.send_query(
        get_ctx(&req, &state),
        MononokeQuery {
            repo: params.repo,
            kind: MononokeRepoQuery::GetRawFile {
                revision: Revision::CommitHash(params.changeset),
                path: params.path,
            },
        },
    )
}

#[derive(Deserialize)]
struct IsAncestorParams {
    repo: String,
    ancestor: String,
    descendant: String,
}

fn is_ancestor(
    (state, params, req): (
        State<HttpServerState>,
        Path<IsAncestorParams>,
        HttpRequest<HttpServerState>,
    ),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    let params = params.into_inner();
    let ancestor_parsed = percent_decode(params.ancestor.as_bytes())
        .decode_utf8_lossy()
        .to_string();
    let descendant_parsed = percent_decode(params.descendant.as_bytes())
        .decode_utf8_lossy()
        .to_string();
    state.mononoke.send_query(
        get_ctx(&req, &state),
        MononokeQuery {
            repo: params.repo,
            kind: MononokeRepoQuery::IsAncestor {
                ancestor: Revision::CommitHash(ancestor_parsed),
                descendant: Revision::CommitHash(descendant_parsed),
            },
        },
    )
}

#[derive(Deserialize)]
struct ListDirectoryParams {
    repo: String,
    changeset: String,
    path: String,
}

fn list_directory(
    (state, params, req): (
        State<HttpServerState>,
        Path<ListDirectoryParams>,
        HttpRequest<HttpServerState>,
    ),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    let params = params.into_inner();
    state.mononoke.send_query(
        get_ctx(&req, &state),
        MononokeQuery {
            repo: params.repo,
            kind: MononokeRepoQuery::ListDirectory {
                revision: Revision::CommitHash(params.changeset),
                path: params.path,
            },
        },
    )
}

#[derive(Deserialize)]
struct GetBlobParams {
    repo: String,
    hash: String,
}

fn get_blob_content(
    (state, params, req): (
        State<HttpServerState>,
        Path<GetBlobParams>,
        HttpRequest<HttpServerState>,
    ),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    let params = params.into_inner();
    state.mononoke.send_query(
        get_ctx(&req, &state),
        MononokeQuery {
            repo: params.repo,
            kind: MononokeRepoQuery::GetBlobContent { hash: params.hash },
        },
    )
}

#[derive(Deserialize)]
struct GetTreeParams {
    repo: String,
    hash: String,
}

fn get_tree(
    (state, params, req): (
        State<HttpServerState>,
        Path<GetTreeParams>,
        HttpRequest<HttpServerState>,
    ),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    let params = params.into_inner();
    state.mononoke.send_query(
        get_ctx(&req, &state),
        MononokeQuery {
            repo: params.repo,
            kind: MononokeRepoQuery::GetTree { hash: params.hash },
        },
    )
}

#[derive(Deserialize)]
struct GetChangesetParams {
    repo: String,
    hash: String,
}

fn get_changeset(
    (state, params, req): (
        State<HttpServerState>,
        Path<GetChangesetParams>,
        HttpRequest<HttpServerState>,
    ),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    let params = params.into_inner();
    state.mononoke.send_query(
        get_ctx(&req, &state),
        MononokeQuery {
            repo: params.repo,
            kind: MononokeRepoQuery::GetChangeset {
                revision: Revision::CommitHash(params.hash),
            },
        },
    )
}

#[derive(Deserialize)]
struct GetBookmarkParams {
    repo: String,
    bookmark: String,
}

fn get_bookmark(
    (state, params, req): (
        State<HttpServerState>,
        Path<GetBookmarkParams>,
        HttpRequest<HttpServerState>,
    ),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    let params = params.into_inner();
    state.mononoke.send_query(
        get_ctx(&req, &state),
        MononokeQuery {
            repo: params.repo,
            kind: MononokeRepoQuery::GetChangeset {
                revision: Revision::Bookmark(params.bookmark),
            },
        },
    )
}

#[derive(Deserialize)]
struct EdenGetDataParams {
    repo: String,
}

#[derive(Deserialize)]
struct EdenGetDataQuery {
    #[serde(default)]
    stream: bool,
}

fn eden_get_data(
    (state, params, query, body, req): (
        State<HttpServerState>,
        Path<EdenGetDataParams>,
        Query<EdenGetDataQuery>,
        Bytes,
        HttpRequest<HttpServerState>,
    ),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    let params = params.into_inner();
    let query = query.into_inner();
    match serde_cbor::from_slice(&body) {
        Ok(request) => state
            .mononoke
            .send_query(
                get_ctx(&req, &state),
                MononokeQuery {
                    repo: params.repo,
                    kind: MononokeRepoQuery::EdenGetData {
                        request,
                        stream: query.stream,
                    },
                },
            )
            .left_future(),
        Err(e) => {
            let msg = "POST data is invalid CBOR".into();
            let e = ErrorKind::InvalidInput(msg, Some(e.into()));
            err(e).right_future()
        }
    }
}

#[derive(Deserialize)]
struct EdenGetHistoryParams {
    repo: String,
}

#[derive(Deserialize)]
struct EdenGetHistoryQuery {
    #[serde(default)]
    stream: bool,
}

fn eden_get_history(
    (state, params, query, body, req): (
        State<HttpServerState>,
        Path<EdenGetHistoryParams>,
        Query<EdenGetHistoryQuery>,
        Bytes,
        HttpRequest<HttpServerState>,
    ),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    let params = params.into_inner();
    let query = query.into_inner();
    match serde_cbor::from_slice(&body) {
        Ok(request) => state
            .mononoke
            .send_query(
                get_ctx(&req, &state),
                MononokeQuery {
                    repo: params.repo,
                    kind: MononokeRepoQuery::EdenGetHistory {
                        request,
                        stream: query.stream,
                    },
                },
            )
            .left_future(),
        Err(e) => {
            let msg = "POST data is invalid CBOR".into();
            let e = ErrorKind::InvalidInput(msg, Some(e.into()));
            err(e).right_future()
        }
    }
}

#[derive(Deserialize)]
struct EdenGetTreesParams {
    repo: String,
}

#[derive(Deserialize)]
struct EdenGetTreesQuery {
    #[serde(default)]
    stream: bool,
}

fn eden_get_trees(
    (state, params, query, body, req): (
        State<HttpServerState>,
        Path<EdenGetTreesParams>,
        Query<EdenGetTreesQuery>,
        Bytes,
        HttpRequest<HttpServerState>,
    ),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    let params = params.into_inner();
    let query = query.into_inner();
    match serde_cbor::from_slice(&body) {
        Ok(request) => state
            .mononoke
            .send_query(
                get_ctx(&req, &state),
                MononokeQuery {
                    repo: params.repo,
                    kind: MononokeRepoQuery::EdenGetTrees {
                        request,
                        stream: query.stream,
                    },
                },
            )
            .left_future(),
        Err(e) => {
            let msg = "POST data is invalid CBOR".into();
            let e = ErrorKind::InvalidInput(msg, Some(e.into()));
            err(e).right_future()
        }
    }
}

#[derive(Deserialize)]
struct EdenPrefetchTreesParams {
    repo: String,
}

#[derive(Deserialize)]
struct EdenPrefetchTreesQuery {
    #[serde(default)]
    stream: bool,
}

fn eden_prefetch_trees(
    (state, params, query, body, req): (
        State<HttpServerState>,
        Path<EdenPrefetchTreesParams>,
        Query<EdenPrefetchTreesQuery>,
        Bytes,
        HttpRequest<HttpServerState>,
    ),
) -> impl Future<Item = MononokeRepoResponse, Error = ErrorKind> {
    let params = params.into_inner();
    let query = query.into_inner();
    match serde_cbor::from_slice(&body) {
        Ok(request) => state
            .mononoke
            .send_query(
                get_ctx(&req, &state),
                MononokeQuery {
                    repo: params.repo,
                    kind: MononokeRepoQuery::EdenPrefetchTrees {
                        request,
                        stream: query.stream,
                    },
                },
            )
            .left_future(),
        Err(e) => {
            let msg = "POST data is invalid CBOR".into();
            let e = ErrorKind::InvalidInput(msg, Some(e.into()));
            err(e).right_future()
        }
    }
}

#[derive(Clone)]
struct HttpServerState {
    fb: FacebookInit,
    mononoke: Arc<Mononoke>,
    new_mononoke: Arc<NewMononoke>,
    logger: Logger,
    scuba_builder: ScubaSampleBuilder,
    use_ssl: bool,
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    panichandler::set_panichandler(Fate::Abort);

    let app = clap::App::new("Mononoke API Server")
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
        .arg(Arg::with_name("without-skiplist").long("without-skiplist"))
        .arg(
            Arg::with_name("mononoke-config-path")
                .long("mononoke-config-path")
                .value_name("PATH")
                .required(true)
                .help("directory of the config repository"),
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
            Arg::with_name("ssl-ticket-seeds")
                .long("ssl-ticket-seeds")
                .value_name("PATH")
                .help("path to the ssl ticket seeds"),
        );

    let app = cmdlib::args::add_myrouter_args(app);
    let app = cmdlib::args::add_logger_args(app);
    let matches =
        cmdlib::args::add_cachelib_args(app, false /* hide_advanced_args */).get_matches();
    let with_cachelib = cmdlib::args::init_cachelib(fb, &matches);

    let host = matches.value_of("http-host").unwrap_or("127.0.0.1");
    let port = matches.value_of("http-port").unwrap_or("8000");
    let thrift_port = value_t!(matches.value_of("thrift-port"), u16);
    let config_path = matches
        .value_of("mononoke-config-path")
        .expect("must set config path");
    let with_scuba = matches.is_present("with-scuba");
    let with_skiplist = !matches.is_present("without-skiplist");
    let with_cache = matches.is_present("with-content-sha1-cache");

    let address = format!("{}:{}", host, port);

    let root_logger = cmdlib::args::init_logging(fb, &matches);
    let actix_logger = root_logger.clone();
    let mononoke_logger = root_logger.clone();
    let thrift_logger = root_logger.clone();

    let stats_aggregation =
        schedule_stats_aggregation().expect("failed to create stats aggregation scheduler");

    let mut runtime = Runtime::new().expect("tokio runtime for blocking jobs");
    let repo_configs = RepoConfigs::read_configs(fb, config_path)?;

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
        let ticket_seed = matches
            .value_of("ssl-ticket-seeds")
            .unwrap_or(secure_utils::fb_tls::SEED_PATH)
            .to_string();

        let ssl = secure_utils::SslConfig {
            cert,
            private_key,
            ca_pem,
        };
        let acceptor = secure_utils::build_tls_acceptor_builder(ssl.clone())?;
        Some(secure_utils::fb_tls::tls_acceptor_builder(
            root_logger.clone(),
            ssl.clone(),
            acceptor,
            ticket_seed,
        )?)
    } else {
        None
    };

    let mut scuba_builder = if with_scuba {
        ScubaSampleBuilder::new(fb, config::SCUBA_TABLE)
    } else {
        ScubaSampleBuilder::with_discard()
    };

    scuba_builder.add_common_server_data();

    let use_ssl = ssl_acceptor.is_some();
    let sys = actix::System::new("mononoke-apiserver");

    let cache = if with_cache && with_cachelib == Caching::Enabled {
        Some(CacheManager::new(fb)?)
    } else {
        None
    };

    let mononoke = runtime.block_on(Mononoke::new(
        fb,
        mononoke_logger.clone(),
        repo_configs,
        cmdlib::args::parse_myrouter_port(&matches),
        cmdlib::args::parse_readonly_storage(&matches),
        cache,
        with_cachelib,
        with_skiplist,
    ))?;
    let mononoke = Arc::new(mononoke);

    let new_mononoke = NewMononoke::new_from_parts(mononoke.repos.iter().map(|(name, repo)| {
        (
            name.clone(),
            repo.repo.clone(),
            repo.skiplist_index.clone(),
            repo.unodes_derived_mapping.clone(),
            repo.warm_bookmarks_cache.clone(),
            repo.synced_commit_mapping.clone(),
            repo.monitoring_config.clone(),
        )
    }));
    let new_mononoke = Arc::new(new_mononoke);

    runtime.spawn(stats_aggregation.map_err(|err| {
        eprintln!("Unexpected error: {:#?}", err);
    }));

    if let Ok(port) = thrift_port {
        thrift::make_thrift(
            fb,
            runtime.executor(),
            thrift_logger,
            host.to_string(),
            port,
            mononoke.clone(),
            scuba_builder.clone(),
        );
    }

    let state = HttpServerState {
        fb,
        mononoke,
        new_mononoke,
        logger: actix_logger.clone(),
        scuba_builder: scuba_builder.clone(),
        use_ssl,
    };

    let server = server::new(move || {
        App::with_state(state.clone())
            .middleware(middleware::CoreContextMiddleware::new(
                fb,
                actix_logger.clone(),
                scuba_builder.clone(),
            ))
            .route(
                "/health_check",
                http::Method::GET,
                |req: HttpRequest<HttpServerState>| {
                    // removing CoreContext will disable scuba logging for this request.
                    req.extensions_mut().remove::<CoreContext>();
                    HttpResponse::Ok().body("I_AM_ALIVE")
                },
            )
            .route(
                "/hostname",
                http::Method::GET,
                |_req: HttpRequest<HttpServerState>| {
                    if let Some(hostname) = hostname::get().ok().and_then(|s| s.into_string().ok())
                    {
                        HttpResponse::Ok().body(hostname)
                    } else {
                        HttpResponse::InternalServerError().body("Failed to get hostname")
                    }
                },
            )
            .scope("/{repo}", |repo| {
                repo.resource("/raw/{changeset}/{path:.*}", |r| {
                    r.method(http::Method::GET).with_async(get_raw_file)
                })
                .resource("/is_ancestor/{ancestor}/{descendant}", |r| {
                    r.method(http::Method::GET).with_async(is_ancestor)
                })
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
                .resource("/resolve_bookmark/{bookmark}", |r| {
                    r.method(http::Method::GET).with_async(get_bookmark)
                })
                .resource("/eden/data", |r| {
                    r.method(http::Method::POST)
                        .with_async_config(eden_get_data, |cfg| {
                            (cfg.0).3.limit(config::MAX_PAYLOAD_SIZE);
                        })
                })
                .resource("/eden/history", |r| {
                    r.method(http::Method::POST)
                        .with_async_config(eden_get_history, |cfg| {
                            (cfg.0).3.limit(config::MAX_PAYLOAD_SIZE);
                        })
                })
                .resource("/eden/trees", |r| {
                    r.method(http::Method::POST)
                        .with_async_config(eden_get_trees, |cfg| {
                            (cfg.0).3.limit(config::MAX_PAYLOAD_SIZE);
                        })
                })
                .resource("/eden/trees/prefetch", |r| {
                    r.method(http::Method::POST)
                        .with_async_config(eden_prefetch_trees, |cfg| {
                            (cfg.0).3.limit(config::MAX_PAYLOAD_SIZE);
                        })
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
