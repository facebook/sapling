/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
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
use gotham_derive::StateData;

use gotham_ext::response::build_response;

use crate::context::ServerContext;
use crate::middleware::RequestContext;

mod clone;
mod commit;
mod complete_trees;
mod files;
mod history;
mod repos;
mod trees;

/// Enum identifying the EdenAPI method that each handler corresponds to.
/// Used to identify the handler for logging and stats collection.
#[derive(Copy, Clone)]
pub enum EdenApiMethod {
    Files,
    Trees,
    CompleteTrees,
    History,
    CommitLocationToHash,
    CommitHashToLocation,
    CommitRevlogData,
    Clone,
    FullIdMapClone,
}

impl fmt::Display for EdenApiMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Files => "files",
            Self::Trees => "trees",
            Self::CompleteTrees => "complete_trees",
            Self::History => "history",
            Self::CommitLocationToHash => "commit_location_to_hash",
            Self::CommitHashToLocation => "commit_hash_to_location",
            Self::CommitRevlogData => "commit_revlog_data",
            Self::Clone => "clone",
            Self::FullIdMapClone => "full_idmap_clone",
        };
        write!(f, "{}", name)
    }
}

/// Information about the handler that served the request.
///
/// This should be inserted into the request's `State` by each handler. It will
/// typically be used by middlware for request logging and stats reporting.
#[derive(Default, StateData, Clone)]
pub struct HandlerInfo {
    pub repo: Option<String>,
    pub method: Option<EdenApiMethod>,
}

impl HandlerInfo {
    pub fn new(repo: impl ToString, method: EdenApiMethod) -> Self {
        Self {
            repo: Some(repo.to_string()),
            method: Some(method),
        }
    }
}

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

                if let Err(e) = res.as_ref() {
                    if let Some(log_ctx) = state.try_borrow_mut::<RequestContext>() {
                        log_ctx.handler_error_msg = Some(e.message());
                    }
                }

                build_response(res, state)
            }
            .boxed()
        }
    };
}

define_handler!(repos_handler, repos::repos);
define_handler!(files_handler, files::files);
define_handler!(trees_handler, trees::trees);
define_handler!(complete_trees_handler, complete_trees::complete_trees);
define_handler!(history_handler, history::history);
define_handler!(commit_location_to_hash_handler, commit::location_to_hash);
define_handler!(commit_hash_to_location_handler, commit::hash_to_location);
define_handler!(commit_revlog_data_handler, commit::revlog_data);
define_handler!(clone_handler, clone::clone_data);
define_handler!(full_idmap_clone_handler, clone::full_idmap_clone_data);

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
        route.get("/repos").to(repos_handler);
        route
            .post("/:repo/files")
            .with_path_extractor::<files::FileParams>()
            .to(files_handler);
        route
            .post("/:repo/trees")
            .with_path_extractor::<trees::TreeParams>()
            .to(trees_handler);
        route
            .post("/:repo/trees/complete")
            .with_path_extractor::<complete_trees::CompleteTreesParams>()
            .to(complete_trees_handler);
        route
            .post("/:repo/history")
            .with_path_extractor::<history::HistoryParams>()
            .to(history_handler);
        route
            .post("/:repo/commit/location_to_hash")
            .with_path_extractor::<commit::LocationToHashParams>()
            .to(commit_location_to_hash_handler);
        route
            .post("/:repo/commit/hash_to_location")
            .with_path_extractor::<commit::HashToLocationParams>()
            .to(commit_hash_to_location_handler);
        route
            .post("/:repo/commit/revlog_data")
            .with_path_extractor::<commit::RevlogDataParams>()
            .to(commit_revlog_data_handler);
        route
            .post("/:repo/clone")
            .with_path_extractor::<clone::CloneParams>()
            .to(clone_handler);
        route
            .post("/:repo/full_idmap_clone")
            .with_path_extractor::<clone::CloneParams>()
            .to(full_idmap_clone_handler);
    })
}
