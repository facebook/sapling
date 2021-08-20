/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::TryFrom;
use std::io::Cursor;
use std::mem;
use std::pin::Pin;

use anyhow::anyhow;
use async_compression::tokio::bufread::{BrotliDecoder, DeflateDecoder, GzipDecoder, ZstdDecoder};
use futures::prelude::*;
use http::header::{self, HeaderMap};
use http::status::StatusCode;
use http::version::Version;
use serde::de::DeserializeOwned;
use tokio::io::BufReader;
use tokio_util::io::{ReaderStream, StreamReader};

use crate::errors::HttpClientError;
use crate::handler::Buffered;
use crate::header::Header;
use crate::receiver::ResponseStreams;
use crate::request::Encoding;
use crate::stream::{BufferedStream, CborStream};

#[derive(Debug)]
pub struct Response {
    pub(crate) version: Version,
    pub(crate) status: StatusCode,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Vec<u8>,
}

impl Response {
    /// Get the HTTP version of the response.
    pub fn version(&self) -> Version {
        self.version
    }

    /// Get the HTTP status code of the response.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Get the response's headers.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Get the response's body.
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// Move the response's body out of the response.
    /// Subsequent calls will return an empty body.
    pub fn take_body(&mut self) -> Vec<u8> {
        mem::take(&mut self.body)
    }

    /// Deserialize the response body from JSON.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.body)
    }

    /// Deserialize the response body from CBOR.
    pub fn cbor<T: DeserializeOwned>(&self) -> Result<T, serde_cbor::Error> {
        serde_cbor::from_slice(&self.body)
    }
}

impl TryFrom<&mut Buffered> for Response {
    type Error = HttpClientError;

    fn try_from(buffered: &mut Buffered) -> Result<Self, Self::Error> {
        let (version, status) = match (buffered.version(), buffered.status()) {
            (Some(version), Some(status)) => (version, status),
            _ => {
                return Err(HttpClientError::BadResponse(anyhow!(
                    "HTTP version or status code missing in response"
                )));
            }
        };

        Ok(Self {
            version,
            status,
            headers: buffered.take_headers(),
            body: buffered.take_body(),
        })
    }
}

macro_rules! decode {
    ($decoder:tt, $body_stream:expr) => {{
        let body = $body_stream
            .map_ok(Cursor::new)
            .map_err(|e| futures::io::Error::new(futures::io::ErrorKind::Other, e));
        let reader = BufReader::new(StreamReader::new(body));
        ReaderStream::new($decoder::new(reader))
            .map_ok(|bytes| bytes.to_vec())
            .map_err(HttpClientError::DecompressionFailed)
            .boxed()
    }};
}

pub type AsyncBody = Pin<Box<dyn Stream<Item = Result<Vec<u8>, HttpClientError>> + Send + 'static>>;
pub type CborStreamBody<T> = CborStream<T, AsyncBody, Vec<u8>, HttpClientError>;
pub type BufferedStreamBody = BufferedStream<AsyncBody, Vec<u8>, HttpClientError>;

pub struct AsyncResponse {
    pub(crate) version: Version,
    pub(crate) status: StatusCode,
    pub(crate) headers: HeaderMap,
    pub(crate) body: AsyncBody,
}

