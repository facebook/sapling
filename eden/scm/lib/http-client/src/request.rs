/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::AcqRel;
use std::time::Duration;
use std::time::SystemTime;

use curl::easy::Easy2;
use curl::easy::HttpVersion;
use curl::easy::List;
use http::header;
use lru_cache::LruCache;
use maplit::hashmap;
use once_cell::sync::Lazy;
use openssl::pkcs12::Pkcs12;
use openssl::pkey::PKey;
use openssl::x509::X509;
use parking_lot::Mutex;
use parking_lot::RwLock;
use serde::Serialize;
use url::Url;

use crate::errors::HttpClientError;
use crate::event_listeners::RequestCreationEventListeners;
use crate::event_listeners::RequestEventListeners;
use crate::handler::Buffered;
use crate::handler::HandlerExt;
use crate::handler::Streaming;
use crate::receiver::ChannelReceiver;
use crate::receiver::Receiver;
use crate::response::AsyncResponse;
use crate::response::Response;

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

#[derive(Clone, Debug, PartialEq)]
pub enum Encoding {
    Identity,
    Brotli,
    Deflate,
    Gzip,
    Zstd,
    Other(String),
}

impl Encoding {
    pub fn all() -> Vec<Self> {
        use Encoding::*;
        vec![Zstd, Brotli, Gzip, Deflate]
    }
}

impl<'a> From<&'a str> for Encoding {
    fn from(encoding: &'a str) -> Self {
        use Encoding::*;
        match encoding {
            "identity" => Identity,
            "br" => Brotli,
            "deflate" => Deflate,
            "gzip" => Gzip,
            "zstd" => Zstd,
            other => Other(other.into()),
        }
    }
}

impl AsRef<str> for Encoding {
    fn as_ref(&self) -> &str {
        use Encoding::*;
        match self {
            Identity => "identity",
            Brotli => "br",
            Deflate => "deflate",
            Gzip => "gzip",
            Zstd => "zstd",
            Other(s) => s,
        }
    }
}

impl fmt::Display for Encoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_ref())
    }
}

/// Metadata about this request.
#[derive(Debug, Clone)]
pub struct RequestInfo {
    id: RequestId,
    url: Url,
    method: Method,
}

impl RequestInfo {
    /// Obtain the URL of the request.
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// Obtain the HTTP method of the request.
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// Obtain the request ID
    ///
    /// The ID is automatically assigned and uniquely identifies the requests
    /// in this process.
    pub fn id(&self) -> RequestId {
        self.id
    }
}

/// A subset of the `Request` builder. Preserved in curl types.
/// Expose the request in curl handler callback context.
#[derive(Clone, Debug)]
pub struct RequestContext {
    pub(crate) info: RequestInfo,
    pub(crate) body: Option<Vec<u8>>,
    pub(crate) event_listeners: RequestEventListeners,
}

/// Identity of a request.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct RequestId(usize);

/// A builder struct for HTTP requests, designed to be
/// a more egonomic API for setting up a curl handle.
#[derive(Clone, Debug)]
pub struct Request {
    ctx: RequestContext,
    headers: HashMap<String, String>,
    cert: Option<PathBuf>,
    key: Option<PathBuf>,
    cainfo: Option<PathBuf>,
    timeout: Option<Duration>,
    http_version: HttpVersion,
    accept_encoding: Vec<Encoding>,
    min_transfer_speed: Option<MinTransferSpeed>,
    verify_tls_host: bool,
    verify_tls_cert: bool,
    verbose: bool,
    convert_cert: bool,
    auth_proxy_socket_path: Option<String>,
}

static REQUEST_CREATION_LISTENERS: Lazy<RwLock<RequestCreationEventListeners>> =
    Lazy::new(Default::default);

impl RequestContext {
    /// Create a [`RequestContext`].
    pub fn new(url: Url, method: Method) -> Self {
        static ID: AtomicUsize = AtomicUsize::new(0);
        let id = RequestId(ID.fetch_add(1, AcqRel));
        Self {
            info: RequestInfo { id, url, method },
            body: None,
            event_listeners: Default::default(),
        }
    }

    /// Obtain the HTTP url of the request.
    pub fn url(&self) -> &Url {
        &self.info.url()
    }

    /// Obtain the HTTP method of the request.
    pub fn method(&self) -> &Method {
        &self.info.method()
    }

    /// Obtain the request Id.
    ///
    /// The Id is automatically assigned and uniquely identifies the requests
    /// in this process.
    pub fn id(&self) -> RequestId {
        self.info.id()
    }

