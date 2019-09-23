// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use futures_preview::{FutureExt, TryFutureExt};
use gotham::{
    handler::{HandlerError, HandlerFuture, IntoHandlerError},
    helpers::http::response::create_response,
    middleware::state::StateMiddleware,
    pipeline::{new_pipeline, single::single_pipeline},
    router::{
        builder::{build_router as gotham_build_router, DefineSingleRoute, DrawRoutes},
        Router,
    },
    state::{request_id, State},
};
use hyper::{Body, Response};
use itertools::Itertools;
use std::iter;

use failure_ext::chain::ChainExt;

use crate::batch;
use crate::download;
use crate::errors::ErrorKind;
use crate::http::{git_lfs_mime, HttpError, TryIntoResponse};
use crate::lfs_server_context::LfsServerContext;
use crate::middleware::RequestContext;
use crate::protocol::ResponseError;
use crate::upload;

fn build_response<IR>(
    res: Result<IR, HttpError>,
    mut state: State,
) -> Result<(State, Response<Body>), (State, HandlerError)>
where
    IR: TryIntoResponse,
{
    let res = res.and_then(|c| {
        c.try_into_response(&mut state)
            .chain_err(ErrorKind::ResponseCreationFailure)
            .map_err(HttpError::e500)
    });

    let res: Response<Body> = match res {
        Ok(resp) => resp,
        Err(error) => {
            let HttpError { error, status_code } = error;

            let error_message = iter::once(error.to_string())
                .chain(error.iter_causes().map(|c| c.to_string()))
                .join(": ");

            let res = ResponseError {
                message: error_message.clone(),
                documentation_url: None,
                request_id: Some(request_id(&state).to_string()),
            };

            if let Some(log_ctx) = state.try_borrow_mut::<RequestContext>() {
                log_ctx.set_error_msg(error_message);
            }

            // Bail if we can't convert the response to json.
            match serde_json::to_string(&res) {
                Ok(res) => create_response(&state, status_code, git_lfs_mime(), res),
                Err(error) => return Err((state, error.into_handler_error())),
            }
        }
    };

    Ok((state, res))
}

// These 3 methods are wrappers to go from async fn's to the implementations Gotham expects,
// as well as creating HTTP responses using build_response().
fn batch_handler(mut state: State) -> Box<HandlerFuture> {
    Box::new(
        (async move || {
            let res = batch::batch(&mut state).await;
            build_response(res, state)
        })()
        .boxed()
        .compat(),
    )
}

fn download_handler(mut state: State) -> Box<HandlerFuture> {
    Box::new(
        (async move || {
            let res = download::download(&mut state).await;
            build_response(res, state)
        })()
        .boxed()
        .compat(),
    )
}

fn upload_handler(mut state: State) -> Box<HandlerFuture> {
    Box::new(
        (async move || {
            let res = upload::upload(&mut state).await;
            build_response(res, state)
        })()
        .boxed()
        .compat(),
    )
}

fn health_handler(state: State) -> (State, &'static str) {
    (state, "I_AM_ALIVE")
}

pub fn build_router(lfs_ctx: LfsServerContext) -> Router {
    let pipeline = new_pipeline().add(StateMiddleware::new(lfs_ctx)).build();

    let (chain, pipelines) = single_pipeline(pipeline);

    gotham_build_router(chain, pipelines, |route| {
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

        route.get("/health_check").to(health_handler);
    })
}
