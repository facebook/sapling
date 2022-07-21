/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::pin::Pin;

use anyhow::Context;
use anyhow::Error;
use edenapi_types::ToWire;
use futures::stream::TryStreamExt;
use futures::FutureExt;
use futures::Stream;
use gotham::handler::HandlerError as GothamHandlerError;
use gotham::handler::HandlerFuture;
use gotham::middleware::state::StateMiddleware;
use gotham::pipeline::new_pipeline;
use gotham::pipeline::single::single_pipeline;
use gotham::router::builder::build_router as gotham_build_router;
use gotham::router::builder::DefineSingleRoute;
use gotham::router::builder::DrawRoutes;
use gotham::router::builder::RouterBuilder;
use gotham::router::Router;
use gotham::state::FromState;
use gotham::state::State;
use gotham_derive::StateData;
use gotham_ext::content_encoding::ContentEncoding;
use gotham_ext::error::ErrorFormatter;
use gotham_ext::error::HttpError;
use gotham_ext::middleware::scuba::ScubaMiddlewareState;
use gotham_ext::response::build_response;
use gotham_ext::response::encode_stream;
use gotham_ext::response::ResponseTryStreamExt;
use gotham_ext::response::StreamBody;
use gotham_ext::response::TryIntoResponse;
use gotham_ext::state_ext::StateExt;
use hyper::Body;
use hyper::Response;
use mime::Mime;
use serde::Deserialize;
use serde::Serialize;

use crate::context::ServerContext;
use crate::middleware::RequestContext;
use crate::utils::cbor_mime;
use crate::utils::get_repo;
use crate::utils::parse_wire_request;
use crate::utils::to_cbor_bytes;

mod bookmarks;
mod capabilities;
mod clone;
mod commit;
mod files;
mod handler;
mod history;
mod land;
mod lookup;
mod pull;
mod repos;
mod trees;

pub(crate) use handler::EdenApiHandler;
pub(crate) use handler::HandlerError;
pub(crate) use handler::HandlerResult;
pub(crate) use handler::PathExtractorWithRepo;

/// Enum identifying the EdenAPI method that each handler corresponds to.
/// Used to identify the handler for logging and stats collection.
#[derive(Copy, Clone)]
pub enum EdenApiMethod {
    Capabilities,
    Files,
    Files2,
    Lookup,
    UploadFile,
    UploadHgFilenodes,
    UploadTrees,
    UploadHgChangesets,
    UploadBonsaiChangeset,
    Trees,
    History,
    CommitLocationToHash,
    CommitHashToLocation,
    CommitRevlogData,
    CommitHashLookup,
    Clone,
    Bookmarks,
    SetBookmark,
    LandStack,
    PullFastForwardMaster,
    PullLazy,
    EphemeralPrepare,
    FetchSnapshot,
    CommitGraph,
    DownloadFile,
    CommitMutations,
    CommitTranslateId,
}

impl fmt::Display for EdenApiMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Capabilities => "capabilities",
            Self::Files => "files",
            Self::Files2 => "files2",
            Self::Trees => "trees",
            Self::History => "history",
            Self::CommitLocationToHash => "commit_location_to_hash",
            Self::CommitHashToLocation => "commit_hash_to_location",
            Self::CommitRevlogData => "commit_revlog_data",
            Self::CommitHashLookup => "commit_hash_lookup",
            Self::CommitGraph => "commit_graph",
            Self::Clone => "clone",
            Self::Bookmarks => "bookmarks",
            Self::SetBookmark => "set_bookmark",
            Self::LandStack => "land_stack",
            Self::Lookup => "lookup",
            Self::UploadFile => "upload_file",
            Self::PullFastForwardMaster => "pull_fast_forward_master",
            Self::PullLazy => "pull_lazy",
            Self::UploadHgFilenodes => "upload_filenodes",
            Self::UploadTrees => "upload_trees",
            Self::UploadHgChangesets => "upload_hg_changesets",
            Self::UploadBonsaiChangeset => "upload_bonsai_changeset",
            Self::EphemeralPrepare => "ephemeral_prepare",
            Self::FetchSnapshot => "fetch_snapshot",
            Self::DownloadFile => "download_file",
            Self::CommitMutations => "commit_mutations",
            Self::CommitTranslateId => "commit_translate_id",
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
            request_id: state.short_request_id().to_string(),
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
define_handler!(trees_handler, trees::trees);
define_handler!(capabilities_handler, capabilities::capabilities_handler);
define_handler!(commit_hash_to_location_handler, commit::hash_to_location);
define_handler!(commit_revlog_data_handler, commit::revlog_data);
define_handler!(clone_handler, clone::clone_data);
define_handler!(upload_file_handler, files::upload_file);
define_handler!(pull_fast_forward_master, pull::pull_fast_forward_master);
define_handler!(pull_lazy, pull::pull_lazy);

fn health_handler(state: State) -> (State, &'static str) {
    if ServerContext::borrow_from(&state).will_exit() {
        (state, "EXITING")
    } else {
        (state, "I_AM_ALIVE")
    }
}