    /// Obtain the request metadata.
    pub fn info(&self) -> &RequestInfo {
        &self.info
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
            headers: hashmap! {
                "Expect".to_string() => "".to_string(),
            },
            cert: None,
            key: None,
            cainfo: None,
            timeout: None,
            // Attempt to use HTTP/2 by default. Will fall back to HTTP/1.1
            // if version negotiation with the server fails.
            http_version: HttpVersion::V2,
            accept_encoding: Vec::new(),
            min_transfer_speed: None,
            verify_tls_host: true,
            verify_tls_cert: true,
            verbose: false,
            convert_cert: false,
            auth_proxy_socket_path: None,
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
    pub(crate) fn get(url: Url) -> Self {
        Self::new(url, Method::Get)
    }

    /// Create a HEAD request.
    pub(crate) fn head(url: Url) -> Self {
        Self::new(url, Method::Head)
    }

    /// Create a POST request.
    pub(crate) fn post(url: Url) -> Self {
        Self::new(url, Method::Post)
    }

    /// Create a PUT request.
    pub(crate) fn put(url: Url) -> Self {
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

    /// Specify the content compression formats the client should advertise to
    /// the server. By default, this will be every format supported by libcurl.
    pub fn accept_encoding(mut self, formats: impl IntoIterator<Item = Encoding>) -> Self {
        self.set_accept_encoding(formats);
        self
    }

    /// Specify the content compression formats the client should advertise to
    /// the server. By default, this will be every format supported by libcurl.
    pub fn set_accept_encoding(
        &mut self,
        formats: impl IntoIterator<Item = Encoding>,
    ) -> &mut Self {
        self.accept_encoding = formats.into_iter().collect();
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
        self.headers
            .insert(name.to_string().to_lowercase(), value.to_string());
        self
    }

    pub fn get_header_mut<'a>(&'a mut self, name: impl ToString) -> Option<&'a mut String> {
        self.headers.get_mut(&name.to_string().to_lowercase())
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

    /// Turn on libcurl's verbose output. This will cause libcurl to print lots
    /// of verbose debug messages to stderr. This can be useful when trying to
    /// understand exactly what libcurl is doing under the hood, which can help
    /// to debug low-level protocol issues.
    pub fn verbose(mut self, verbose: bool) -> Self {
        self.set_verbose(verbose);
        self
    }

    pub fn set_client_info(&mut self, client_info: &Option<String>) -> &mut Self {
        if let Some(info) = client_info {
            self.set_header("X-Client-Info", info);
        }
        self
    }

    /// Turn on libcurl's verbose output. This will cause libcurl to print lots
    /// of verbose debug messages to stderr. This can be useful when trying to
    /// understand exactly what libcurl is doing under the hood, which can help
    /// to debug low-level protocol issues.
    pub fn set_verbose(&mut self, verbose: bool) -> &mut Self {
        self.verbose = verbose;
        self
    }

    pub fn set_auth_proxy_socket_path(
        &mut self,
        auth_proxy_socket_path: Option<String>,
    ) -> &mut Self {
        self.auth_proxy_socket_path = auth_proxy_socket_path;
        self
    }

    /// Convert the client's X.509 certificate from a PEM file into an in-memory
    /// PKCS#12 archive before passing it to libcurl. This is necessary on some
    /// platforms (most notably Windows) where the system crypto APIs (SChannel
    /// in the case of Windows) do not support loading PEM files.
    pub fn convert_cert(mut self, convert: bool) -> Self {
        self.set_convert_cert(convert);
        self
    }

    /// Convert the client's X.509 certificate from a PEM file into an in-memory
    /// PKCS#12 archive before passing it to libcurl. This is necessary on some
    /// platforms (most notably Windows) where the system crypto APIs (SChannel
    /// in the case of Windows) do not support loading PEM files.
    pub fn set_convert_cert(&mut self, convert: bool) -> &mut Self {
        self.convert_cert = convert;
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
        let res = easy.perform();
        let ctx = easy.get_mut().request_context_mut();
        let info = ctx.info().clone();

        match res {
            Ok(()) => {
                ctx.event_listeners().trigger_success(&info);
            }
            Err(e) => {
                ctx.event_listeners().trigger_failure(&info);
                return Err(e.into());
            }
        }

        Response::try_from(easy.get_mut())
    }

    /// Execute this request asynchronously.
    pub async fn send_async(self) -> Result<AsyncResponse, HttpClientError> {
        let request_info = self.ctx().info().clone();
        let (receiver, streams) = ChannelReceiver::new();
        let request = self.into_streaming(receiver);

        // Spawn the request as another task, which will block
        // the worker it is scheduled on until completion.
        let io_task = tokio::task::spawn_blocking(move || request.send());

        match AsyncResponse::new(streams, request_info).await {
            Ok(res) => Ok(res),
            // If the request was dropped before completion, this likely means
            // that configuring or sending the request failed. The IO task will
            // likely return a more meaningful error message, so return that
            // instead of a generic "this request was dropped" error.
            e @ Err(HttpClientError::RequestDropped(_)) => io_task.await?.and(e),
            Err(e) => Err(e),
        }
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
        let mut url = self.ctx.url().clone();
        if self.auth_proxy_socket_path.is_some() {
            url.set_scheme("http")
                .expect("Failed setting url scheme to http");
            self.set_verify_tls_cert(false)
                .set_verify_tls_host(false)
                .set_convert_cert(false);

            if let Some(user_agent) = self.get_header_mut("user-agent") {
                user_agent.push_str("+x2pagentd");
            }
        }
        let handler = create_handler(self.ctx);

        let mut easy = Easy2::new(handler);

        easy.url(url.as_str())?;
        easy.verbose(self.verbose)?;
        easy.unix_socket_path(self.auth_proxy_socket_path)?;

        // Configure the handle for the desired HTTP method.
        match easy.get_ref().request_context().method() {
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

        if !self.accept_encoding.is_empty() {
            // To maintain compatibility with libcurl, if the Accept-Encoding is explicitly set to
            // the empty string, advertise all formats the client supports.
            if self.accept_encoding.len() == 1
                && self.accept_encoding[0] == Encoding::Other("".into())
            {
                self.accept_encoding = Encoding::all()
            }

            let encoding = self
                .accept_encoding
                .iter()
                .map(|s| s.as_ref())
                .collect::<Vec<_>>()
                .join(", ");

            // XXX: Ideally, we should set the Accept-Encoding via the accept_encoding() method
            // (which corresponds to CURLOPT_ACCEPT_ENCODING). This will cause libcurl to decode
            // the response body automatically if the received Content-Encoding matches one of the
            // requested formats.
            //
            // Unfortunately, although libcurl can be built to support many compression formats,
            // the Rust bindings configure it so that only a few formats (e.g., gzip and deflate)
            // are supported. To work around this, right now we just set the Accept-Encoding header
            // as a regular header (without setting CURLOPT_ACCEPT_ENCODING) and decode the response
            // manually. This allows us to ensure support for formats we care about (e.g., zstd).
            self.headers
                .insert(header::ACCEPT_ENCODING.as_str().into(), encoding);
        }

        // Add headers.
        let mut headers = List::new();
        for (name, value) in self.headers.iter() {
            let header = format!("{}: {}", name, value);
            headers.append(&header)?;
        }
        easy.http_headers(headers)?;

        // Configure TLS verification.
        easy.ssl_verify_host(self.verify_tls_host)?;
        easy.ssl_verify_peer(self.verify_tls_cert)?;

        match &self.cert {
            Some(cert) if self.convert_cert => {
                // Convert certificate to PKCS#12 format for platforms that do
                // not support loading PEM files (notably Windows).
                tracing::debug!("Converting certificate {:?} to PKCS#12 format", cert);
                let blob = pem_to_pkcs12(cert, self.key)?;
                easy.ssl_cert_type("P12")?;
                easy.ssl_cert_blob(&blob)?;
            }
            Some(cert) => {
                easy.ssl_cert(cert)?;
                if let Some(key) = &self.key {
                    easy.ssl_key(key)?;
                }
            }
            None => {}
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

fn read_file(path: impl AsRef<Path>) -> Result<Vec<u8>, anyhow::Error> {
    let mut f = File::open(path)?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    Ok(buf)
}

#[derive(Eq, Hash, PartialEq)]
struct PemCacheKey {
    pub cert: PathBuf,
    pub key: Option<PathBuf>,
    pub cert_mtime: SystemTime,
    pub key_mtime: Option<SystemTime>,
}

static PEM_CONVERT_CACHE: Lazy<Mutex<LruCache<PemCacheKey, Vec<u8>>>> =
    Lazy::new(|| Mutex::new(LruCache::new(10)));

/// Convert a PEM-formatted X.509 certificate chain and private key into a
/// PKCS#12 archive, which can then be directly passed to libcurl using
/// `CURLOPT_SSLCERT_BLOB`. This is useful because not all TLS engines (notably
/// SChannel (WinSSL) on Windows) support loading PEM files, but all major TLS
/// engines support PKCS#12. Returns a DER-encoded binary representation of
/// the combined certificate chain and private key.
fn pem_to_pkcs12(
    cert: impl AsRef<Path>,
    key: Option<impl AsRef<Path>>,
) -> Result<Vec<u8>, anyhow::Error> {
    let mut cache = PEM_CONVERT_CACHE.lock();
    let cert_mtime = cert.as_ref().metadata()?.modified()?;
    let key_mtime = match &key {
        Some(key) => Some(key.as_ref().metadata()?.modified()?),
        None => None,
    };
    let cache_key = PemCacheKey {
        cert: cert.as_ref().to_owned(),
        key: key.as_ref().map(|k| k.as_ref().to_owned()),
        cert_mtime,
        key_mtime,
    };
    if let Some(data) = cache.get_mut(&cache_key) {
        return Ok(data.clone());
    }

    // It's common for the certificate and private key to be concatenated
    // together in the same PEM file. If a key path isn't specified, assume
    // this is the case and use the certificate PEM for the key as well.
    let cert_bytes = read_file(cert)?;
    let key_bytes = match key {
        Some(key) => Cow::Owned(read_file(key)?),
        None => Cow::Borrowed(&cert_bytes),
    };

    let cert = X509::from_pem(&cert_bytes)?;
    let key = PKey::private_key_from_pem(&key_bytes)?;

    // PKCS#12 archives are encrypted, so we need to specify a password when
    // creating one. Here we just use an empty password since it seems like most
    // TLS engines will attempt to decrypt using the empty string if no password
    // is specified.
    let pkcs12 = Pkcs12::builder().build("", "", &key, &cert)?;

    let result = pkcs12.to_der()?;
    cache.insert(cache_key, result.clone());

    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering::Acquire;
    use std::sync::Arc;

    use anyhow::Result;
    use futures::TryStreamExt;
    use http::header;
    use http::header::HeaderName;
    use http::header::HeaderValue;
    use http::StatusCode;
    use mockito::mock;
    use mockito::Matcher;
    use serde_json::json;

    use super::*;
    use crate::Config;
    use crate::HttpClient;

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

        assert_eq!(res.head.status, StatusCode::OK);
        assert_eq!(&*res.body, &b"Hello, world!"[..]);
        assert_eq!(
            res.head.headers.get(header::CONTENT_TYPE).unwrap(),
            HeaderValue::from_static("text/plain")
        );
        assert_eq!(
            res.head
                .headers
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

        assert_eq!(res.head.status, StatusCode::OK);
        assert_eq!(
            res.head.headers.get(header::CONTENT_TYPE).unwrap(),
            HeaderValue::from_static("text/plain")
        );
        assert_eq!(
            res.head
                .headers
                .get(HeaderName::from_bytes(b"X-Served-By")?)
                .unwrap(),
            HeaderValue::from_static("mock")
        );

        let body = res.into_body().raw().try_concat().await?;
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

        assert_eq!(res.head.status, StatusCode::OK);
        assert!(res.body.is_empty());
        assert_eq!(
            res.head.headers.get(header::CONTENT_TYPE).unwrap(),
            HeaderValue::from_static("text/plain")
        );
        assert_eq!(
            res.head
                .headers
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
        assert_eq!(res.head.status, StatusCode::CREATED);

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
        assert_eq!(res.head.status, StatusCode::CREATED);

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
        assert_eq!(res.head.status, StatusCode::CREATED);

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
        assert_eq!(res.head.status, StatusCode::CREATED);

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
        assert_eq!(res.head.status, StatusCode::CREATED);

        Ok(())
    }

    #[test]
    fn test_accept_encoding() -> Result<()> {
        let mock = mock("GET", "/test")
            .with_status(200)
            .match_header("Accept-Encoding", "zstd, gzip, foobar")
            .create();

        let encodings = vec![
            Encoding::Zstd,
            Encoding::Gzip,
            Encoding::Other("foobar".into()),
        ];

        let url = Url::parse(&mockito::server_url())?.join("test")?;
        let _ = Request::get(url).accept_encoding(encodings).send()?;

        mock.assert();
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

    #[test]
    fn test_convert_cert_flag() -> Result<()> {
        let client = HttpClient::new();
        let url: Url = "https://example.com".parse()?;

        // Make sure convert_cert defaults to cfg!(windows) and gets
        // passed along to request.
        assert_eq!(cfg!(windows), client.get(url.clone()).convert_cert);

        let client = HttpClient::from_config(Config {
            convert_cert: true,
            ..Default::default()
        });
        assert!(client.get(url).convert_cert);

        Ok(())
    }
}
