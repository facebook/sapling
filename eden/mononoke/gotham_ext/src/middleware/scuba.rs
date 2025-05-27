/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::marker::PhantomData;
use std::num::NonZeroU64;
use std::panic::RefUnwindSafe;

use futures_stats::FutureStats;
use gotham::state::FromState;
use gotham::state::State;
use gotham_derive::StateData;
use hyper::Body;
use hyper::Method;
use hyper::Response;
use hyper::StatusCode;
use hyper::Uri;
use hyper::header;
use hyper::header::AsHeaderName;
use hyper::header::HeaderMap;
use scopeguard::ScopeGuard;
use scuba_ext::MononokeScubaSampleBuilder;
use scuba_ext::ScubaValue;
use time_ext::DurationExt;

use super::HeadersDuration;
use super::request_context::RequestContext;
use crate::middleware::ConfigInfo;
use crate::middleware::MetadataState;
use crate::middleware::Middleware;
use crate::middleware::PostResponseCallbacks;
use crate::middleware::PostResponseInfo;
use crate::response::HeadersMeta;
use crate::state_ext::StateExt;

const X_FB_PRODUCT_LOG_HEADER: &str = "x-fb-product-log";
const X_FB_X2PAGENT_REQUEST_ID_HEADER: &str = "x-fb-x2pagent-request-id";
const X_FB_PRODUCT_LOG_INFO_HEADER: &str = "x-fb-product-log-info";
const X_FB_GIT_WRAPPER: &str = "x-fb-git-wrapper";
const X_FB_NETWORK_TYPE: &str = "x-fb-validated-x2pauth-advice-subject-network-type";

/// Common HTTP-related Scuba columns that the middleware will set automatically.
/// Applications using the middleware are encouraged to follow a similar pattern
/// when adding application-specific columns to the `ScubaMiddlewareState`.
#[derive(Copy, Clone, Debug)]
pub enum HttpScubaKey {
    /// The cause of the fetch. E.g. eden prefetch, eden fuse, sapling prefetch, etc.
    FetchCause,
    /// The status code for this response
    HttpStatus,
    /// The HTTP Path requested by the client.
    HttpPath,
    /// The HTTP Query string provided by the client.
    HttpQuery,
    /// The HTTP Method requested by the client.
    HttpMethod,
    /// The Http "Host" header sent by the client.
    HttpHost,
    /// The HTTP User Agent provided by the client.
    HttpUserAgent,
    /// The "Content-Length" advertised by the client in their request.
    RequestContentLength,
    /// The "Content-Length" we returned in our response.
    ResponseContentLength,
    /// The Content-Encoding we used for our response.
    ResponseContentEncoding,
    /// The IP of the connecting client.
    ClientIp,
    /// The client identities received for the client, if any.
    ClientIdentities,
    /// Alias of the sandcastle job, if any.
    SandcastleAlias,
    /// Nonce of the sandcastle job, if any.
    SandcastleNonce,
    /// VCS type of the sandcastle job, if any.
    SandcastleVCS,
    /// A unique ID identifying this request.
    RequestId,
    /// How long it took to send headers.
    HeadersDurationMs,
    /// How long it took to finish sending the response.
    DurationMs,
    /// The hostname of the connecting client.
    ClientHostname,
    /// How many bytes were sent to the client (should normally equal the content length)
    ResponseBytesSent,
    /// How many bytes were received from the client (should normally equal the content length)
    RequestBytesReceived,
    /// The config store version at the time of the request.
    ConfigStoreVersion,
    /// The config store last update time at the time of the request.
    ConfigStoreLastUpdatedAt,
    /// Request correlator ID that is recognized and standardized across all traffic infra for E2E tracebility.
    XFBProductLog,
    /// Request correlator ID that Proxygen sends and can be used as identifier for a request in
    /// traffic infra.
    XFBProductLogInfo,
    // Request id as set by the x2pagentd.
    XFBX2PAgentRequestId,
    /// Indicates whether the client was using the Meta's git wrapper or not.
    XFBGitWrapper,
    /// Which kind of network type user came from. E.g. corp or vpnless.
    XFBNetworkType,
}

impl AsRef<str> for HttpScubaKey {
    fn as_ref(&self) -> &'static str {
        use HttpScubaKey::*;

