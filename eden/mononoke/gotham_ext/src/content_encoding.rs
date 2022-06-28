/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use gotham::state::FromState;
use gotham::state::State;
use http::header::HeaderMap;
use http::header::HeaderValue;
use http::header::ACCEPT_ENCODING;

const GZIP: &str = "gzip";
const ZSTD: &str = "zstd";

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContentCompression {
    Gzip,
    Zstd,
}

impl ContentCompression {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Gzip => GZIP,
            Self::Zstd => ZSTD,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum ContentEncoding {
    Compressed(ContentCompression),
    Identity,
}

impl ContentEncoding {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Compressed(c) => c.as_str(),
            Self::Identity => "identity",
        }
    }
}

impl Into<HeaderValue> for ContentEncoding {
    fn into(self) -> HeaderValue {
        HeaderValue::from_static(self.as_str())
    }
}

impl ContentEncoding {
    pub fn from_state(state: &State) -> Self {
        let header = HeaderMap::try_borrow_from(state).and_then(|h| h.get(ACCEPT_ENCODING));

        match header {
            Some(h) => Self::from_header(h.as_bytes()).unwrap_or(Self::Identity),
            None => Self::Identity,
        }
    }

    /// Parse an [Accept-Encoding header] and provide a ContentEncoding
    /// representing it. This ignores client preferences entirely, since we
    /// control client & server, and instead uses our own preference ordering,
    /// restricted to what the client allows.
    ///
    /// [Accept-Encoding header]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Accept-Encoding
    fn from_header(header: &[u8]) -> Result<Self, Error> {
        let mut gzip = false;
        let mut zstd = false;

        let encodings = std::str::from_utf8(header)?.split(' ');

        for encoding in encodings {
            let encoding = match encoding.split(';').next() {
                Some(encoding) => encoding.trim(),
                None => continue,
            };

            if encoding == ZSTD {
                zstd = true;
            }

            if encoding == GZIP {
                gzip = true;
            }
        }

        if zstd {
            return Ok(Self::Compressed(ContentCompression::Zstd));
        }

        if gzip {
            return Ok(Self::Compressed(ContentCompression::Gzip));
        }

        Ok(Self::Identity)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_decode_content_encoding() -> Result<(), Error> {
        assert_eq!(
            ContentEncoding::from_header(b"identity, foobar")?,
            ContentEncoding::Identity
        );

        assert_eq!(
            ContentEncoding::from_header(b"foobar")?,
            ContentEncoding::Identity
        );

        assert_eq!(
            ContentEncoding::from_header(b"foobar, identity, zstd")?,
            ContentEncoding::Compressed(ContentCompression::Zstd),
        );

        assert_eq!(
            ContentEncoding::from_header(b"gzip, zstd")?,
            ContentEncoding::Compressed(ContentCompression::Zstd),
        );

        assert_eq!(
            ContentEncoding::from_header(b"deflate, gzip;q=1.0, *;q=0.5")?,
            ContentEncoding::Compressed(ContentCompression::Gzip),
        );

        assert_eq!(
            ContentEncoding::from_header(b"")?,
            ContentEncoding::Identity
        );

        Ok(())
    }
}
