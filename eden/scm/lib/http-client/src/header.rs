/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::str;

use http::header::HeaderName;
use http::header::HeaderValue;
use http::status::StatusCode;
use http::version::Version;
use once_cell::sync::Lazy;
use regex::Regex;
use thiserror::Error;

static STATUS_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)HTTP/([0-9.]+) ([0-9]+)").unwrap());

/// A parsed header line.
///
/// This enum represents a line from the header section of a
/// response. Note that in addition to the headers, libcurl
/// also passes the initial status line and trailing blank
/// line to the user-specified header callback, so we must
/// be able to handle those cases.
#[derive(Eq, PartialEq, Debug)]
pub enum Header {
    Status(Version, StatusCode),
    Header(HeaderName, HeaderValue),
    EndOfHeaders,
}

#[derive(Debug, Error)]
#[error("Malformed header: {:?}", String::from_utf8_lossy(.0))]
pub struct BadHeader<'a>(&'a [u8]);

impl Header {
    /// Parse a header line. The input is expected to be a CRLF-terminated
    /// line which can be decoded as UTF-8. Note that per RFC 7230, header
    /// values can sometimes contain arbitrary binary data, but in practice
    /// they are limited to ASCII characters, so for simplicity we reject
    /// non-UTF-8 header values. Aside from the values, the specification
    /// restricts all other parts of a header line to be limited to ASCII.
    pub fn parse(line: &[u8]) -> Result<Self, BadHeader<'_>> {
        let header = str::from_utf8(line)
            .map_err(|_| BadHeader(line))?
            .trim_end(); // Strip off trailing CRLF.

        if header.is_empty() {
            return Ok(Self::EndOfHeaders);
        }
        if let Some((name, value)) = parse_header(header) {
            return Ok(Self::Header(name, value));
        }
        if let Some((version, code)) = parse_status(header) {
            return Ok(Self::Status(version, code));
        }

        Err(BadHeader(line))
    }
}

/// Parse a status line, e.g. "HTTP/1.1 200 OK".
fn parse_status(line: &str) -> Option<(Version, StatusCode)> {
    let captures = STATUS_REGEX.captures(line)?;

    let version_str = captures.get(1).map(|m| m.as_str())?;
    let version = parse_version(version_str)?;

    let code_str = captures.get(2).map(|m| m.as_str())?;
    let code = StatusCode::from_u16(code_str.parse().ok()?).ok()?;

    Some((version, code))
}

/// Parse an HTTP version number.
fn parse_version(version: &str) -> Option<Version> {
    Some(match version {
        "0.9" => Version::HTTP_09,
        "1.0" => Version::HTTP_10,
        "1.1" => Version::HTTP_11,
        "2" | "2.0" => Version::HTTP_2,
        "3" | "3.0 " => Version::HTTP_3,
        _ => return None,
    })
}

/// Parse a header name-value pair, e.g. "Content-Length: 42\r\n".
fn parse_header(header: &str) -> Option<(HeaderName, HeaderValue)> {
    let parts = header.splitn(2, ':').collect::<Vec<_>>();
    let (name, value) = if parts.len() > 1 {
        (parts[0], parts[1].trim_start())
    } else {
        (parts[0], "")
    };

    let name = HeaderName::from_bytes(name.as_bytes()).ok()?;
    let value = HeaderValue::from_bytes(value.as_bytes()).ok()?;

    Some((name, value))
}

#[cfg(test)]
mod test {
    use anyhow::Result;
    use http::header;

    use super::*;

    #[test]
    fn test_parse_header() -> Result<()> {
        let header = Header::parse(b"Content-Length: 42\r\n")?;
        let expected = Header::Header(header::CONTENT_LENGTH, HeaderValue::from_static("42"));
        assert_eq!(header, expected);

        let header = Header::parse(b"X-Non-Standard: test\r\n")?;
        let expected = Header::Header(
            HeaderName::from_static("x-non-standard"),
            HeaderValue::from_static("test"),
        );
        assert_eq!(header, expected);

        let header = Header::parse(b"X-No-Value\r\n")?;
        let expected = Header::Header(
            HeaderName::from_static("x-no-value"),
            HeaderValue::from_static(""),
        );
        assert_eq!(header, expected);

        let header = Header::parse(b"X-Whitespace:  hello  world  \r\n")?;
        let expected = Header::Header(
            HeaderName::from_static("x-whitespace"),
            HeaderValue::from_static("hello  world"),
        );
        assert_eq!(header, expected);

        let header = Header::parse("X-Non-ASCII-Value: \u{1F980}\r\n".as_ref())?;
        let expected = Header::Header(
            HeaderName::from_static("x-non-ascii-value"),
            HeaderValue::from_bytes("\u{1F980}".as_ref())?,
        );
        assert_eq!(header, expected);

        assert!(Header::parse("\u{1F980}: Non-ASCII name\r\n".as_ref()).is_err());
        Ok(())
    }

    #[test]
    fn test_parse_status() -> Result<()> {
        let status = Header::parse(b"HTTP/2 201 CREATED\r\n")?;
        let expected = Header::Status(Version::HTTP_2, StatusCode::CREATED);
        assert_eq!(status, expected);
        Ok(())
    }

    #[test]
    fn test_parse_crlf() -> Result<()> {
        assert_eq!(Header::parse(b"\r\n")?, Header::EndOfHeaders);
        Ok(())
    }
}
