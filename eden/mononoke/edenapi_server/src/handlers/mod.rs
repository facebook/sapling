/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use gotham::{
    middleware::state::StateMiddleware,
    pipeline::{new_pipeline, single::single_pipeline},
    router::{
        builder::{build_router as gotham_build_router, DefineSingleRoute, DrawRoutes},
        Router,
    },
};

use crate::context::EdenApiServerContext;

mod health;

use self::health::health_handler;

pub fn build_router(ctx: EdenApiServerContext) -> Router {
    let pipeline = new_pipeline().add(StateMiddleware::new(ctx)).build();
    let (chain, pipelines) = single_pipeline(pipeline);

    gotham_build_router(chain, pipelines, |route| {
        route.get("/health_check").to(health_handler);
    })
}
