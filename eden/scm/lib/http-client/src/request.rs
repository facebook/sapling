/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::{TryFrom, TryInto};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::AcqRel;
use std::time::Duration;

use curl::{
    self,
    easy::{Easy2, HttpVersion, List},
};
use serde::Serialize;
use url::Url;

use crate::{
    errors::HttpClientError,
    handler::{Buffered, Configure, Streaming},
    receiver::{ChannelReceiver, Receiver},
    response::{AsyncResponse, Response},
};

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Method {
    Get,
    Head,
    Post,
    Put,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct MinTransferSpeed {
    pub min_bytes_per_second: u32,
    pub grace_period: Duration,
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Method::Get => "GET",
                Method::Head => "HEAD",
                Method::Post => "POST",
                Method::Put => "PUT",
            }
        )
    }
}

/// A subset of the `Request` builder. Preserved in curl types.
/// Expose the request in curl handler callback context.
#[derive(Clone, Debug)]
pub struct RequestContext {
    id: RequestId,
    url: Url,
    method: Method,
}

/// Identity of a request.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct RequestId(usize);

/// A builder struct for HTTP requests, designed to be
/// a more egonomic API for setting up a curl handle.
#[derive(Clone, Debug)]
pub struct Request {
    pub(crate) ctx: RequestContext,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    cert: Option<PathBuf>,
    key: Option<PathBuf>,
    cainfo: Option<PathBuf>,
    timeout: Option<Duration>,
    http_version: HttpVersion,
    min_transfer_speed: Option<MinTransferSpeed>,
}

impl RequestContext {
    /// Create a [`RequestContext`].
    pub fn new(url: Url, method: Method) -> Self {
        static ID: AtomicUsize = AtomicUsize::new(0);
        let id = RequestId(ID.fetch_add(1, AcqRel));
        Self { id, url, method }
    }

    /// Obtain the HTTP url of the request.
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Obtain the HTTP method of the request.
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// Obtain the request Id.
    ///
    /// The Id is automatically assigned and uniquely identifies the requests
    /// in this process.
    pub fn id(&self) -> RequestId {
        self.id
    }
}

impl Request {
    pub fn new(url: Url, method: Method) -> Self {
        let ctx = RequestContext::new(url, method);
        Self {
            ctx,
            // Always set Expect so we can disable curl automatically expecting "100-continue".
            // That would require two response reads, which breaks the http_client model.
            headers: vec![("Expect".to_string(), "".to_string())],
            body: None,
            cert: None,
            key: None,
            cainfo: None,
            timeout: None,
            // Attempt to use HTTP/2 by default. Will fall back to HTTP/1.1
            // if version negotiation with the server fails.
            http_version: HttpVersion::V2,
            min_transfer_speed: None,
        }
    }

    /// Obtain the request Id.
    ///
    /// The Id is automatically assigned and uniquely identifies the requests
    /// in this process.
    pub fn id(&self) -> RequestId {
        self.ctx.id()
    }

    /// Create a GET request.
    pub fn get(url: Url) -> Self {
        Self::new(url, Method::Get)
    }

    /// Create a HEAD request.
    pub fn head(url: Url) -> Self {
        Self::new(url, Method::Head)
    }

    /// Create a POST request.
    pub fn post(url: Url) -> Self {
        Self::new(url, Method::Post)
    }

    /// Create a PUT request.
    pub fn put(url: Url) -> Self {
        Self::new(url, Method::Put)
    }

    /// Set the data to be uploaded in the request body.
    pub fn body<B: Into<Vec<u8>>>(self, data: B) -> Self {
        Self {
            body: Some(data.into()),
            ..self
        }
    }

    /// Set the http version for this request. Defaults to HTTP/2.
    pub fn http_version(self, http_version: HttpVersion) -> Self {
        Self {
            http_version,
            ..self
        }
    }

    /// Set transfer speed options for this request.
    pub fn min_transfer_speed(self, min_transfer_speed: MinTransferSpeed) -> Self {
        Self {
            min_transfer_speed: Some(min_transfer_speed),
            ..self
        }
    }

    /// Serialize the given value as JSON and use it as the request body.
    pub fn json<S: Serialize>(self, value: &S) -> Result<Self, serde_json::Error> {
        Ok(self
            .header("Content-Type", "application/json")
            .body(serde_json::to_vec(value)?))
    }

