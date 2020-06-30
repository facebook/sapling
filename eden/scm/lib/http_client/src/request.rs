/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;

use curl::{
    self,
    easy::{Easy2, HttpVersion, List},
};
use serde::Serialize;
use url::Url;

use crate::{
    errors::{CertOrKeyMissing, HttpClientError},
    handler::{Buffered, Configure},
    response::Response,
};

#[derive(Copy, Clone, Debug)]
enum Method {
    Get,
    Head,
    Post,
    Put,
}

/// A builder struct for HTTP requests, designed to be
/// a more egonomic API for setting up a curl handle.
#[derive(Debug)]
pub struct Request<'a> {
    url: &'a Url,
    method: Method,
    headers: Vec<(&'a str, &'a str)>,
    body: Option<Vec<u8>>,
    creds: Option<(&'a Path, &'a Path)>,
    cainfo: Option<&'a Path>,
}

impl<'a> Request<'a> {
    fn new(url: &'a Url, method: Method) -> Self {
        Self {
            url,
            method,
            headers: Vec::new(),
            body: None,
            creds: None,
            cainfo: None,
        }
    }

    /// Create a GET request.
    pub fn get(url: &'a Url) -> Self {
        Self::new(url, Method::Get)
    }

    /// Create a HEAD request.
    pub fn head(url: &'a Url) -> Self {
        Self::new(url, Method::Head)
    }

    /// Create a POST request.
    pub fn post(url: &'a Url) -> Self {
        Self::new(url, Method::Post)
    }

    /// Create a PUT request.
    pub fn put(url: &'a Url) -> Self {
        Self::new(url, Method::Put)
    }

    /// Set the data to be uploaded in the request body.
    pub fn body<B: Into<Vec<u8>>>(self, data: B) -> Self {
        Self {
            body: Some(data.into()),
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
    pub fn header(mut self, name: &'a str, value: &'a str) -> Self {
        self.headers.push((name, value));
        self
    }

    /// Specify client credentials for mTLS. The arguments should be
    /// paths to a PEM-encoded X.509 client certificate chain and the
    /// corresponding private key. (It is possible for the certificate
    /// and private key to be in the same file.)
    pub fn creds<C, K>(self, cert: &'a C, key: &'a K) -> Result<Self, CertOrKeyMissing>
    where
        C: AsRef<Path>,
        K: AsRef<Path>,
    {
        let cert = cert.as_ref();
        if !cert.is_file() {
            return Err(CertOrKeyMissing(cert.into()));
        }
        let key = key.as_ref();
        if !key.is_file() {
            return Err(CertOrKeyMissing(key.into()));
        }

        Ok(Self {
            creds: Some((cert, key)),
            ..self
        })
    }

    /// Specify a CA certificate bundle to be used to verify the
    /// server's certificate. If not specified, the client will
    /// use the system default CA certificate bundle.
    pub fn cainfo<C: AsRef<Path>>(self, cainfo: &'a C) -> Result<Self, CertOrKeyMissing> {
        let cainfo = cainfo.as_ref();
        if !cainfo.is_file() {
            return Err(CertOrKeyMissing(cainfo.into()));
        }

        Ok(Self {
            cainfo: Some(cainfo),
            ..self
        })
    }

    /// Execute the request, blocking until completion.
    ///
    /// This method is intended as a simple way to perform
    /// one-off HTTP requests. `HttpClient::send` should be
    /// used instead of this method when working with many
    /// concurrent requests or large requests that require
    /// progress reporting.
    pub fn send(self) -> Result<Response, HttpClientError> {
        let easy = self.into_handle()?;
        easy.perform()?;
        Ok(Response::from_handle(easy)?)
    }

    /// Turn this Request into a `curl::Easy2` handle.
    pub(crate) fn into_handle(self) -> Result<Easy2<Buffered>, HttpClientError> {
        let body_size = self.body.as_ref().map(|body| body.len() as u64);
        let handler = Buffered::new().with_payload(self.body);

        let mut easy = Easy2::new(handler);
        easy.url(self.url.as_str())?;

        // Configure the handle for the desired HTTP method.
        match self.method {
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
        if let Some((cert, key)) = self.creds {
            easy.ssl_cert(cert)?;
            easy.ssl_key(key)?;
        }
        if let Some(cainfo) = self.cainfo {
            easy.cainfo(cainfo)?;
        }

        // Always use attempt to use HTTP/2. Will fall back to HTTP/1.1
        // if version negotiation with the server fails.
        easy.http_version(HttpVersion::V2)?;

        // Tell libcurl to report progress to the handler.
        easy.progress(true)?;

        Ok(easy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs::File;

    use anyhow::Result;
    use http::{
        header::{self, HeaderName, HeaderValue},
        StatusCode,
    };
    use mockito::{mock, Matcher};
    use serde_json::json;
    use tempdir::TempDir;

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
        let res = Request::get(&url).header("X-Api-Key", "1234").send()?;

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

    #[test]
    fn test_head() -> Result<()> {
        let mock = mock("HEAD", "/test")
            .with_status(200)
            .match_header("X-Api-Key", "1234")
            .with_header("Content-Type", "text/plain")
            .with_header("X-Served-By", "mock")
            .create();

        let url = Url::parse(&mockito::server_url())?.join("test")?;
        let res = Request::head(&url).header("X-Api-Key", "1234").send()?;

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
        let res = Request::post(&url).body(body.as_bytes()).send()?;

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
        let res = Request::put(&url)
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
        let res = Request::post(&url).json(&body)?.send()?;

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
        };

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
        let res = Request::post(&url).cbor(&body)?.send()?;

        mock.assert();
        assert_eq!(res.status, StatusCode::CREATED);

        Ok(())
    }

    #[test]
    fn test_creds_exist() -> Result<()> {
        let tmp = TempDir::new("test_creds_exist")?;
        let cert = tmp.path().to_path_buf().join("cert.pem");
        let key = tmp.path().to_path_buf().join("key.pem");
        let cainfo = tmp.path().to_path_buf().join("cainfo.pem");
        let url = Url::parse("https://example.com")?;

        // Cert and key missing.
        assert!(Request::get(&url).creds(&cert, &key).is_err());

        // Just key missing.
        let _ = File::create(&cert)?;
        assert!(Request::get(&url).creds(&cert, &key).is_err());

        // Both present.
        let _ = File::create(&key)?;
        let _ = Request::get(&url).creds(&cert, &key)?;

        // CA cert bundle missing.
        assert!(Request::get(&url).cainfo(&cainfo).is_err());

        // CA cert bundle present.
        let _ = File::create(&cainfo)?;
        let _ = Request::get(&url).cainfo(&cainfo)?;

        Ok(())
    }
}
