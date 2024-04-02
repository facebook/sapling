/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::pin::Pin;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use anyhow::Error;
use edenapi_types::ToWire;
use futures::channel::oneshot;
use futures::stream::TryStreamExt;
use futures::FutureExt;
use futures::Stream;
use futures_stats::futures03::TimedFutureExt;
use gotham::handler::HandlerError as GothamHandlerError;
use gotham::handler::HandlerFuture;
use gotham::middleware::state::StateMiddleware;
use gotham::pipeline::new_pipeline;
use gotham::pipeline::single_pipeline;
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
use gotham_ext::middleware::load::RequestLoad;
use gotham_ext::middleware::scuba::HttpScubaKey;
use gotham_ext::middleware::scuba::ScubaMiddlewareState;
use gotham_ext::middleware::MetadataState;
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
use time_ext::DurationExt;

use crate::context::ServerContext;
use crate::middleware::request_dumper::RequestDumper;
use crate::middleware::RequestContext;
use crate::scuba::EdenApiScubaKey;
use crate::utils::cbor_mime;
use crate::utils::get_repo;
use crate::utils::monitor::Monitor;
use crate::utils::parse_wire_request;
use crate::utils::to_cbor_bytes;

mod blame;
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
pub(crate) use handler::HandlerResult;
pub(crate) use handler::PathExtractorWithRepo;

use self::handler::EdenApiContext;

const REPORTING_LOOP_WAIT: u64 = 5;

/// Enum identifying the EdenAPI method that each handler corresponds to.
/// Used to identify the handler for logging and stats collection.
#[derive(Copy, Clone)]
pub enum EdenApiMethod {
    Blame,
    Capabilities,
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
    AlterSnapshot,
    CommitGraphV2,
    CommitGraphSegments,
    DownloadFile,
    CommitMutations,
    CommitTranslateId,
}

impl fmt::Display for EdenApiMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Blame => "blame",
            Self::Capabilities => "capabilities",
            Self::Files2 => "files2",
            Self::Trees => "trees",
            Self::History => "history",
            Self::CommitLocationToHash => "commit_location_to_hash",
            Self::CommitHashToLocation => "commit_hash_to_location",
            Self::CommitRevlogData => "commit_revlog_data",
            Self::CommitHashLookup => "commit_hash_lookup",
            Self::CommitGraphV2 => "commit_graph_v2",
            Self::CommitGraphSegments => "commit_graph_segments",
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
            Self::AlterSnapshot => "alter_snapshot",
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
                let (future_stats, res) = $func(&mut state).timed().await;
                ScubaMiddlewareState::try_set_future_stats(&mut state, &future_stats);
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

static HIGH_LOAD_SIGNAL: &str = "I_AM_OVERLOADED";
static ALIVE: &str = "I_AM_ALIVE";
static EXITING: &str = "EXITING";

// Used for monitoring VIP health
fn proxygen_health_handler(state: State) -> (State, &'static str) {
    if ServerContext::borrow_from(&state).will_exit() {
        (state, EXITING)
    } else {
        if let Some(request_load) = RequestLoad::try_borrow_from(&state) {
            let threshold =
                justknobs::get_as::<i64>("scm/mononoke:edenapi_high_load_threshold", None)
                    .unwrap_or_default();
            if threshold > 0 && request_load.0 > threshold {
                return (state, HIGH_LOAD_SIGNAL);
            }
        }
        (state, ALIVE)
    }
}

// Used for monitoring TW tasks
fn health_handler(state: State) -> (State, &'static str) {
    if ServerContext::borrow_from(&state).will_exit() {
        (state, EXITING)
    } else {
        (state, ALIVE)
    }
}

async fn handler_wrapper<Handler: EdenApiHandler>(
    mut state: State,
) -> Result<(State, Response<Body>), (State, GothamHandlerError)>
where
    <Handler as EdenApiHandler>::Request: std::fmt::Debug,
{
    let (future_stats, res) = async {
        let path = Handler::PathExtractor::take_from(&mut state);
        let query = Handler::QueryStringExtractor::take_from(&mut state);
        let content_encoding = ContentEncoding::from_state(&state);

        state.put(HandlerInfo::new(path.repo(), Handler::API_METHOD));

        let rctx = RequestContext::borrow_from(&state).clone();
        let sctx = ServerContext::borrow_from(&state).clone();

        let repo = get_repo(&sctx, &rctx, path.repo(), None).await?;
        let request = parse_wire_request::<<Handler::Request as ToWire>::Wire>(&mut state).await?;

        let sampling_rate = Handler::sampling_rate(&request);
        if sampling_rate.get() > 1 {
            ScubaMiddlewareState::try_set_sampling_rate(&mut state, sampling_rate);
        }

        if let Some(rd) = RequestDumper::try_borrow_mut_from(&mut state) {
            rd.add_request(&request);
        }

        let ectx = EdenApiContext::new(rctx, sctx, repo, path, query);

        match Handler::handler(ectx, request).await {
            Ok(responses) => Ok(encode_response_stream(
                monitor_request(&state, responses),
                content_encoding,
            )),
            Err(err) => Err(err.into()),
        }
    }
    .timed()
    .await;
    ScubaMiddlewareState::try_set_future_stats(&mut state, &future_stats);

    build_response(res, state, &JsonErrorFomatter)
}