async fn handler_wrapper<Handler: EdenApiHandler>(
    mut state: State,
) -> Result<(State, Response<Body>), (State, GothamHandlerError)> {
    let res = async {
        let path = Handler::PathExtractor::take_from(&mut state);
        let query_string = Handler::QueryStringExtractor::take_from(&mut state);
        let content_encoding = ContentEncoding::from_state(&state);

        state.put(HandlerInfo::new(path.repo(), Handler::API_METHOD));

        let rctx = RequestContext::borrow_from(&mut state).clone();
        let sctx = ServerContext::borrow_from(&mut state);

        let repo = get_repo(sctx, &rctx, path.repo(), None).await?;
        let request = parse_wire_request::<<Handler::Request as ToWire>::Wire>(&mut state).await?;

        let sampling_rate = Handler::sampling_rate(&request);
        if sampling_rate.get() > 1 {
            ScubaMiddlewareState::try_set_sampling_rate(&mut state, sampling_rate);
        }

        match Handler::handler(repo, path, query_string, request).await {
            Ok(responses) => Ok(encode_response_stream(responses, content_encoding)),
            Err(HandlerError::E500(err)) => Err(HttpError::e500(err)),
        }
    }
    .await;

    build_response(res, state, &JsonErrorFomatter)
}

/// Encode a stream of EdenAPI responses into its final on-wire representation.
///
/// This involves converting each item to its wire format, CBOR serializing them, and then
/// optionally compressing the resulting byte stream based on the specified Content-Encoding.
pub fn encode_response_stream<S, T>(stream: S, encoding: ContentEncoding) -> impl TryIntoResponse
where
    S: Stream<Item = Result<T, Error>> + Send + 'static,
    T: ToWire + Send + 'static,
{
    let stream = stream.and_then(|item| async move { to_cbor_bytes(&item.to_wire()) });
    let stream = encode_stream(stream, encoding, None).capture_first_err();
    StreamBody::new(stream, cbor_mime())
}

// We use a struct here (rather than just a global function) just for the convenience
// of writing `Handlers::setup::<MyHandler>(route)`
// instead of `setup_handler::<MyHandler, _, _>(route)`, to make things clearer.
struct Handlers<C, P> {
    _phantom: (std::marker::PhantomData<C>, std::marker::PhantomData<P>),
}

impl<C, P> Handlers<C, P>
where
    C: gotham::pipeline::chain::PipelineHandleChain<P> + Copy + Send + Sync + 'static,
    P: std::panic::RefUnwindSafe + Send + Sync + 'static,
{
    fn setup<Handler: EdenApiHandler>(route: &mut RouterBuilder<C, P>) {
        route
            .request(
                vec![Handler::HTTP_METHOD],
                &format!("/:repo{}", Handler::ENDPOINT),
            )
            .with_path_extractor::<Handler::PathExtractor>()
            .with_query_string_extractor::<Handler::QueryStringExtractor>()
            .to_async(handler_wrapper::<Handler>);
    }
}

pub fn build_router(ctx: ServerContext) -> Router {
    let pipeline = new_pipeline().add(StateMiddleware::new(ctx)).build();
    let (chain, pipelines) = single_pipeline(pipeline);

    gotham_build_router(chain, pipelines, |route| {
        route.get("/health_check").to(health_handler);
        route.get("/repos").to(repos_handler);
        Handlers::setup::<commit::EphemeralPrepareHandler>(route);
        Handlers::setup::<commit::UploadHgChangesetsHandler>(route);
        Handlers::setup::<commit::UploadBonsaiChangesetHandler>(route);
        Handlers::setup::<commit::LocationToHashHandler>(route);
        Handlers::setup::<commit::HashLookupHandler>(route);
        Handlers::setup::<files::FilesHandler>(route);
        Handlers::setup::<files::Files2Handler>(route);
        Handlers::setup::<files::UploadHgFilenodesHandler>(route);
        Handlers::setup::<bookmarks::BookmarksHandler>(route);
        Handlers::setup::<bookmarks::SetBookmarkHandler>(route);
        Handlers::setup::<land::LandStackHandler>(route);
        Handlers::setup::<history::HistoryHandler>(route);
        Handlers::setup::<lookup::LookupHandler>(route);
        Handlers::setup::<trees::UploadTreesHandler>(route);
        Handlers::setup::<commit::FetchSnapshotHandler>(route);
        Handlers::setup::<commit::GraphHandler>(route);
        Handlers::setup::<files::DownloadFileHandler>(route);
        Handlers::setup::<commit::CommitMutationsHandler>(route);
        Handlers::setup::<commit::CommitTranslateId>(route);
        route.get("/:repo/health_check").to(health_handler);
        route
            .get("/:repo/capabilities")
            .with_path_extractor::<capabilities::CapabilitiesParams>()
            .to(capabilities_handler);
        route
            .post("/:repo/trees")
            .with_path_extractor::<trees::TreeParams>()
            .to(trees_handler);
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
            .post("/:repo/pull_fast_forward_master")
            .with_path_extractor::<pull::PullFastForwardParams>()
            .to(pull_fast_forward_master);
        route
            .post("/:repo/pull_lazy")
            .with_path_extractor::<pull::PullLazyParams>()
            .to(pull_lazy);
        route
            .put("/:repo/upload/file/:idtype/:id")
            .with_path_extractor::<files::UploadFileParams>()
            .with_query_string_extractor::<files::UploadFileQueryString>()
            .to(upload_file_handler);
    })
}
