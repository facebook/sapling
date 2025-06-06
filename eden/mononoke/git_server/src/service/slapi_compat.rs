/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anyhow::anyhow;
use edenapi_service::context::ServerContext;
use edenapi_service::handlers::HandlerInfo;
use edenapi_service::handlers::JsonErrorFomatter;
use edenapi_service::handlers::encode_response_stream;
use edenapi_service::handlers::handler::HandlerError;
use edenapi_service::handlers::handler::PathExtractorWithRepo;
use edenapi_service::handlers::handler::SaplingRemoteApiContext;
use edenapi_service::handlers::handler::SaplingRemoteApiHandler;
use edenapi_service::handlers::monitor_request;
use edenapi_service::middleware::request_dumper::RequestDumper;
use edenapi_service::utils::get_repo;
use edenapi_service::utils::parse_wire_request;
use edenapi_types::ToWire;
use futures_stats::futures03::TimedFutureExt;
use gotham::handler::HandlerError as GothamHandlerError;
use gotham::router::builder::DefineSingleRoute;
use gotham::router::builder::DrawRoutes;
use gotham::router::builder::RouterBuilder;
use gotham::state::FromState;
use gotham::state::State;
use gotham_ext::content_encoding::ContentEncoding;
use gotham_ext::handler::SlapiCommitIdentityScheme;
use gotham_ext::middleware::ScubaMiddlewareState;
use gotham_ext::middleware::request_context::RequestContext;
use gotham_ext::response::build_response;
use hyper::Body;
use hyper::Response;

use crate::GitServerContext;

pub(crate) struct GitHandlers<C, P> {
    _phantom: (std::marker::PhantomData<C>, std::marker::PhantomData<P>),
}

impl<C, P> GitHandlers<C, P>
where
    C: gotham::pipeline::PipelineHandleChain<P> + Copy + Send + Sync + 'static,
    P: std::panic::RefUnwindSafe + Send + Sync + 'static,
{
    pub(crate) fn setup<Handler: SaplingRemoteApiHandler>(route: &mut RouterBuilder<C, P>)
    where
        <Handler as SaplingRemoteApiHandler>::Request: std::fmt::Debug,
    {
        route
            .request(
                vec![Handler::HTTP_METHOD],
                &format!("/edenapi/*repo{}", Handler::ENDPOINT),
            )
            .with_path_extractor::<Handler::PathExtractor>()
            .with_query_string_extractor::<Handler::QueryStringExtractor>()
            .to_async(git_handler_wrapper::<Handler>);
    }
}

async fn git_handler_wrapper<Handler: SaplingRemoteApiHandler>(
    mut state: State,
) -> Result<(State, Response<Body>), (State, GothamHandlerError)>
where
    <Handler as SaplingRemoteApiHandler>::Request: std::fmt::Debug,
{
    let (future_stats, res) = async {
        let path = Handler::PathExtractor::take_from(&mut state);
        let query = Handler::QueryStringExtractor::take_from(&mut state);
        let content_encoding = ContentEncoding::from_state(&state);

        if !Handler::SUPPORTED_FLAVOURS.contains(&SlapiCommitIdentityScheme::Git) {
            return Err(gotham_ext::error::HttpError::e400(anyhow!(
                "Unsupported SaplingRemoteApi flavour"
            )));
        }

        state.put(HandlerInfo::new(path.repo(), Handler::API_METHOD));
        let rctx = RequestContext::borrow_from(&state).clone();
        let gctx = GitServerContext::borrow_from(&state).clone();

        let mononoke = gctx.repo_as_mononoke_api().map_err(HandlerError::from)?;
        let will_exit = Arc::new(AtomicBool::new(false));
        let sctx = ServerContext::new(Arc::new(mononoke), will_exit);

        let repo = get_repo(&sctx, &rctx, path.repo(), None).await?;
        let request = parse_wire_request::<<Handler::Request as ToWire>::Wire>(&mut state).await?;

        let sampling_rate = Handler::sampling_rate(&request);
        if sampling_rate.get() > 1 {
            ScubaMiddlewareState::try_set_sampling_rate(&mut state, sampling_rate);
        }

        if let Some(rd) = RequestDumper::try_borrow_mut_from(&mut state) {
            rd.add_request(&request);
        }

        let ectx = SaplingRemoteApiContext::new(
            rctx,
            sctx,
            repo,
            path,
            query,
            SlapiCommitIdentityScheme::Git,
        );

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
