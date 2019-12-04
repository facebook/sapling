/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use fbinit::FacebookInit;
use futures_ext::FutureExt;
use futures_preview::{FutureExt as Futures03Ext, TryFutureExt};
use gotham::{
    handler::HandlerFuture,
    helpers::http::response::{create_empty_response, create_response},
    middleware::state::StateMiddleware,
    pipeline::{new_pipeline, single::single_pipeline},
    router::{
        builder::{build_router as gotham_build_router, DefineSingleRoute, DrawRoutes},
        Router,
    },
    state::{FromState, State},
};
use hyper::{Body, Response, StatusCode};
use mime;

use crate::batch;
use crate::download;
use crate::lfs_server_context::LfsServerContext;
use crate::upload;

use super::middleware::ThrottleMiddleware;
use super::util::build_response;

// These 3 methods are wrappers to go from async fn's to the implementations Gotham expects,
// as well as creating HTTP responses using build_response().
fn batch_handler(mut state: State) -> Box<HandlerFuture> {
    async move {
        let res = batch::batch(&mut state).await;
        build_response(res, state)
    }
        .boxed()
        .compat()
        .boxify()
}

fn download_handler(mut state: State) -> Box<HandlerFuture> {
    async move {
        let res = download::download(&mut state).await;
        build_response(res, state)
    }
        .boxed()
        .compat()
        .boxify()
}

fn download_sha256_handler(mut state: State) -> Box<HandlerFuture> {
    async move {
        let res = download::download_sha256(&mut state).await;
        build_response(res, state)
    }
        .boxed()
        .compat()
        .boxify()
}

fn upload_handler(mut state: State) -> Box<HandlerFuture> {
    async move {
        let res = upload::upload(&mut state).await;
        build_response(res, state)
    }
        .boxed()
        .compat()
        .boxify()
}

fn health_handler(state: State) -> (State, &'static str) {
    let lfs_ctx = LfsServerContext::borrow_from(&state);
    let res = if lfs_ctx.will_exit() {
        "EXITING"
    } else {
        "I_AM_ALIVE"
    };
    (state, res)
}

fn config_handler(state: State) -> (State, Response<Body>) {
    let lfs_ctx = LfsServerContext::borrow_from(&state);

    let res = match serde_json::to_string(&lfs_ctx.get_config()) {
        Ok(json) => create_response(&state, StatusCode::OK, mime::APPLICATION_JSON, json),
        Err(_) => create_empty_response(&state, StatusCode::INTERNAL_SERVER_ERROR),
    };

    (state, res)
}

pub fn build_router(fb: FacebookInit, lfs_ctx: LfsServerContext) -> Router {
    let pipeline = new_pipeline()
        .add(ThrottleMiddleware::new(fb, lfs_ctx.get_config_handle()))
        .add(StateMiddleware::new(lfs_ctx))
        .build();

    let (chain, pipelines) = single_pipeline(pipeline);

    gotham_build_router(chain, pipelines, |route| {
        route
            .post("/:repository/objects/batch")
            .with_path_extractor::<batch::BatchParams>()
            .to(batch_handler);

        route
            .get("/:repository/download/:content_id")
            .with_path_extractor::<download::DownloadParamsContentId>()
            .to(download_handler);

        route
            .get("/:repository/download_sha256/:oid")
            .with_path_extractor::<download::DownloadParamsSha256>()
            .to(download_sha256_handler);

        route
            .put("/:repository/upload/:oid/:size")
            .with_path_extractor::<upload::UploadParams>()
            .to(upload_handler);

        route.get("/health_check").to(health_handler);
        route.get("/config").to(config_handler);
    })
}
