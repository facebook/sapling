/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::marker::PhantomData;
use std::num::NonZeroU64;
use std::panic::RefUnwindSafe;

use gotham::state::FromState;
use gotham::state::State;
use gotham_derive::StateData;
use hyper::header;
use hyper::header::AsHeaderName;
use hyper::header::HeaderMap;
use hyper::Body;
use hyper::Method;
use hyper::Response;
use hyper::StatusCode;
use hyper::Uri;
use scuba_ext::MononokeScubaSampleBuilder;
use scuba_ext::ScubaValue;
use time_ext::DurationExt;

use crate::middleware::ClientIdentity;
use crate::middleware::Middleware;
use crate::middleware::PostResponseCallbacks;
use crate::middleware::PostResponseInfo;
use crate::response::HeadersMeta;
use crate::state_ext::StateExt;

use super::HeadersDuration;
use super::RequestLoad;

/// Common HTTP-related Scuba columns that the middlware will set automatically.
/// Applications using the middleware are encouraged to follow a similar pattern
/// when adding application-specific columns to the `ScubaMiddlewareState`.
#[derive(Copy, Clone, Debug)]
pub enum HttpScubaKey {
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
    /// The client correlator submitted by the client, if any.
    ClientCorrelator,
    /// The client identities received for the client, if any.
    ClientIdentities,
    /// The request load when this request was admitted.
    RequestLoad,
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
}

impl AsRef<str> for HttpScubaKey {
    fn as_ref(&self) -> &'static str {
        use HttpScubaKey::*;

        match self {
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
            ClientCorrelator => "client_correlator",
            ClientIdentities => "client_identities",
            RequestLoad => "request_load",
            RequestId => "request_id",
            HeadersDurationMs => "headers_duration_ms",
            DurationMs => "duration_ms",
            ClientHostname => "client_hostname",
            ResponseBytesSent => "response_bytes_sent",
            RequestBytesReceived => "request_bytes_received",
        }
    }
}

impl Into<String> for HttpScubaKey {
    fn into(self) -> String {
        self.as_ref().to_string()
    }
}

pub trait ScubaHandler: Send + 'static {
    fn from_state(state: &State) -> Self;

    fn populate_scuba(self, info: &PostResponseInfo, scuba: &mut MononokeScubaSampleBuilder);
}

#[derive(Clone)]
pub struct ScubaMiddleware<H> {
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
/// `PhantomData<H>` with these traits via a wrapper struct, ensuring that
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

fn log_stats<H: ScubaHandler>(state: &mut State, status_code: &StatusCode) -> Option<()> {
    let mut scuba = state.try_take::<ScubaMiddlewareState>()?.0;

    scuba.add(HttpScubaKey::HttpStatus, status_code.as_u16());

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
            &mut scuba,
            headers,
            HttpScubaKey::HttpHost,
            header::HOST,
            |header| header.to_string(),
        );

        add_header(
            &mut scuba,
            headers,
            HttpScubaKey::RequestContentLength,
            header::CONTENT_LENGTH,
            |header| header.parse::<u64>().unwrap_or(0),
        );

        add_header(
            &mut scuba,
            headers,
            HttpScubaKey::HttpUserAgent,
            header::USER_AGENT,
            |header| header.to_string(),
        );
    }

    if let Some(identity) = ClientIdentity::try_borrow_from(state) {
        if let Some(ref address) = identity.address() {
            scuba.add(HttpScubaKey::ClientIp, address.to_string());
        }

        if let Some(ref client_correlator) = identity.client_correlator() {
            scuba.add(
                HttpScubaKey::ClientCorrelator,
                client_correlator.to_string(),
            );
        }

        if let Some(ref identities) = identity.identities() {
            scuba.sample_for_identities(identities);
            let identities: Vec<_> = identities.iter().map(|i| i.to_string()).collect();
            scuba.add(HttpScubaKey::ClientIdentities, identities);
        }
    }

    if let Some(request_load) = RequestLoad::try_borrow_from(state) {
        scuba.add(HttpScubaKey::RequestLoad, request_load.0);
    }

    scuba.add(HttpScubaKey::RequestId, state.short_request_id());

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
            // If tunables say we should, log high values unsampled
            if let Ok(threshold) = tunables::tunables()
                .get_edenapi_unsampled_duration_threshold_ms()
                .try_into()
            {
                if duration.as_millis_unchecked() > threshold {
                    scuba.unsampled();
                }
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

        handler.populate_scuba(info, &mut scuba);

        scuba.log();
    });

    Some(())
}

#[derive(StateData)]
pub struct ScubaMiddlewareState(MononokeScubaSampleBuilder);

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

    pub fn maybe_add<K, V>(scuba: &mut Option<&mut ScubaMiddlewareState>, key: K, value: V)
    where
        K: Into<String>,
        V: Into<ScubaValue>,
    {
        if let Some(ref mut scuba) = scuba {
            scuba.add(key, value);
        }
    }
}

#[async_trait::async_trait]
impl<H: ScubaHandler> Middleware for ScubaMiddleware<H> {
    async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
        state.put(ScubaMiddlewareState(self.scuba.clone()));
        None
    }

    async fn outbound(&self, state: &mut State, response: &mut Response<Body>) {
        if let Some(uri) = Uri::try_borrow_from(state) {
            if uri.path() == "/health_check" {
                return;
            }
        }

        log_stats::<H>(state, &response.status());
    }
}
