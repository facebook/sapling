/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use failure::Error;
use gotham::state::{request_id, FromState, State};
use gotham_derive::StateData;
use hyper::{
    body::Payload,
    header::{self, AsHeaderName, HeaderMap},
    Method, StatusCode, Uri,
};
use hyper::{Body, Response};
use scuba::{ScubaSampleBuilder, ScubaValue};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::result::Result;
use std::sync::{Arc, Mutex};
use time_ext::DurationExt;

use super::{ClientIdentity, Middleware, RequestContext, RequestLoad as RequestLoadMiddleware};

#[derive(Copy, Clone, Debug)]
pub enum ScubaKey {
    /// The status code for this response
    HttpStatus,
    /// The HTTP Path requested by the client.
    HttpPath,
    /// The HTTP Method requested by the client.
    HttpMethod,
    /// The Http "Host" header sent by the client.
    HttpHost,
    /// The "Content-Length" advertised by the client in their request.
    RequestContentLength,
    /// The "Content-Length" we returned in our response.
    ResponseContentLength,
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
    /// The repository this request was for.
    Repository,
    /// The method this request matched for in our handlers.
    Method,
    /// If an error was encountered during processing, the error message.
    ErrorMessage,
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
    /// The order in which the response to a batch request was produced.
    BatchOrder,
    /// The number of objects in a batch request
    BatchObjectCount,
    /// The objects that could not be serviced by this LFS server in a batch request
    BatchInternalMissingBlobs,
}

impl AsRef<str> for ScubaKey {
    fn as_ref(&self) -> &'static str {
        use ScubaKey::*;

        match self {
            HttpStatus => "http_status",
            HttpPath => "http_path",
            HttpMethod => "http_method",
            HttpHost => "http_host",
            RequestContentLength => "request_content_length",
            ResponseContentLength => "response_content_length",
            ClientIp => "client_ip",
            ClientCorrelator => "client_correlator",
            ClientIdentities => "client_identities",
            RequestLoad => "request_load",
            RequestId => "request_id",
            Repository => "repository",
            Method => "method",
            ErrorMessage => "error_msg",
            HeadersDurationMs => "headers_duration_ms",
            DurationMs => "duration_ms",
            ClientHostname => "client_hostname",
            ResponseBytesSent => "response_bytes_sent",
            RequestBytesReceived => "request_bytes_received",
            BatchOrder => "batch_order",
            BatchObjectCount => "batch_object_count",
            BatchInternalMissingBlobs => "batch_internal_missing_blobs",
        }
    }
}

impl Into<String> for ScubaKey {
    fn into(self) -> String {
        self.as_ref().to_string()
    }
}

#[derive(Clone)]
pub struct ScubaMiddleware {
    scuba: ScubaSampleBuilder,
    log_file: Option<Arc<Mutex<File>>>,
}

impl ScubaMiddleware {
    pub fn new<L: AsRef<Path>>(
        scuba: ScubaSampleBuilder,
        log_file: Option<L>,
    ) -> Result<Self, Error> {
        let log_file: Result<_, Error> = log_file
            .map(|log_file| {
                let log_file = File::create(log_file)?;
                Ok(Arc::new(Mutex::new(log_file)))
            })
            .transpose();

        Ok(Self {
            scuba,
            log_file: log_file?,
        })
    }
}

fn add_header<'a, Header, Converter, Value>(
    scuba: &mut ScubaSampleBuilder,
    headers: &'a HeaderMap,
    scuba_key: ScubaKey,
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
            scuba.entry(scuba_key).or_insert(convert(header_val).into());
            return Some(header_val);
        }
    }

    None
}

