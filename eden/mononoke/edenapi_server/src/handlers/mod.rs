/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::pin::Pin;

use futures::FutureExt;
use gotham::{
    handler::HandlerFuture,
    middleware::state::StateMiddleware,
    pipeline::{new_pipeline, single::single_pipeline},
    router::{
        builder::{build_router as gotham_build_router, DefineSingleRoute, DrawRoutes},
        Router,
    },
    state::{FromState, State},
};

use gotham_ext::response::build_response;
use mercurial_types::{HgFileNodeId, HgManifestId};

use crate::context::ServerContext;

mod data;
mod history;
mod repos;
mod subtree;

/// Macro to create a Gotham handler function from an async function.
///
/// The expected signature of the input function is:
/// ```rust,ignore
/// async fn handler(state: &mut State) -> Result<impl TryIntoResponse, HttpError>
/// ```
///
/// The resulting wrapped function will have the signaure:
/// ```rust,ignore
/// fn wrapped(mut state: State) -> Pin<Box<HandlerFuture>>
/// ```
macro_rules! define_handler {
    ($name:ident, $func:path) => {
        fn $name(mut state: State) -> Pin<Box<HandlerFuture>> {
            async move {
                let res = $func(&mut state).await;
                build_response(res, state)
            }
            .boxed()
        }
    };
}

define_handler!(repos_handler, repos::repos);
define_handler!(files_handler, data::data::<HgFileNodeId>);
define_handler!(trees_handler, data::data::<HgManifestId>);
define_handler!(history_handler, history::history);
define_handler!(subtree_handler, subtree::subtree);

fn health_handler(state: State) -> (State, &'static str) {
    if ServerContext::borrow_from(&state).will_exit() {
        (state, "EXITING")
    } else {
        (state, "I_AM_ALIVE")
    }
}

pub fn build_router(ctx: ServerContext) -> Router {
    let pipeline = new_pipeline().add(StateMiddleware::new(ctx)).build();
    let (chain, pipelines) = single_pipeline(pipeline);

    gotham_build_router(chain, pipelines, |route| {
        route.get("/health_check").to(health_handler);
        route
            .get("/repos")
            .with_query_string_extractor::<repos::ReposParams>()
            .to(repos_handler);
        route
            .post("/:repo/files")
            .with_path_extractor::<data::DataParams>()
            .to(files_handler);
        route
            .post("/:repo/trees")
            .with_path_extractor::<data::DataParams>()
            .to(trees_handler);
        route
            .post("/:repo/history")
            .with_path_extractor::<history::HistoryParams>()
            .to(history_handler);
        route
            .post("/:repo/subtree")
            .with_path_extractor::<subtree::SubTreeParams>()
            .to(subtree_handler);
    })
}
