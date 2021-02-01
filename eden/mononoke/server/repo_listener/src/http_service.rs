/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context, Error, Result};
use futures::future::{BoxFuture, FutureExt};
use http::{request::Parts, HeaderMap, HeaderValue, Method, Request, Response, Uri};
use hyper::{service::Service, Body};
use sha1::{Digest, Sha1};
use slog::{debug, error, Logger};
use sshrelay::Metadata;
use std::io::Cursor;
use std::marker::PhantomData;
use std::task;
use thiserror::Error;
use tokio::io::AsyncReadExt;

use crate::connection_acceptor::{
    self, AcceptedConnection, ChannelConn, FramedConn, MononokeStream,
};

const HEADER_CLIENT_DEBUG: &str = "x-client-debug";
const HEADER_WEBSOCKET_KEY: &str = "sec-websocket-key";
const HEADER_WEBSOCKET_ACCEPT: &str = "sec-websocket-accept";

// See https://tools.ietf.org/html/rfc6455#section-1.3
const WEBSOCKET_MAGIC_KEY: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

#[derive(Error, Debug)]
pub enum HttpError {
    #[error("Bad request")]
    BadRequest(#[source] Error),

    #[error("Method not acceptable")]
    NotAcceptable,

    #[error("Not found")]
    NotFound,

    #[error("Internal server error")]
    InternalServerError(#[source] Error),
}

impl HttpError {
    pub fn internal(e: impl Into<Error>) -> Self {
        Self::InternalServerError(e.into())
    }

    pub fn http_response(&self) -> http::Result<Response<Body>> {
        let status = match self {
            Self::BadRequest(..) => http::StatusCode::BAD_REQUEST,
            Self::NotAcceptable => http::StatusCode::NOT_ACCEPTABLE,
            Self::NotFound => http::StatusCode::NOT_FOUND,
            Self::InternalServerError(..) => http::StatusCode::INTERNAL_SERVER_ERROR,
        };

        let body = match self {
            Self::BadRequest(ref e) => Body::from(format!("{:#}", e)),
            Self::NotAcceptable => Body::empty(),
            Self::NotFound => Body::empty(),
            Self::InternalServerError(ref e) => Body::from(format!("{:#}", e)),
        };

        Response::builder().status(status).body(body)
    }
}

pub struct MononokeHttpService<S> {
    pub conn: AcceptedConnection,
    sock: PhantomData<S>,
}

impl<S> MononokeHttpService<S> {
    pub fn new(conn: AcceptedConnection) -> Self {
        Self {
            conn,
            sock: PhantomData,
        }
    }
}

impl<S> Clone for MononokeHttpService<S> {
    fn clone(&self) -> Self {
        Self {
            conn: self.conn.clone(),
            sock: PhantomData,
        }
    }
}

impl<S> MononokeHttpService<S>
where
    S: MononokeStream,
{
    async fn handle(
        &self,
        method: Method,
        uri: &Uri,
        headers: HeaderMap<HeaderValue>,
        body: Body,
    ) -> Result<Response<Body>, HttpError> {
        let upgrade = headers
            .get(http::header::UPGRADE)
            .as_ref()
            .map(|h| h.to_str())
            .transpose()
            .with_context(|| {
                // NOTE: We're just stringifying here: the borrow is fine.
                #[allow(clippy::borrow_interior_mutable_const)]
                let header = &http::header::UPGRADE;
                format!("Invalid header: {}", header)
            })
            .map_err(HttpError::BadRequest)?;

        if upgrade == Some("websocket") {
            return self.handle_websocket_request(&uri, &headers, body).await;
        }

        if uri.path() == "/netspeedtest" {
            return crate::netspeedtest::handle(method, &headers, body).await;
        }

        if method == Method::GET && (uri.path() == "/" || uri.path() == "/health_check") {
            let res = Response::builder()
                .status(http::StatusCode::OK)
                .body("I_AM_ALIVE".into())
                .map_err(HttpError::internal)?;
            return Ok(res);
        }

        Err(HttpError::NotFound)
    }

    async fn handle_websocket_request(
        &self,
        uri: &Uri,
        headers: &HeaderMap<HeaderValue>,
        body: Body,
    ) -> Result<Response<Body>, HttpError> {
        let reponame = uri.path().trim_matches('/').to_string();

        let websocket_key = calculate_websocket_accept(headers);

        let res = Response::builder()
            .status(http::StatusCode::SWITCHING_PROTOCOLS)
            .header(http::header::CONNECTION, "upgrade")
            .header(http::header::UPGRADE, "websocket")
            .header(HEADER_WEBSOCKET_ACCEPT, websocket_key)
            .body(Body::empty())
            .map_err(HttpError::internal)?;

        let metadata = try_convert_headers_to_metadata(self.conn.is_trusted, &headers)
            .await
            .context("Invalid metadata")
            .map_err(HttpError::BadRequest)?;

        let debug = headers.get(HEADER_CLIENT_DEBUG).is_some();

        let this = self.clone();

        let fut = async move {
            let io = body
                .on_upgrade()
                .await
                .context("Failed to upgrade connection")?;

            // NOTE: We unwrap() here because we explicitly parameterize the MononokeHttpService
            // over its socket type. If we get it wrong then that'd be a deterministic failure that
            // would show up in tests.
            let hyper::upgrade::Parts { io, read_buf, .. } = io.downcast::<S>().unwrap();

            let (rx, tx) = tokio::io::split(io);
            let rx = AsyncReadExt::chain(Cursor::new(read_buf), rx);

            let conn = FramedConn::setup(rx, tx);
            let channels = ChannelConn::setup(conn);

            connection_acceptor::handle_wireproto(this.conn, channels, reponame, metadata, debug)
                .await
                .context("Failed to handle_wireproto")?;

            Result::<_, Error>::Ok(())
        };

        self.conn
            .pending
            .spawn_task(fut, "Failed to handle websocket channel");

        Ok(res)
    }

    fn logger(&self) -> &Logger {
        &self.conn.pending.acceptor.logger
    }
}

impl<S> Service<Request<Body>> for MononokeHttpService<S>
where
    S: MononokeStream,
{
    type Response = Response<Body>;
    type Error = http::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _cx: &mut task::Context<'_>) -> task::Poll<Result<(), Self::Error>> {
        task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let this = self.clone();

        async move {
            let (
                Parts {
                    method,
                    uri,
                    headers,
                    ..
                },
                body,
            ) = req.into_parts();

            debug!(this.logger(), "{} {}", method, uri);

            let res = this
                .handle(method.clone(), &uri, headers, body)
                .await
                .or_else(|e| {
                    error!(
                        this.logger(),
                        "http service error: {} {}: {:#}", method, uri, e
                    );

                    e.http_response()
                });

            // NOTE: If we fail to even generate the response here, this will crash
            // serve_connection in Hyper, so we don't actually need to log this here.
            res
        }
        .boxed()
    }
}

// See https://tools.ietf.org/html/rfc6455#section-1.3
fn calculate_websocket_accept(headers: &HeaderMap<HeaderValue>) -> String {
    let mut sha1 = Sha1::new();

    // This is OK to fall back to empty, because we only need to give
    // this header, if it's asked for. In case of hg<->mononoke with
    // no Proxygen in between, this header will be missing and the result
    // ignored.
    if let Some(header) = headers.get(HEADER_WEBSOCKET_KEY) {
        sha1.input(header.as_ref());
    }
    sha1.input(WEBSOCKET_MAGIC_KEY.as_bytes());
    let hash: [u8; 20] = sha1.result().into();
    base64::encode(&hash)
}

#[cfg(fbcode_build)]
async fn try_convert_headers_to_metadata(
    is_trusted: bool,
    headers: &HeaderMap<HeaderValue>,
) -> Result<Option<Metadata>> {
    use percent_encoding::percent_decode;
    use permission_checker::MononokeIdentity;
    use session_id::generate_session_id;
    use sshrelay::Priority;
    use std::net::IpAddr;

    const HEADER_ENCODED_CLIENT_IDENTITY: &str = "x-fb-validated-client-encoded-identity";
    const HEADER_CLIENT_IP: &str = "tfb-orig-client-ip";

    if !is_trusted {
        return Ok(None);
    }

    if let (Some(encoded_identities), Some(client_address)) = (
        headers.get(HEADER_ENCODED_CLIENT_IDENTITY),
        headers.get(HEADER_CLIENT_IP),
    ) {
        let json_identities = percent_decode(encoded_identities.as_ref())
            .decode_utf8()
            .context("Invalid encoded identities")?;
        let identities = MononokeIdentity::try_from_json_encoded(&json_identities)
            .context("Invalid identities")?;
        let ip_addr = client_address
            .to_str()?
            .parse::<IpAddr>()
            .context("Invalid IP Address")?;

        // In the case of HTTP proxied/trusted requests we only have the
        // guarantee that we can trust the forwarded credentials. Beyond
        // this point we can't trust anything else, ACL checks have not
        // been performed, so set 'is_trusted' to 'false' here to enforce
        // further checks.
        Ok(Some(
            Metadata::new(
                Some(&generate_session_id().to_string()),
                false,
                identities,
                Priority::Default,
                headers.contains_key(HEADER_CLIENT_DEBUG),
                Some(ip_addr),
            )
            .await,
        ))
    } else {
        Ok(None)
    }
}

#[cfg(not(fbcode_build))]
async fn try_convert_headers_to_metadata(
    _is_trusted: bool,
    _headers: &HeaderMap<HeaderValue>,
) -> Result<Option<Metadata>> {
    Ok(None)
}