    /// Serialize the given value as CBOR and use it as the request body.
    pub fn cbor<S: Serialize>(self, value: &S) -> Result<Self, serde_cbor::Error> {
        Ok(self
            .header("Content-Type", "application/cbor")
            .body(serde_cbor::to_vec(value)?))
    }

    /// Set a request header.
    pub fn header(mut self, name: impl ToString, value: impl ToString) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    /// Specify a client certificate for TLS mutual authentiation.
    ///
    /// This should be a path to a base64-encoded PEM file containing the
    /// client's X.509 certificate. When using a client certificate, the client
    /// must also provide the corresponding private key; this can either be
    /// concatenated to the certificate in the PEM file (in which case it will
    /// be used automatically), or specified separately via the `key` method.
    pub fn cert(self, cert: impl AsRef<Path>) -> Self {
        Self {
            cert: Some(cert.as_ref().into()),
            ..self
        }
    }

    /// Specify a client private key for TLS mutual authentiation.
    ///
    /// This method can be used to specify the path to the client's private
    /// key if this key was not included in the certificate file specified via
    /// the `cert` method.
    pub fn key(self, key: impl AsRef<Path>) -> Self {
        Self {
            key: Some(key.as_ref().into()),
            ..self
        }
    }

    /// Specify a CA certificate bundle to be used to verify the
    /// server's certificate. If not specified, the client will
    /// use the system default CA certificate bundle.
    pub fn cainfo(self, cainfo: impl AsRef<Path>) -> Self {
        Self {
            cainfo: Some(cainfo.as_ref().into()),
            ..self
        }
    }

    /// Set the maximum time this request is allowed to take.
    pub fn timeout(self, timeout: Duration) -> Self {
        Self {
            timeout: Some(timeout),
            ..self
        }
    }

    /// Execute the request, blocking until completion.
    ///
    /// This method is intended as a simple way to perform
    /// one-off HTTP requests. `HttpClient::send` should be
    /// used instead of this method when working with many
    /// concurrent requests or large requests that require
    /// progress reporting.
    pub fn send(self) -> Result<Response, HttpClientError> {
        let mut easy: Easy2<Buffered> = self.try_into()?;
        easy.perform()?;
        Response::try_from(easy.get_mut())
    }

    /// Execute this request asynchronously.
    pub async fn send_async(self) -> Result<AsyncResponse, HttpClientError> {
        let (receiver, streams) = ChannelReceiver::new();
        let request = self.into_streaming(receiver);

        // Spawn the request as another task, which will block
        // the worker it is scheduled on until completion.
        let _ = tokio::task::spawn_blocking(move || request.send());

        AsyncResponse::new(streams).await
    }

    /// Turn this `Request` into a streaming request. The
    /// received data for this request will be passed as
    /// it arrives to the given `Receiver`.
    pub fn into_streaming<R>(self, receiver: R) -> StreamRequest<R> {
        StreamRequest {
            request: self,
            receiver,
        }
    }

    /// Turn this `Request` into a `curl::Easy2` handle using
    /// the given `Handler` to process the response.
    pub(crate) fn into_handle<H: Configure>(self, handler: H) -> Result<Easy2<H>, HttpClientError> {
        let body_size = self.body.as_ref().map(|body| body.len() as u64);
        let handler = handler.with_payload(self.body);

        let mut easy = Easy2::new(handler);
        easy.url(self.ctx.url.as_str())?;

        // Configure the handle for the desired HTTP method.
        match self.ctx.method {
            Method::Get => {}
            Method::Head => {
                easy.nobody(true)?;
            }
            Method::Post => {
                easy.post(true)?;
                if let Some(size) = body_size {
                    easy.post_field_size(size)?;
                }
            }
            Method::Put => {
                easy.upload(true)?;
                if let Some(size) = body_size {
                    easy.in_filesize(size)?;
                }
            }
        }

        // Add headers.
        let mut headers = List::new();
        for (name, value) in self.headers {
            let header = format!("{}: {}", name, value);
            headers.append(&header)?;
        }
        easy.http_headers(headers)?;

        // Set up client credentials for mTLS.
        if let Some(cert) = self.cert {
            easy.ssl_cert(cert)?;
        }
        if let Some(key) = self.key {
            easy.ssl_key(key)?;
        }

        // Windows enables ssl revocation checking by default, which doesn't work inside the
        // datacenter.
        #[cfg(windows)]
        {
            use curl::easy::SslOpt;
            let mut ssl_opts = SslOpt::new();
            ssl_opts.no_revoke(true);
            easy.ssl_options(&ssl_opts)?;
        }

        if let Some(cainfo) = self.cainfo {
            easy.cainfo(cainfo)?;
        }

        if let Some(timeout) = self.timeout {
            easy.timeout(timeout)?;
        }

        easy.http_version(self.http_version)?;

        if let Some(mts) = self.min_transfer_speed {
            easy.low_speed_limit(mts.min_bytes_per_second)?;
            easy.low_speed_time(mts.grace_period)?;
        }

        // Tell libcurl to report progress to the handler.
        easy.progress(true)?;

        Ok(easy)
    }
}