        match self {
            FetchCause => "fetch_cause",
            HttpStatus => "http_status",
            HttpPath => "http_path",
            HttpQuery => "http_query",
            HttpMethod => "http_method",
            HttpHost => "http_host",
            HttpUserAgent => "http_user_agent",
            RequestContentLength => "request_content_length",
            ResponseContentLength => "response_content_length",
            ResponseContentEncoding => "response_content_encoding",
            ClientIp => "client_ip",
            ClientIdentities => "client_identities",
            SandcastleAlias => "sandcastle_alias",
            SandcastleNonce => "sandcastle_nonce",
            SandcastleVCS => "sandcastle_vcs",
            RequestId => "request_id",
            HeadersDurationMs => "headers_duration_ms",
            DurationMs => "duration_ms",
            ClientHostname => "client_hostname",
            ResponseBytesSent => "response_bytes_sent",
            RequestBytesReceived => "request_bytes_received",
            ConfigStoreVersion => "config_store_version",
            ConfigStoreLastUpdatedAt => "config_store_last_updated_at",
            XFBProductLog => "x_fb_product_log",
            XFBProductLogInfo => "x_fb_product_log_info",
            XFBX2PAgentRequestId => "x_fb_x2pagent_request_id",
            XFBGitWrapper => "git_wrapper",
            XFBNetworkType => "fb_network_type",
        }
    }
}

impl From<HttpScubaKey> for String {
    fn from(k: HttpScubaKey) -> String {
        k.as_ref().to_string()
    }
}

pub trait ScubaHandler: Send + 'static {
    /// Construct an instance of this scuba handler from the Gotham `State`.
    fn from_state(state: &State) -> Self;

    /// Log to scuba that this request was processed.
    fn log_processed(self, info: &PostResponseInfo, scuba: MononokeScubaSampleBuilder);

    /// Log to scuba that this request was cancelled.
    fn log_cancelled(scuba: MononokeScubaSampleBuilder) {
        let _ = scuba;
    }
}

#[derive(Clone)]
pub struct ScubaMiddleware<H> {
    /// Fallback scuba sample builder to use if the request context is not
    /// available.
    scuba: MononokeScubaSampleBuilder,
    _phantom: PhantomHandler<H>,
}

impl<H> ScubaMiddleware<H> {
    pub fn new(scuba: MononokeScubaSampleBuilder) -> Self {
        Self {
            scuba,
            _phantom: PhantomHandler(PhantomData),
        }
    }
}

/// Phantom type that ensures that `ScubaMiddleware` can be `RefUnwindSafe` and
/// `Sync` without imposing those constraints on its type parameter.
///
/// Since `ScubaMiddleware` is generic over its handler type, in order for it
/// to automatically implement `Sync` and `RefUnwindSafe` (which are required
/// by the `Middleware` trait), the handler would ordinarily need to also
/// be subject to those constraints.
///
/// This isn't actually necessary since the middleware itself does not contain
/// an instance of the handler. (The handler is instantiated shortly before it
/// is used in a post-request callback.) Therefore, it is safe to manually mark
/// `PhantomHandler<H>` with these traits via a wrapper struct, ensuring that
/// the middleware automatically implements the required marker traits.
#[derive(Clone)]
struct PhantomHandler<H>(PhantomData<H>);

impl<H> RefUnwindSafe for PhantomHandler<H> {}

unsafe impl<H> Sync for PhantomHandler<H> {}

fn add_header<'a, Header, Converter, Value>(
    scuba: &mut MononokeScubaSampleBuilder,
    headers: &'a HeaderMap,
    scuba_key: HttpScubaKey,
    header: Header,
    convert: Converter,
) -> Option<&'a str>
where
    Header: AsHeaderName,
    Converter: FnOnce(&str) -> Value,
    Value: Into<ScubaValue>,
{
    if let Some(header_val) = headers.get(header) {
        if let Ok(header_val) = header_val.to_str() {
            scuba
                .entry(scuba_key)
                .or_insert_with(|| convert(header_val).into());
            return Some(header_val);
        }
    }

    None
}

