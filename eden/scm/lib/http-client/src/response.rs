/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Cursor;
use std::pin::Pin;

use anyhow::anyhow;
use async_compression::tokio::bufread::BrotliDecoder;
use async_compression::tokio::bufread::DeflateDecoder;
use async_compression::tokio::bufread::GzipDecoder;
use async_compression::tokio::bufread::ZstdDecoder;
use futures::prelude::*;
use http::header;
use http::header::HeaderMap;
use http::status::StatusCode;
use http::version::Version;
use serde::de::DeserializeOwned;
use tokio::io::BufReader;
use tokio_util::io::ReaderStream;
use tokio_util::io::StreamReader;
use url::Url;

use crate::errors::HttpClientError;
use crate::handler::Buffered;
use crate::handler::HandlerExt;
use crate::header::Header;
use crate::receiver::ResponseStreams;
use crate::request::Encoding;
use crate::request::RequestInfo;
use crate::stream::BufferedStream;
use crate::stream::CborStream;

#[derive(Debug)]
pub struct Head {
    pub(crate) version: Version,
    pub(crate) status: StatusCode,
    pub(crate) headers: HeaderMap,
    pub(crate) request_info: RequestInfo,
}

impl Head {
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

    /// Get metadata about the response's corresponding HTTP request.
    pub fn request_info(&self) -> &RequestInfo {
        &self.request_info
    }

    /// Get the response's encoding from the Content-Encoding header.
    pub fn encoding(&self) -> Result<Encoding, HttpClientError> {
        self.headers
            .get(header::CONTENT_ENCODING)
            .map(|encoding| Ok(encoding.to_str()?.into()))
            .unwrap_or(Ok(Encoding::Identity))
            .map_err(|_: header::ToStrError| {
                HttpClientError::BadResponse(anyhow!("Invalid Content-Encoding"))
            })
    }
}

#[derive(Debug)]
pub struct Response {
    pub(crate) head: Head,
    pub(crate) body: Vec<u8>,
}

impl Response {
    /// Get the HTTP version of the response.
    pub fn version(&self) -> Version {
        self.head.version
    }

    /// Get the HTTP status code of the response.
    pub fn status(&self) -> StatusCode {
        self.head.status
    }

    /// Get the response's headers.
    pub fn headers(&self) -> &HeaderMap {
        &self.head.headers
    }

    /// Get the response's body.
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// Split the
    pub fn into_parts(self) -> (Head, Vec<u8>) {
        (self.head, self.body)
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
            head: Head {
                version,
                status,
                headers: buffered.take_headers(),
                request_info: buffered.request_context().info.clone(),
            },
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
            .map_err(|e| {
                if e.get_ref()
                    .map(|i| i.is::<HttpClientError>())
                    .unwrap_or(false)
                {
                    *e.into_inner()
                        .unwrap()
                        .downcast::<HttpClientError>()
                        .unwrap()
                } else {
                    HttpClientError::DecompressionFailed(e)
                }
            })
            .boxed()
    }};
}

pub type ByteStream =
    Pin<Box<dyn Stream<Item = Result<Vec<u8>, HttpClientError>> + Send + 'static>>;

pub struct AsyncBody {
    // This is a `Result` so that an invalid Content-Encoding header does not prevent the caller
    // from accessing the raw body stream if desired. The error will only be propagated if the
    // caller actually wants to decode the body stream.
    encoding: Result<Encoding, HttpClientError>,
    body: ByteStream,
}

pub type CborStreamBody<T> = CborStream<T, ByteStream, Vec<u8>, HttpClientError>;
pub type BufferedStreamBody = BufferedStream<ByteStream, Vec<u8>, HttpClientError>;

impl AsyncBody {
    /// Get a stream of the response's body content.
    ///
    /// This method is the preferred way of accessing the response's body stream. The data will be
    /// automatically decoded based on the response's Content-Encoding header.
    pub fn decoded(self) -> ByteStream {
        let Self { encoding, body } = self;
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

    /// Get a stream of the response's raw on-the-wire content.
    ///
    /// Note that the caller is responsible for decoding the response if it is compressed. Most
    /// callers will want to use the `decoded` method instead which does this automatically.
    pub fn raw(self) -> ByteStream {
        self.body
    }

    /// Attempt to deserialize the incoming data as a stream of CBOR values.
    pub fn cbor<T: DeserializeOwned>(self) -> CborStreamBody<T> {
        CborStream::new(self.decoded())
    }

    /// Create a buffered body stream that ensures that all yielded chunks
    /// (except the last) are at least as large as the given chunk size.
    pub fn buffered(self, size: usize) -> BufferedStreamBody {
        BufferedStream::new(self.decoded(), size)
    }
}

pub struct AsyncResponse {
    pub(crate) head: Head,
    pub(crate) body: AsyncBody,
}

impl AsyncResponse {
    pub(crate) async fn new(
        streams: ResponseStreams,
        request_info: RequestInfo,
    ) -> Result<Self, HttpClientError> {
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

        let head = Head {
            version,
            status,
            headers,
            request_info,
        };
        let encoding = head.encoding();
        let body = AsyncBody { encoding, body };

        Ok(Self { head, body })
    }

    /// Get the HTTP version of the response.
    pub fn version(&self) -> Version {
        self.head.version
    }

    /// Get the HTTP status code of the response.
    pub fn status(&self) -> StatusCode {
        self.head.status
    }

    /// Get the response's headers.
    pub fn headers(&self) -> &HeaderMap {
        &self.head.headers
    }

    /// Consume the response and obtain its body stream.
    pub fn into_body(self) -> AsyncBody {
        self.body
    }

    /// Split the response into its head and body.
    pub fn into_parts(self) -> (Head, AsyncBody) {
        (self.head, self.body)
    }

    pub fn url(&self) -> &Url {
        self.head.request_info.url()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use anyhow::Result;
    use futures::TryStreamExt;
    use mockito::mock;
    use url::Url;

    use crate::request::Encoding;
    use crate::request::Request;

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
        let res = Request::get(url)
            .accept_encoding(Encoding::all())
            .send_async()
            .await?;

        mock.assert();

        let body = res.into_body().decoded().try_concat().await?;
        assert_eq!(&*body, &uncompressed[..]);

        Ok(())
    }
}
