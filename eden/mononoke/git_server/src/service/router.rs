/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::pin::Pin;

use futures::FutureExt;
use futures_stats::futures03::TimedFutureExt;
use gotham::handler::HandlerFuture;
use gotham::middleware::state::StateMiddleware;
use gotham::pipeline::new_pipeline;
use gotham::pipeline::single_pipeline;
use gotham::router::builder::build_router as gotham_build_router;
use gotham::router::builder::DefineSingleRoute;
use gotham::router::builder::DrawRoutes;
use gotham::router::Router;
use gotham::state::FromState;
use gotham::state::State;
use gotham_ext::middleware::ScubaMiddlewareState;
use gotham_ext::response::build_error_response;
use hyper::HeaderMap;

use super::error_formatter::GitErrorFormatter;
use crate::model::GitServerContext;
use crate::model::RepositoryParams;
use crate::model::ServiceType;
use crate::read;
use crate::write;

fn capability_advertisement_handler(mut state: State) -> Pin<Box<HandlerFuture>> {
    async move {
        let (future_stats, res) = read::capability_advertisement(&mut state).timed().await;
        ScubaMiddlewareState::try_set_future_stats(&mut state, &future_stats);
        match res {
            Ok(res) => Ok((state, res)),
            Err(err) => {
                println!("Encountered error {:?}", err);
                build_error_response(err, state, &GitErrorFormatter)
            }
        }
    }
    .boxed()
}

fn upload_pack_handler(mut state: State) -> Pin<Box<HandlerFuture>> {
    async move {
        let (future_stats, res) = read::upload_pack(&mut state).timed().await;
        ScubaMiddlewareState::try_set_future_stats(&mut state, &future_stats);
        match res {
            Ok(res) => Ok((state, res)),
            Err(err) => {
                println!("Encountered error {:?}", err);
                build_error_response(err, state, &GitErrorFormatter)
            }
        }
    }
    .boxed()
}

fn receive_pack_handler(mut state: State) -> Pin<Box<HandlerFuture>> {
    async move {
        let (future_stats, res) = write::receive_pack(&mut state).timed().await;
        ScubaMiddlewareState::try_set_future_stats(&mut state, &future_stats);
        match res {
            Ok(res) => Ok((state, res)),
            Err(err) => {
                println!("Encountered error {:?}", err);
                build_error_response(err, state, &GitErrorFormatter)
            }
        }
    }
    .boxed()
}

fn health_handler(state: State) -> Pin<Box<HandlerFuture>> {
    async move {
        let headers = HeaderMap::borrow_from(&state);
        if let Some(wait_time) = headers
            .get("x-fb-healthcheck-wait-time-seconds")
            .and_then(|x| x.to_str().ok())
            .and_then(|t| t.parse().ok())
        {
            tokio::time::sleep(std::time::Duration::from_secs(wait_time)).await;
        }

        let res = gotham::helpers::http::response::create_response(
            &state,
            http::status::StatusCode::OK,
            mime::TEXT_PLAIN,
            "I_AM_ALIVE\n",
        );
        Ok((state, res))
    }
    .boxed()
}

pub fn build_router(context: GitServerContext) -> Router {
    let pipeline = new_pipeline().add(StateMiddleware::new(context)).build();

    let (chain, pipelines) = single_pipeline(pipeline);

    gotham_build_router(chain, pipelines, |route| {
        route
            .get("/repos/git/:server_type/*repository/info/refs")
            .with_path_extractor::<RepositoryParams>()
            .with_query_string_extractor::<ServiceType>()
            .to(capability_advertisement_handler);

        route
            .post("/repos/git/:server_type/*repository/git-upload-pack")
            .with_path_extractor::<RepositoryParams>()
            .to(upload_pack_handler);

        route
            .post("/repos/git/:server_type/*repository/git-receive-pack")
            .with_path_extractor::<RepositoryParams>()
            .to(receive_pack_handler);

        route.get("/health_check").to(health_handler);
    })
}