fn populate_scuba(scuba: &mut MononokeScubaSampleBuilder, state: &mut State) {
    if let Some(uri) = Uri::try_borrow_from(state) {
        scuba.add(HttpScubaKey::HttpPath, uri.path());
        if let Some(query) = uri.query() {
            scuba.add(HttpScubaKey::HttpQuery, query);
        }
    }

    if let Some(method) = Method::try_borrow_from(state) {
        scuba.add(HttpScubaKey::HttpMethod, method.to_string());
    }

    if let Some(headers) = HeaderMap::try_borrow_from(state) {
        add_header(
            scuba,
            headers,
            HttpScubaKey::HttpHost,
            header::HOST,
            |header| header.to_string(),
        );

        add_header(
            scuba,
            headers,
            HttpScubaKey::RequestContentLength,
            header::CONTENT_LENGTH,
            |header| header.parse::<u64>().unwrap_or(0),
        );

        add_header(
            scuba,
            headers,
            HttpScubaKey::HttpUserAgent,
            header::USER_AGENT,
            |header| header.to_string(),
        );

        add_header(
            scuba,
            headers,
            HttpScubaKey::XFBProductLog,
            X_FB_PRODUCT_LOG_HEADER,
            |header| header.to_string(),
        );

        add_header(
            scuba,
            headers,
            HttpScubaKey::XFBProductLogInfo,
            X_FB_PRODUCT_LOG_INFO_HEADER,
            |header| header.to_string(),
        );

        add_header(
            scuba,
            headers,
            HttpScubaKey::XFBX2PAgentRequestId,
            X_FB_X2PAGENT_REQUEST_ID_HEADER,
            |header| header.to_string(),
        );

        add_header(
            scuba,
            headers,
            HttpScubaKey::XFBGitWrapper,
            X_FB_GIT_WRAPPER,
            |header| header.to_string(),
        );
        add_header(
            scuba,
            headers,
            HttpScubaKey::XFBNetworkType,
            X_FB_NETWORK_TYPE,
            |header| header.to_string(),
        );
    }

    if let Some(metadata_state) = MetadataState::try_borrow_from(state) {
        let metadata = metadata_state.metadata();
        if let Some(ref address) = metadata.client_ip() {
            scuba.add(HttpScubaKey::ClientIp, address.to_string());
        }
        if let Some(client_info) = metadata.client_request_info() {
            scuba.add_client_request_info(client_info);
        }
        let identities = metadata.identities();
        scuba.sample_for_identities(identities);
        let identities: Vec<_> = identities.iter().map(|i| i.to_string()).collect();
        scuba.add(HttpScubaKey::ClientIdentities, identities);

        let sandcastle_alias = metadata.sandcastle_alias();
        scuba.add(HttpScubaKey::SandcastleAlias, sandcastle_alias);

        let sandcastle_nonce = metadata.sandcastle_nonce();
        scuba.add(HttpScubaKey::SandcastleNonce, sandcastle_nonce);

        let sandcastle_vcs = metadata.sandcastle_vcs();
        scuba.add(HttpScubaKey::SandcastleVCS, sandcastle_vcs);

        let fetch_cause = metadata.fetch_cause();
        scuba.add(HttpScubaKey::FetchCause, fetch_cause);
    }

    if let Some(config_version) = ConfigInfo::try_borrow_from(state) {
        scuba.add(
            HttpScubaKey::ConfigStoreVersion,
            config_version.version.clone(),
        );
        scuba.add(
            HttpScubaKey::ConfigStoreLastUpdatedAt,
            config_version.last_updated_at.clone(),
        );
    }

    scuba.add(HttpScubaKey::RequestId, state.short_request_id());
}

fn log_stats<H: ScubaHandler>(
    mut scuba: MononokeScubaSampleBuilder,
    state: &mut State,
    status_code: &StatusCode,
) -> Option<()> {
    scuba.add(HttpScubaKey::HttpStatus, status_code.as_u16());

    if let Some(HeadersDuration(duration)) = HeadersDuration::try_borrow_from(state) {
        scuba.add(
            HttpScubaKey::HeadersDurationMs,
            duration.as_millis_unchecked(),
        );
    }

    let handler = H::from_state(state);

    let callbacks = state.try_borrow_mut::<PostResponseCallbacks>()?;
    callbacks.add(move |info| {
        if let Some(duration) = info.duration {
            let threshold: u64 = justknobs::get_as::<u64>(
                "scm/mononoke_timeouts:edenapi_unsampled_duration_threshold_ms",
                None,
            )
            .unwrap_or_default();

            if duration.as_millis_unchecked() > threshold {
                scuba.unsampled();
            }

            scuba.add(HttpScubaKey::DurationMs, duration.as_millis_unchecked());
        }

        if let Some(client_hostname) = info.client_hostname.as_deref() {
            scuba.add(HttpScubaKey::ClientHostname, client_hostname);
        }

        if let Some(meta) = info.meta.as_ref() {
            match *meta.headers() {
                HeadersMeta::Sized(content_length) => {
                    scuba.add(HttpScubaKey::ResponseContentLength, content_length);
                }
                HeadersMeta::Compressed(compression) => {
                    scuba.add(HttpScubaKey::ResponseContentEncoding, compression.as_str());
                }
                HeadersMeta::Chunked => {}
            }

            scuba.add(HttpScubaKey::ResponseBytesSent, meta.body().bytes_sent);
        }

        if let Some(stats) = info.stream_stats.as_ref() {
            scuba.add_prefixed_stream_stats(stats);
        }

        handler.log_processed(info, scuba);
    });

    Some(())
}

