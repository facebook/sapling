/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::pin::Pin;

use anyhow::{Context, Error};
use futures::FutureExt;
use gotham::{
    handler::HandlerFuture,
    middleware::state::StateMiddleware,
    pipeline::{new_pipeline, single::single_pipeline},
    router::{
        builder::{build_router as gotham_build_router, DefineSingleRoute, DrawRoutes},
        Router,
    },
    state::{request_id, FromState, State},
};
use gotham_derive::StateData;
use mime::Mime;
use serde::{Deserialize, Serialize};

use gotham_ext::{error::ErrorFormatter, response::build_response};

use crate::context::ServerContext;

mod bookmarks;
mod clone;
mod commit;
mod complete_trees;
mod files;
mod history;
mod lookup;
mod pull;
mod repos;
mod trees;

/// Enum identifying the EdenAPI method that each handler corresponds to.
/// Used to identify the handler for logging and stats collection.
#[derive(Copy, Clone)]
pub enum EdenApiMethod {
    Files,
    Lookup,
    UploadFile,
    UploadHgFilenodes,
    UploadTrees,
    UploadHgChangesets,
    Trees,
    CompleteTrees,
    History,
    CommitLocationToHash,
    CommitHashToLocation,
    CommitRevlogData,
    CommitHashLookup,
    Clone,
    FullIdMapClone,
    Bookmarks,
    PullFastForwardMaster,
    EphemeralPrepare,
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
            Self::CommitHashLookup => "commit_hash_lookup",
            Self::Clone => "clone",
            Self::FullIdMapClone => "full_idmap_clone",
            Self::Bookmarks => "bookmarks",
            Self::Lookup => "lookup",
            Self::UploadFile => "upload_file",
            Self::PullFastForwardMaster => "pull_fast_forward_master",
            Self::UploadHgFilenodes => "upload_filenodes",
            Self::UploadTrees => "upload_trees",
            Self::UploadHgChangesets => "upload_hg_changesets",
            Self::EphemeralPrepare => "ephemeral_prepare",
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

/// JSON representation of an error to send to the client.
#[derive(Clone, Serialize, Debug, Deserialize)]
struct JsonError {
    message: String,
    request_id: String,
}

struct JsonErrorFomatter;

impl ErrorFormatter for JsonErrorFomatter {
    type Body = Vec<u8>;

    fn format(&self, error: &Error, state: &State) -> Result<(Self::Body, Mime), Error> {
        let message = format!("{:#}", error);

        // Package the error message into a JSON response.
        let res = JsonError {
            message,
            request_id: request_id(&state).to_string(),
        };

        let body = serde_json::to_vec(&res).context("Failed to serialize error")?;

        Ok((body, mime::APPLICATION_JSON))
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
                build_response(res, state, &JsonErrorFomatter)
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
define_handler!(commit_hash_lookup_handler, commit::hash_lookup);
define_handler!(clone_handler, clone::clone_data);
define_handler!(full_idmap_clone_handler, clone::full_idmap_clone_data);
define_handler!(bookmarks_handler, bookmarks::bookmarks);
define_handler!(lookup_handler, lookup::lookup);
define_handler!(upload_file_handler, files::upload_file);
define_handler!(pull_fast_forward_master, pull::pull_fast_forward_master);
define_handler!(upload_hg_filenodes_handler, files::upload_hg_filenodes);
define_handler!(upload_trees_handler, trees::upload_trees);
define_handler!(upload_hg_changesets_handler, commit::upload_hg_changesets);
define_handler!(ephemeral_prepare_handler, commit::ephemeral_prepare);

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
            .post("/:repo/commit/hash_lookup")
            .with_path_extractor::<commit::HashLookupParams>()
            .to(commit_hash_lookup_handler);
        route
            .post("/:repo/clone")
            .with_path_extractor::<clone::CloneParams>()
            .to(clone_handler);
        route
            .post("/:repo/pull_fast_forward_master")
            .with_path_extractor::<pull::PullFastForwardParams>()
            .to(pull_fast_forward_master);
        route
            .post("/:repo/full_idmap_clone")
            .with_path_extractor::<clone::CloneParams>()
            .to(full_idmap_clone_handler);
        route
            .post("/:repo/bookmarks")
            .with_path_extractor::<bookmarks::BookmarksParams>()
            .to(bookmarks_handler);
        route
            .post("/:repo/lookup")
            .with_path_extractor::<lookup::LookupParams>()
            .to(lookup_handler);
        route
            .put("/:repo/upload/file/:idtype/:id")
            .with_path_extractor::<files::UploadFileParams>()
            .to(upload_file_handler);
        route
            .post("/:repo/upload/filenodes")
            .with_path_extractor::<files::UploadHgFilenodesParams>()
            .to(upload_hg_filenodes_handler);
        route
            .post("/:repo/upload/trees")
            .with_path_extractor::<trees::UploadTreesParams>()
            .to(upload_trees_handler);
        route
            .post("/:repo/upload/changesets")
            .with_path_extractor::<commit::UploadHgChangesetsParams>()
            .to(upload_hg_changesets_handler);
        route
            .post("/:repo/ephemeral/prepare")
            .with_path_extractor::<commit::EphemeralPrepareParams>()
            .to(ephemeral_prepare_handler);
    })
}
