/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::AcqRel;
use std::time::Duration;

use curl::{
    self,
    easy::{Easy2, HttpVersion, List},
};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use serde::Serialize;
use url::Url;

use crate::{
    errors::HttpClientError,
    event_listeners::RequestCreationEventListeners,
    event_listeners::RequestEventListeners,
    handler::{Buffered, HandlerExt, Streaming},
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
#[cfg_attr(test, derive(Clone))]
#[derive(Debug)]
pub struct RequestContext {
    id: RequestId,
    url: Url,
    method: Method,
    pub(crate) body: Option<Vec<u8>>,
    pub(crate) event_listeners: RequestEventListeners,
}

/// Identity of a request.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct RequestId(usize);

/// A builder struct for HTTP requests, designed to be
/// a more egonomic API for setting up a curl handle.
#[cfg_attr(test, derive(Clone))]
#[derive(Debug)]
pub struct Request {
    ctx: RequestContext,
    headers: Vec<(String, String)>,
    cert: Option<PathBuf>,
    key: Option<PathBuf>,
    cainfo: Option<PathBuf>,
    timeout: Option<Duration>,
    http_version: HttpVersion,
    min_transfer_speed: Option<MinTransferSpeed>,
    verify_tls_host: bool,
    verify_tls_cert: bool,
}

static REQUEST_CREATION_LISTENERS: Lazy<RwLock<RequestCreationEventListeners>> =
    Lazy::new(Default::default);

impl RequestContext {
    /// Create a [`RequestContext`].
    pub fn new(url: Url, method: Method) -> Self {
        static ID: AtomicUsize = AtomicUsize::new(0);
        let id = RequestId(ID.fetch_add(1, AcqRel));
        Self {
            id,
            url,
            method,
            body: None,
            event_listeners: Default::default(),
        }
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

    /// Set the data to be uploaded in the request body.
    pub fn body<B: Into<Vec<u8>>>(mut self, data: B) -> Self {
        self.body = Some(data.into());
        self
    }

    /// Set the data to be uploaded in the request body.
    pub fn set_body<B: Into<Vec<u8>>>(&mut self, data: B) {
        self.body = Some(data.into());
    }

    /// Provide a way to register event callbacks.
    pub fn event_listeners(&mut self) -> &mut RequestEventListeners {
        &mut self.event_listeners
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
            cert: None,
            key: None,
            cainfo: None,
            timeout: None,
            // Attempt to use HTTP/2 by default. Will fall back to HTTP/1.1
            // if version negotiation with the server fails.
            http_version: HttpVersion::V2,
            min_transfer_speed: None,
            verify_tls_host: true,
            verify_tls_cert: true,
        }
    }

    /// Obtain the request Id.
    ///
    /// The Id is automatically assigned and uniquely identifies the requests
    /// in this process.
    pub fn id(&self) -> RequestId {
        self.ctx.id()
    }

    /// Get a reference to this request's context.
    ///
    /// The request context contains most of the information about the request,
    /// such as URL, method, etc.
    pub fn ctx(&self) -> &RequestContext {
        &self.ctx
    }

    /// Get a mutable reference to this request's context.
    pub fn ctx_mut(&mut self) -> &mut RequestContext {
        &mut self.ctx
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
    pub fn body<B: Into<Vec<u8>>>(mut self, data: B) -> Self {
        self.set_body(data);
        self
    }

    /// Set the data to be uploaded in the request body.
    pub fn set_body<B: Into<Vec<u8>>>(&mut self, data: B) -> &mut Self {
        self.ctx.set_body(data);
        self
    }

    /// Set the http version for this request. Defaults to HTTP/2.
    pub fn http_version(mut self, version: HttpVersion) -> Self {
        self.set_http_version(version);
        self
    }

    /// Set the http version for this request. Defaults to HTTP/2.
    pub fn set_http_version(&mut self, version: HttpVersion) -> &mut Self {
        self.http_version = version;
        self
    }

    /// Set transfer speed options for this request.
    pub fn min_transfer_speed(mut self, min_transfer_speed: MinTransferSpeed) -> Self {
        self.set_min_transfer_speed(min_transfer_speed);
        self
    }

    /// Set transfer speed options for this request.
    pub fn set_min_transfer_speed(&mut self, min_transfer_speed: MinTransferSpeed) -> &mut Self {
        self.min_transfer_speed = Some(min_transfer_speed);
        self
    }

    /// Serialize the given value as JSON and use it as the request body.
    pub fn json<S: Serialize>(mut self, value: &S) -> Result<Self, serde_json::Error> {
        self.set_json_body(value)?;
        Ok(self)
    }

    /// Serialize the given value as JSON and use it as the request body.
    pub fn set_json_body<S: Serialize>(
        &mut self,
        value: &S,
    ) -> Result<&mut Self, serde_json::Error> {
        self.set_header("Content-Type", "application/json")
            .set_body(serde_json::to_vec(value)?);
        Ok(self)
    }

    /// Serialize the given value as CBOR and use it as the request body.
    pub fn cbor<S: Serialize>(mut self, value: &S) -> Result<Self, serde_cbor::Error> {
        self.set_cbor_body(value)?;
        Ok(self)
    }


    /// Serialize the given value as CBOR and use it as the request body.
    pub fn set_cbor_body<S: Serialize>(
        &mut self,
        value: &S,
    ) -> Result<&mut Self, serde_cbor::Error> {
        self.set_header("Content-Type", "application/cbor")
            .set_body(serde_cbor::to_vec(value)?);
        Ok(self)
    }

    /// Set a request header.
    pub fn header(mut self, name: impl ToString, value: impl ToString) -> Self {
        self.set_header(name, value);
        self
    }

    /// Set a request header.
    pub fn set_header(&mut self, name: impl ToString, value: impl ToString) -> &mut Self {
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
    pub fn cert(mut self, cert: impl AsRef<Path>) -> Self {
        self.set_cert(cert);
        self
    }

    /// Specify a client certificate for TLS mutual authentiation.
    ///
    /// This should be a path to a base64-encoded PEM file containing the
    /// client's X.509 certificate. When using a client certificate, the client
    /// must also provide the corresponding private key; this can either be
    /// concatenated to the certificate in the PEM file (in which case it will
    /// be used automatically), or specified separately via the `key` method.
    pub fn set_cert(&mut self, cert: impl AsRef<Path>) -> &mut Self {
        self.cert = Some(cert.as_ref().into());
        self
    }

    /// Specify a client private key for TLS mutual authentiation.
    ///
    /// This method can be used to specify the path to the client's private
    /// key if this key was not included in the certificate file specified via
    /// the `cert` method.
    pub fn key(mut self, key: impl AsRef<Path>) -> Self {
        self.set_key(key);
        self
    }

    /// Specify a client private key for TLS mutual authentiation.
    ///
    /// This method can be used to specify the path to the client's private
    /// key if this key was not included in the certificate file specified via
    /// the `cert` method.
    pub fn set_key(&mut self, key: impl AsRef<Path>) -> &mut Self {
        self.key = Some(key.as_ref().into());
        self
    }

    /// Specify a CA certificate bundle to be used to verify the
    /// server's certificate. If not specified, the client will
    /// use the system default CA certificate bundle.
    pub fn cainfo(mut self, cainfo: impl AsRef<Path>) -> Self {
        self.set_cainfo(cainfo);
        self
    }

    /// Specify a CA certificate bundle to be used to verify the
    /// server's certificate. If not specified, the client will
    /// use the system default CA certificate bundle.
    pub fn set_cainfo(&mut self, cainfo: impl AsRef<Path>) -> &mut Self {
        self.cainfo = Some(cainfo.as_ref().into());
        self
    }

    /// Set the maximum time this request is allowed to take.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.set_timeout(timeout);
        self
    }

    /// Set the maximum time this request is allowed to take.
    pub fn set_timeout(&mut self, timeout: Duration) -> &mut Self {
        self.timeout = Some(timeout);
        self
    }

    /// Configure whether the client should verify that the server's hostname
    /// matches either the common name (CN) or a subject alternate name (SAN)
    /// present in the server's TLS certificate. Disabling this option will make
    /// the connection insecure. This is primarily useful for testing.
    pub fn verify_tls_host(mut self, verify: bool) -> Self {
        self.set_verify_tls_host(verify);
        self
    }

    /// Configure whether the client should verify that the server's hostname
    /// matches either the common name (CN) or a subject alternate name (SAN)
    /// present in the server's TLS certificate. Disabling this option will make
    /// the connection insecure. This is primarily useful for testing.
    pub fn set_verify_tls_host(&mut self, verify: bool) -> &mut Self {
        self.verify_tls_host = verify;
        self
    }

    /// Configure whether the client should verify the authenticity of the
    /// server's TLS certificate using the CA certificate bundle specified
    /// via `cainfo` (or the default CA bundle if not set). This option is
    /// enabled by default; disabling it will make the connection insecure.
    /// This is primarily useful for testing.
    pub fn verify_tls_cert(mut self, verify: bool) -> Self {
        self.set_verify_tls_cert(verify);
        self
    }

    /// Configure whether the client should verify the authenticity of the
    /// server's TLS certificate using the CA certificate bundle specified
    /// via `cainfo` (or the default CA bundle if not set). This option is
    /// enabled by default; disabling it will make the connection insecure.
    /// This is primarily useful for testing.
    pub fn set_verify_tls_cert(&mut self, verify: bool) -> &mut Self {
        self.verify_tls_cert = verify;
        self
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

    /// Turn this `Request` into a `curl::Easy2` handle using the given
    /// `Handler` to process the response.
    pub(crate) fn into_handle<H: HandlerExt>(
        mut self,
        create_handler: impl FnOnce(RequestContext) -> H,
    ) -> Result<Easy2<H>, HttpClientError> {
        // Allow request creation listeners to configure the Request before we
        // use it, potentially overriding settings explicitly configured via
        // the methods on Request.
        REQUEST_CREATION_LISTENERS
            .read()
            .trigger_new_request(&mut self);

        let body_size = self.ctx.body.as_ref().map(|body| body.len() as u64);
        let url = self.ctx.url.clone();
        let handler = create_handler(self.ctx);

        let mut easy = Easy2::new(handler);
        easy.url(url.as_str())?;

        // Configure the handle for the desired HTTP method.
        match easy.get_ref().request_context().method {
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

        // Configure TLS verification.
        easy.ssl_verify_host(self.verify_tls_host)?;
        easy.ssl_verify_peer(self.verify_tls_cert)?;

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

    /// Register a callback function that is called on new requests.
    pub fn on_new_request(f: impl Fn(&mut Self) + Send + Sync + 'static) {
        REQUEST_CREATION_LISTENERS.write().on_new_request(f);
    }
}

impl TryFrom<Request> for Easy2<Buffered> {
    type Error = HttpClientError;

    fn try_from(req: Request) -> Result<Self, Self::Error> {
        req.into_handle(Buffered::new)
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
        request.into_handle(|ctx| Streaming::new(receiver, ctx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::Ordering::Acquire;
    use std::sync::Arc;

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

    #[test]
    fn test_request_callback() -> Result<()> {
        let called = Arc::new(AtomicUsize::new(0));
        Request::on_new_request({
            let called = called.clone();
            move |req| {
                // The callback can receive requests in other tests.
                // So we need to check the request is sent by this test.
                if req.ctx().url().path() == "/test_callback" {
                    called.fetch_add(1, AcqRel);
                }
            }
        });

        let mock = mock("HEAD", "/test_callback").with_status(200).create();
        let url = Url::parse(&mockito::server_url())?.join("test_callback")?;
        let _res = Request::head(url).send()?;

        mock.assert();
        assert_eq!(called.load(Acquire), 1);

        Ok(())
    }
}