pub fn monitor_request<S, T>(state: &State, stream: S) -> impl Stream<Item = T> + Send + 'static
where
    S: Stream<Item = T> + Send + 'static,
{
    let start = Instant::now();
    let ctx = RequestContext::borrow_from(state).ctx.clone();
    let (sender, receiver) = oneshot::channel::<()>();

    // EdenApi doesn't fill these in until the end of the request, so we need
    // to add them now.   A future improvement is to put these on the scuba
    // sample builder earlier on so we can clone it.
    let mut base_scuba = ctx.scuba().clone();
    base_scuba.add(
        HttpScubaKey::RequestId,
        state.short_request_id().to_string(),
    );

    if let Some(info) = state.try_borrow::<HandlerInfo>() {
        base_scuba.add_opt(EdenApiScubaKey::Repo, info.repo.clone());
        base_scuba.add_opt(EdenApiScubaKey::Method, info.method.map(|m| m.to_string()));
    }

    if let Some(metadata_state) = MetadataState::try_borrow_from(state) {
        let metadata = metadata_state.metadata();
        if let Some(ref address) = metadata.client_ip() {
            base_scuba.add(HttpScubaKey::ClientIp, address.to_string());
        }

        let identities = metadata.identities();
        let identities: Vec<_> = identities.iter().map(|i| i.to_string()).collect();
        base_scuba.add(HttpScubaKey::ClientIdentities, identities);
    }

    let reporting_loop = async move {
        loop {
            tokio::time::sleep(Duration::from_secs(REPORTING_LOOP_WAIT)).await;
            let mut scuba = base_scuba.clone();
            ctx.perf_counters().insert_perf_counters(&mut scuba);
            scuba.log_with_msg(
                "Long running EdenAPI request",
                format!("{}", start.elapsed().as_micros_unchecked()),
            );
        }
    };

    tokio::task::spawn(async move {
        futures::pin_mut!(reporting_loop);
        let _ = futures::future::select(reporting_loop, receiver).await;
    });

    Monitor::new(stream, sender)
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
    C: gotham::pipeline::PipelineHandleChain<P> + Copy + Send + Sync + 'static,
    P: std::panic::RefUnwindSafe + Send + Sync + 'static,
{
    fn setup<Handler: EdenApiHandler>(route: &mut RouterBuilder<C, P>)
    where
        <Handler as EdenApiHandler>::Request: std::fmt::Debug,
    {
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
        route
            .get("/proxygen/health_check")
            .to(proxygen_health_handler);
        Handlers::setup::<commit::EphemeralPrepareHandler>(route);
        Handlers::setup::<commit::UploadHgChangesetsHandler>(route);
        Handlers::setup::<commit::UploadBonsaiChangesetHandler>(route);
        Handlers::setup::<commit::LocationToHashHandler>(route);
        Handlers::setup::<commit::HashLookupHandler>(route);
        Handlers::setup::<files::Files2Handler>(route);
        Handlers::setup::<files::UploadHgFilenodesHandler>(route);
        Handlers::setup::<bookmarks::BookmarksHandler>(route);
        Handlers::setup::<bookmarks::SetBookmarkHandler>(route);
        Handlers::setup::<land::LandStackHandler>(route);
        Handlers::setup::<history::HistoryHandler>(route);
        Handlers::setup::<lookup::LookupHandler>(route);
        Handlers::setup::<trees::UploadTreesHandler>(route);
        Handlers::setup::<commit::FetchSnapshotHandler>(route);
        Handlers::setup::<commit::AlterSnapshotHandler>(route);
        Handlers::setup::<commit::GraphHandlerV2>(route);
        Handlers::setup::<commit::GraphSegmentsHandler>(route);
        Handlers::setup::<files::DownloadFileHandler>(route);
        Handlers::setup::<commit::CommitMutationsHandler>(route);
        Handlers::setup::<commit::CommitTranslateId>(route);
        Handlers::setup::<blame::BlameHandler>(route);
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