#[derive(StateData)]
pub struct ScubaMiddlewareState(
    ScopeGuard<MononokeScubaSampleBuilder, Box<dyn FnOnce(MononokeScubaSampleBuilder) + Send>>,
);

impl ScubaMiddlewareState {
    pub fn add<K, V>(&mut self, key: K, value: V) -> &mut Self
    where
        K: Into<String>,
        V: Into<ScubaValue>,
    {
        self.0.add(key, value);
        self
    }

    /// Borrow the ScubaMiddlewareState, if any, and add a key-value pair to it.
    pub fn try_borrow_add<K, V>(state: &mut State, key: K, value: V)
    where
        K: Into<String>,
        V: Into<ScubaValue>,
    {
        let mut scuba = state.try_borrow_mut::<Self>();
        if let Some(ref mut scuba) = scuba {
            scuba.add(key, value);
        }
    }

    pub fn try_set_sampling_rate(state: &mut State, rate: NonZeroU64) {
        let mut scuba = state.try_borrow_mut::<Self>();
        if let Some(ref mut scuba) = scuba {
            scuba.0.sampled_unless_verbose(rate);
        }
    }

    pub fn try_set_future_stats(state: &mut State, future_stats: &FutureStats) {
        let mut scuba = state.try_borrow_mut::<Self>();
        if let Some(ref mut scuba) = scuba {
            scuba.0.add_future_stats(future_stats);
        }
    }

    pub fn maybe_add<K, V>(scuba: &mut Option<&mut ScubaMiddlewareState>, key: K, value: V)
    where
        K: Into<String>,
        V: Into<ScubaValue>,
    {
        if let Some(scuba) = scuba {
            scuba.add(key, value);
        }
    }
}

#[async_trait::async_trait]
impl<H: ScubaHandler> Middleware for ScubaMiddleware<H> {
    async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
        // Get the scuba sample builder from the ctx in the request context.
        let mut scuba = if let Some(req) = state.try_borrow::<RequestContext>() {
            req.ctx.scuba().clone()
        } else {
            // Use the fallback scuba sample builder instead, resetting the
            // scuba sequence counter for each request.
            self.scuba.clone().with_seq("seq")
        };

        // Populate the sample builder with values available at the start of the request.
        populate_scuba(&mut scuba, state);

        // Update the request context with the populated scuba sample builder.
        if let Some(req) = state.try_borrow_mut::<RequestContext>() {
            req.ctx = req.ctx.with_mutated_scuba(|_| scuba.clone());
        }

        // Ensure we log if the request is cancelled.
        let scuba = scopeguard::guard(
            scuba,
            Box::new(|scuba| {
                H::log_cancelled(scuba);
            }) as Box<dyn FnOnce(MononokeScubaSampleBuilder) + Send>,
        );

        state.put(ScubaMiddlewareState(scuba));
        None
    }

    async fn outbound(&self, state: &mut State, response: &mut Response<Body>) {
        if let Some(scuba_middleware) = state.try_take::<ScubaMiddlewareState>() {
            // Defuse the scopeguard so that we will no longer log cancellation.
            let mut scuba = ScopeGuard::into_inner(scuba_middleware.0).clone();
            let status = &response.status();

            if let Some(uri) = Uri::try_borrow_from(state) {
                if uri.path() == "/health_check" || uri.path() == "/proxygen/health_check" {
                    if !justknobs::eval("scm/mononoke:health_check_scuba_log_enabled", None, None)
                        .unwrap_or(false)
                    {
                        return;
                    }

                    let sampling_rate = core::num::NonZeroU64::new(
                        if status.as_u16() >= 200 || status.as_u16() < 299 {
                            const FALLBACK_SAMPLING_RATE: u64 = 1000;
                            justknobs::get_as::<u64>(
                                "scm/mononoke:health_check_scuba_log_success_sampling_rate",
                                None,
                            )
                            .unwrap_or(FALLBACK_SAMPLING_RATE)
                        } else {
                            const FALLBACK_SAMPLING_RATE: u64 = 1;
                            justknobs::get_as::<u64>(
                                "scm/mononoke:health_check_scuba_log_failure_sampling_rate",
                                None,
                            )
                            .unwrap_or(FALLBACK_SAMPLING_RATE)
                        },
                    );
                    if let Some(sampling_rate) = sampling_rate {
                        scuba.sampled(sampling_rate);
                    } else {
                        scuba.unsampled();
                    }
                }
            }

            log_stats::<H>(scuba, state, status);
        }
    }
}
