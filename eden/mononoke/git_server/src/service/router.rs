/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use gotham::middleware::state::StateMiddleware;
use gotham::pipeline::new_pipeline;
use gotham::pipeline::single_pipeline;
use gotham::prelude::DrawRoutes;
use gotham::router::builder::build_router as gotham_build_router;
use gotham::router::builder::DefineSingleRoute;
use gotham::router::Router;
use gotham::state::State;

use crate::GitServerContext;

fn health_handler(state: State) -> (State, &'static str) {
    (state, "I_AM_ALIVE\n")
}

pub fn build_router(context: GitServerContext) -> Router {
    let pipeline = new_pipeline().add(StateMiddleware::new(context)).build();

    let (chain, pipelines) = single_pipeline(pipeline);

    gotham_build_router(chain, pipelines, |route| {
        route.get("/health_check").to(health_handler);
    })
}