impl AsyncResponse {
    pub async fn new(streams: ResponseStreams) -> Result<Self, HttpClientError> {
        let ResponseStreams {
            headers_rx,
            body_rx,
            done_rx,
        } = streams;

        let header_lines = headers_rx
            .take_while(|h| future::ready(h != &Header::EndOfHeaders))
            .collect::<Vec<_>>()
            .await;

        let mut version = None;
        let mut status = None;
        let mut headers = HeaderMap::new();

        for line in header_lines {
            match line {
                Header::Status(v, s) => {
                    version = Some(v);
                    status = Some(s);
                }
                Header::Header(k, v) => {
                    headers.insert(k, v);
                }
                Header::EndOfHeaders => unreachable!(),
            }
        }

        let (version, status) = match (version, status) {
            (Some(v), Some(s)) => (v, s),
            _ => {
                // If we didn't get a status line, we most likely
                // failed to connect to the server at all. In this
                // case, we should expect to receive an error in
                // the `done_rx` stream. Even if not, a response
                // without a status line is invalid so we should
                // fail regardless.
                done_rx.await??;
                return Err(HttpClientError::BadResponse(anyhow!(
                    "HTTP version or status code missing in response"
                )));
            }
        };

        let body = body_rx
            .map(Ok)
            .chain(stream::once(async move {
                done_rx.await??;
                Ok(Vec::new())
            }))
            .try_filter(|chunk| future::ready(!chunk.is_empty()))
            .boxed();

        Ok(Self {
            version,
            status,
            headers,
            body,
        })
    }

    /// Get the HTTP version of the response.
    pub fn version(&self) -> Version {
        self.version
    }

    /// Get the HTTP status code of the response.
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Get the response's headers.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Get the response's raw body stream, consisting of the raw bytes from
    /// the wire. The data may be compresssed, depending on the value of the
    /// `Content-Encoding` header.
    pub fn raw_body(&mut self) -> AsyncBody {
        mem::replace(&mut self.body, stream::empty().boxed())
    }

    /// Get the response's body stream. This will move the body stream out of
    /// the response; subsequent calls will return an empty body stream.
    pub fn body(&mut self) -> AsyncBody {
        let body = self.raw_body();
        let encoding = self
            .headers
            .get(header::CONTENT_ENCODING)
            .map(|encoding| Ok(encoding.to_str()?.into()))
            .unwrap_or(Ok(Encoding::Identity))
            .map_err(|_: header::ToStrError| {
                HttpClientError::BadResponse(anyhow!("Invalid Content-Encoding"))
            });

        stream::once(async move {
            Ok(match encoding? {
                Encoding::Identity => body.boxed(),
                Encoding::Brotli => decode!(BrotliDecoder, body),
                Encoding::Deflate => decode!(DeflateDecoder, body),
                Encoding::Gzip => decode!(GzipDecoder, body),
                Encoding::Zstd => decode!(ZstdDecoder, body),
                other => {
                    return Err(HttpClientError::BadResponse(anyhow!(
                        "Unsupported Content-Encoding: {:?}",
                        other
                    )));
                }
            })
        })
        .try_flatten()
        .boxed()
    }

    /// Attempt to deserialize the incoming data as a stream of CBOR values.
    pub fn cbor<T: DeserializeOwned>(&mut self) -> CborStreamBody<T> {
        CborStream::new(self.body())
    }

    /// Create a buffered body stream that ensures that all yielded chunks
    /// (except the last) are at least as large as the given chunk size.
    pub fn buffered(&mut self, size: usize) -> BufferedStreamBody {
        BufferedStream::new(self.body(), size)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use anyhow::Result;
    use futures::TryStreamExt;
    use mockito::mock;
    use url::Url;

    use crate::request::{Encoding, Request};

    #[tokio::test]
    async fn test_decompression() -> Result<()> {
        let uncompressed = b"Hello, world!";
        let compressed = zstd::encode_all(Cursor::new(uncompressed), 0)?;

        let mock = mock("GET", "/test")
            .match_header("Accept-Encoding", "zstd, br, gzip, deflate")
            .with_status(200)
            .with_header("Content-Encoding", "zstd")
            .with_body(compressed)
            .create();

        let url = Url::parse(&mockito::server_url())?.join("test")?;
        let mut res = Request::get(url)
            .accept_encoding(Encoding::all())
            .send_async()
            .await?;

        mock.assert();

        let body = res.body().try_concat().await?;
        assert_eq!(&*body, &uncompressed[..]);

        Ok(())
    }
}