fn log_stats(
    log_file: Option<Arc<Mutex<File>>>,
    state: &mut State,
    status_code: &StatusCode,
    content_length: Option<u64>,
) -> Option<()> {
    let mut scuba = state.try_take::<ScubaMiddlewareState>()?.0;

    scuba.add(ScubaKey::HttpStatus, status_code.as_u16());

    if let Some(uri) = Uri::try_borrow_from(&state) {
        scuba.add(ScubaKey::HttpPath, uri.path());
    }

    if let Some(method) = Method::try_borrow_from(&state) {
        scuba.add(ScubaKey::HttpMethod, method.to_string());
    }

    if let Some(headers) = HeaderMap::try_borrow_from(&state) {
        add_header(
            &mut scuba,
            headers,
            ScubaKey::HttpHost,
            header::HOST,
            |header| header.to_string(),
        );

        add_header(
            &mut scuba,
            headers,
            ScubaKey::RequestContentLength,
            header::CONTENT_LENGTH,
            |header| header.parse::<u64>().unwrap_or(0),
        );
    }

    // Set the response size to the content length, unless it was overridden earlier. This is
    // helpful to ensure all our responses get a content length if Hyper can provide one, and only
    // those responses where Hyper is unable to derive the content length need to provide it for
    // themselves.
    if let Some(content_length) = content_length {
        scuba
            .entry(ScubaKey::ResponseContentLength)
            .or_insert(content_length.into());
    }

    if let Some(identity) = ClientIdentity::try_borrow_from(&state) {
        if let Some(ref address) = identity.address() {
            scuba.add(ScubaKey::ClientIp, address.to_string());
        }

        if let Some(ref client_correlator) = identity.client_correlator() {
            scuba.add(ScubaKey::ClientCorrelator, client_correlator.to_string());
        }

        if let Some(ref identities) = identity.identities() {
            let identities: Vec<_> = identities.into_iter().map(|i| i.to_string()).collect();
            scuba.add(ScubaKey::ClientIdentities, identities);
        }
    }

    if let Some(request_load) = RequestLoadMiddleware::try_borrow_from(&state) {
        scuba.add(ScubaKey::RequestLoad, request_load.0);
    }

    scuba.add(ScubaKey::RequestId, request_id(&state));

    let ctx = state.try_borrow_mut::<RequestContext>()?;

    if let Some(repository) = &ctx.repository {
        scuba.add(ScubaKey::Repository, repository.as_ref());
    }

    if let Some(method) = ctx.method {
        scuba.add(ScubaKey::Method, method.to_string());
    }

    if let Some(err_msg) = &ctx.error_msg {
        scuba.add(ScubaKey::ErrorMessage, err_msg.as_ref());
    }

    if let Some(headers_duration) = ctx.headers_duration {
        scuba.add(
            ScubaKey::HeadersDurationMs,
            headers_duration.as_millis_unchecked(),
        );
    }

    ctx.add_post_request(move |duration, client_hostname, bytes_sent| {
        scuba.add(ScubaKey::DurationMs, duration.as_millis_unchecked());

        if let Some(client_hostname) = client_hostname {
            scuba.add(ScubaKey::ClientHostname, client_hostname.to_string());
        }
        if let Some(bytes_sent) = bytes_sent {
            scuba.add(ScubaKey::ResponseBytesSent, bytes_sent);
        }

        scuba.log();

        // Write to a log file here. If this fails, we don't take further action (this is only used
        // in tests, so it's largely fine).
        if let Some(log_file) = log_file {
            let mut log_file = log_file.lock().expect("Poisoned lock");
            let _ = scuba.to_json().map_err(|_| ()).and_then(|sample| {
                log_file
                    .write_all(sample.to_string().as_bytes())
                    .map_err(|_| ())
            });
        }
    });

    Some(())
}

#[derive(StateData)]
pub struct ScubaMiddlewareState(ScubaSampleBuilder);

impl ScubaMiddlewareState {
    pub fn add<V: Into<ScubaValue>>(&mut self, key: ScubaKey, value: V) -> &mut Self {
        self.0.add(key, value);
        self
    }

    /// Borrow the ScubaMiddlewareState, if any, and add a key-value pair to it.
    pub fn try_borrow_add<V: Into<ScubaValue>>(state: &mut State, key: ScubaKey, value: V) {
        let mut scuba = state.try_borrow_mut::<Self>();
        if let Some(ref mut scuba) = scuba {
            scuba.add(key, value);
        }
    }
}

impl Middleware for ScubaMiddleware {
    fn inbound(&self, state: &mut State) {
        state.put(ScubaMiddlewareState(self.scuba.clone()));
    }

    fn outbound(&self, state: &mut State, response: &mut Response<Body>) {
        if let Some(uri) = Uri::try_borrow_from(&state) {
            if uri.path() == "/health_check" {
                return;
            }
        }

        log_stats(
            self.log_file.clone(),
            state,
            &response.status(),
            response.body().content_length(),
        );
    }
}