impl TryFrom<Request> for Easy2<Buffered> {
    type Error = HttpClientError;

    fn try_from(req: Request) -> Result<Self, Self::Error> {
        let ctx = req.ctx.clone();
        req.into_handle(Buffered::new(ctx))
    }
}

pub struct StreamRequest<R> {
    pub(crate) request: Request,
    pub(crate) receiver: R,
}

impl<R: Receiver> StreamRequest<R> {
    pub fn send(self) -> Result<(), HttpClientError> {
        let mut easy: Easy2<Streaming<R>> = self.try_into()?;
        let res = easy.perform().map_err(Into::into);
        let _ = easy
            .get_mut()
            .take_receiver()
            .expect("Receiver is gone; this should never happen")
            .done(res);
        Ok(())
    }
}

impl<R: Receiver> TryFrom<StreamRequest<R>> for Easy2<Streaming<R>> {
    type Error = HttpClientError;

    fn try_from(req: StreamRequest<R>) -> Result<Self, Self::Error> {
        let StreamRequest { request, receiver } = req;
        let ctx = request.ctx.clone();
        request.into_handle(Streaming::new(receiver, ctx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use anyhow::Result;
    use futures::TryStreamExt;
    use http::{
        header::{self, HeaderName, HeaderValue},
        StatusCode,
    };
    use mockito::{mock, Matcher};
    use serde_json::json;

    #[test]
    fn test_get() -> Result<()> {
        let mock = mock("GET", "/test")
            .with_status(200)
            .match_header("X-Api-Key", "1234")
            .with_header("Content-Type", "text/plain")
            .with_header("X-Served-By", "mock")
            .with_body("Hello, world!")
            .create();

        let url = Url::parse(&mockito::server_url())?.join("test")?;
        let res = Request::get(url).header("X-Api-Key", "1234").send()?;

        mock.assert();

        assert_eq!(res.status, StatusCode::OK);
        assert_eq!(&*res.body, &b"Hello, world!"[..]);
        assert_eq!(
            res.headers.get(header::CONTENT_TYPE).unwrap(),
            HeaderValue::from_static("text/plain")
        );
        assert_eq!(
            res.headers
                .get(HeaderName::from_bytes(b"X-Served-By")?)
                .unwrap(),
            HeaderValue::from_static("mock")
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_async_get() -> Result<()> {
        let mock = mock("GET", "/test")
            .with_status(200)
            .match_header("X-Api-Key", "1234")
            .with_header("Content-Type", "text/plain")
            .with_header("X-Served-By", "mock")
            .with_body("Hello, world!")
            .create();

        let url = Url::parse(&mockito::server_url())?.join("test")?;
        let res = Request::get(url)
            .header("X-Api-Key", "1234")
            .send_async()
            .await?;

        mock.assert();

        assert_eq!(res.status, StatusCode::OK);
        assert_eq!(
            res.headers.get(header::CONTENT_TYPE).unwrap(),
            HeaderValue::from_static("text/plain")
        );
        assert_eq!(
            res.headers
                .get(HeaderName::from_bytes(b"X-Served-By")?)
                .unwrap(),
            HeaderValue::from_static("mock")
        );

        let body = res.body.try_concat().await?;
        assert_eq!(&*body, &b"Hello, world!"[..]);

        Ok(())
    }

    #[test]
    fn test_head() -> Result<()> {
        let mock = mock("HEAD", "/test")
            .with_status(200)
            .match_header("X-Api-Key", "1234")
            .with_header("Content-Type", "text/plain")
            .with_header("X-Served-By", "mock")
            .create();

        let url = Url::parse(&mockito::server_url())?.join("test")?;
        let res = Request::head(url).header("X-Api-Key", "1234").send()?;

        mock.assert();

        assert_eq!(res.status, StatusCode::OK);
        assert!(res.body.is_empty());
        assert_eq!(
            res.headers.get(header::CONTENT_TYPE).unwrap(),
            HeaderValue::from_static("text/plain")
        );
        assert_eq!(
            res.headers
                .get(HeaderName::from_bytes(b"X-Served-By")?)
                .unwrap(),
            HeaderValue::from_static("mock")
        );

        Ok(())
    }

    #[test]
    fn test_post() -> Result<()> {
        let body = "foo=hello&bar=world";

        let mock = mock("POST", "/test")
            .with_status(201)
            .match_header("Content-Type", "application/x-www-form-urlencoded")
            .match_body(Matcher::Exact(body.into()))
            .create();

        let url = Url::parse(&mockito::server_url())?.join("test")?;
        let res = Request::post(url).body(body.as_bytes()).send()?;

        mock.assert();
        assert_eq!(res.status, StatusCode::CREATED);

        Ok(())
    }

    #[test]
    fn test_post_large() -> Result<()> {
        let body_bytes = vec![65; 1024 * 1024];
        let body = String::from_utf8_lossy(body_bytes.as_ref());

        let mock = mock("POST", "/test")
            .with_status(201)
            .match_header("Expect", Matcher::Missing)
            .match_body(Matcher::Exact(body.into()))
            .create();

        let url = Url::parse(&mockito::server_url())?.join("test")?;
        let res = Request::post(url).body(body_bytes).send()?;

        mock.assert();
        assert_eq!(res.status, StatusCode::CREATED);

        Ok(())
    }

    #[test]
    fn test_put() -> Result<()> {
        let body = "Hello, world!";

        let mock = mock("PUT", "/test")
            .with_status(201)
            .match_header("Content-Type", "text/plain")
            .match_body(Matcher::Exact(body.into()))
            .create();

        let url = Url::parse(&mockito::server_url())?.join("test")?;
        let res = Request::put(url)
            .header("Content-Type", "text/plain")
            .body(body.as_bytes())
            .send()?;

        mock.assert();
        assert_eq!(res.status, StatusCode::CREATED);

        Ok(())
    }

    #[test]
    fn test_json() -> Result<()> {
        let body = json!({
            "foo": "bar",
            "hello": "world"
        });

        let mock = mock("POST", "/test")
            .with_status(201)
            .match_header("Content-Type", "application/json")
            .match_body(Matcher::Json(body.clone()))
            .create();

        let url = Url::parse(&mockito::server_url())?.join("test")?;
        let res = Request::post(url).json(&body)?.send()?;

        mock.assert();
        assert_eq!(res.status, StatusCode::CREATED);

        Ok(())
    }

    #[test]
    fn test_cbor() -> Result<()> {
        #[derive(Serialize)]
        struct Body<'a> {
            foo: &'a str,
            hello: &'a str,
        }

        let body = Body {
            foo: "bar",
            hello: "world",
        };

        let mock = mock("POST", "/test")
            .with_status(201)
            .match_header("Content-Type", "application/cbor")
            // As of v0.25, mockito doesn't support matching binary bodies.
            .match_body(Matcher::Any)
            .create();

        let url = Url::parse(&mockito::server_url())?.join("test")?;
        let res = Request::post(url).cbor(&body)?.send()?;

        mock.assert();
        assert_eq!(res.status, StatusCode::CREATED);

        Ok(())
    }

    const DUMMY_URL_STR: &str = "https://a.example.com/b";
    const DUMMY_METOD: Method = Method::Get;

    impl RequestContext {
        /// Dummy RequestContext for testing.
        pub(crate) fn dummy() -> Self {
            Self::new(Url::parse(DUMMY_URL_STR).unwrap(), DUMMY_METOD)
        }
    }

    #[test]
    fn test_request_context() {
        let req = RequestContext::dummy();
        assert_eq!(req.url().as_str(), DUMMY_URL_STR);
        assert_eq!(req.method(), &DUMMY_METOD);

        let req2 = RequestContext::dummy();
        assert_ne!(req.id(), req2.id());
    }
}
