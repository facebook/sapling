/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Bounded request/response frames for a persistent, single-repository importer.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

/// The protocol version understood by both the importer and its parent.
pub const PROTOCOL_VERSION: u32 = 1;
/// Maximum encoded frame size, including the response prefix and final newline.
pub const MAX_FRAME_BYTES: usize = 128 * 1024;
/// Distinguishes completion responses from bounded native timing lines.
pub const RESPONSE_PREFIX: &str = "gitimport_session ";

/// Work explicitly authorized by one request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    /// Initialize scoped repo clients without importing or publishing anything.
    Warm,
    /// Import the exact supplied ref snapshot and reconcile those bookmarks.
    Import,
}

/// Successful completion of an operation. Failures terminate the helper.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ready,
    Completed,
}

/// One request, with a caller-chosen identifier and exact repository/ref scope.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub request_id: String,
    pub repo_name: String,
    pub operation: Operation,
    pub refs: BTreeMap<String, String>,
}

/// A response echoes the complete accepted request; markers alone are not success.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub version: u32,
    pub request_id: String,
    pub repo_name: String,
    pub operation: Operation,
    pub refs: BTreeMap<String, String>,
    pub status: Status,
}

/// Sanitized protocol failures, without input or parser diagnostics.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum ProtocolError {
    #[error("invalid session frame")]
    InvalidFrame,
    #[error("unsupported session version")]
    InvalidVersion,
    #[error("invalid session request identifier")]
    InvalidRequestId,
    #[error("invalid session repository")]
    InvalidRepoName,
    #[error("invalid session refs")]
    InvalidRefs,
    #[error("invalid session response status")]
    InvalidStatus,
}

impl Request {
    /// Validate the version, identifier and concrete ref snapshot.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.version != PROTOCOL_VERSION {
            return Err(ProtocolError::InvalidVersion);
        }
        if self.request_id.is_empty()
            || self.request_id.len() > 128
            || !self
                .request_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(ProtocolError::InvalidRequestId);
        }
        if self.repo_name.is_empty()
            || self.repo_name.len() > 255
            || !self
                .repo_name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-/".contains(&b))
            || self
                .repo_name
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(ProtocolError::InvalidRepoName);
        }
        match self.operation {
            Operation::Warm if !self.refs.is_empty() => return Err(ProtocolError::InvalidRefs),
            Operation::Import if self.refs.is_empty() => return Err(ProtocolError::InvalidRefs),
            _ => {}
        }
        if self.refs.iter().any(|(name, sha)| {
            !valid_ref(name)
                || sha.len() != 40
                || !sha
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                || sha.bytes().all(|b| b == b'0')
        }) {
            return Err(ProtocolError::InvalidRefs);
        }
        Ok(())
    }
}

fn valid_ref(name: &str) -> bool {
    (name.starts_with("refs/heads/") || name.starts_with("refs/tags/"))
        && name.len() <= 1024
        && !name.contains("..")
        && !name.contains("@{")
        && !name.ends_with('.')
        && !name
            .bytes()
            .any(|b| !b.is_ascii() || b <= b' ' || b == 127 || b"~^:?*[\\".contains(&b))
        && name
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && !part.ends_with(".lock"))
}

impl Response {
    /// Construct the success response for a completed, validated request.
    pub fn for_request(request: &Request) -> Self {
        Self {
            version: request.version,
            request_id: request.request_id.clone(),
            repo_name: request.repo_name.clone(),
            operation: request.operation,
            refs: request.refs.clone(),
            status: match request.operation {
                Operation::Warm => Status::Ready,
                Operation::Import => Status::Completed,
            },
        }
    }

    /// Require a full echo and the status associated with the requested operation.
    pub fn matches_request(&self, request: &Request) -> bool {
        self == &Self::for_request(request)
    }

    fn validate(&self) -> Result<(), ProtocolError> {
        let request = Request {
            version: self.version,
            request_id: self.request_id.clone(),
            repo_name: self.repo_name.clone(),
            operation: self.operation,
            refs: self.refs.clone(),
        };
        request.validate()?;
        if !self.matches_request(&request) {
            return Err(ProtocolError::InvalidStatus);
        }
        Ok(())
    }
}

fn payload<'a>(frame: &'a [u8], prefix: &[u8]) -> Result<&'a [u8], ProtocolError> {
    if frame.len() > MAX_FRAME_BYTES {
        return Err(ProtocolError::InvalidFrame);
    }
    let body = frame
        .strip_prefix(prefix)
        .and_then(|body| body.strip_suffix(b"\n"))
        .ok_or(ProtocolError::InvalidFrame)?;
    if body.contains(&b'\n') || body.contains(&b'\r') {
        return Err(ProtocolError::InvalidFrame);
    }
    Ok(body)
}

fn encode(value: &impl Serialize, prefix: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    let mut frame = prefix.to_vec();
    serde_json::to_writer(&mut frame, value).map_err(|_| ProtocolError::InvalidFrame)?;
    frame.push(b'\n');
    if frame.len() > MAX_FRAME_BYTES {
        return Err(ProtocolError::InvalidFrame);
    }
    Ok(frame)
}

/// Encode one validated request, including its terminating newline.
pub fn encode_request(request: &Request) -> Result<Vec<u8>, ProtocolError> {
    request.validate()?;
    encode(request, b"")
}

/// Decode one complete bounded request frame.
pub fn decode_request(frame: &[u8]) -> Result<Request, ProtocolError> {
    let request: Request =
        serde_json::from_slice(payload(frame, b"")?).map_err(|_| ProtocolError::InvalidFrame)?;
    request.validate()?;
    Ok(request)
}

/// Encode one validated success response, including prefix and newline.
pub fn encode_response(response: &Response) -> Result<Vec<u8>, ProtocolError> {
    response.validate()?;
    encode(response, RESPONSE_PREFIX.as_bytes())
}

/// Decode one complete bounded response frame, including its required prefix.
pub fn decode_response(frame: &[u8]) -> Result<Response, ProtocolError> {
    let response: Response = serde_json::from_slice(payload(frame, RESPONSE_PREFIX.as_bytes())?)
        .map_err(|_| ProtocolError::InvalidFrame)?;
    response.validate()?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    fn request(operation: Operation) -> Request {
        Request {
            version: PROTOCOL_VERSION,
            request_id: "attempt-1".to_owned(),
            repo_name: "par-msl/jarvis".to_owned(),
            operation,
            refs: if operation == Operation::Import {
                BTreeMap::from([("refs/heads/main".to_owned(), "1".repeat(40))])
            } else {
                BTreeMap::new()
            },
        }
    }

    #[mononoke::test]
    fn round_trip_requires_the_complete_echo() {
        for operation in [Operation::Warm, Operation::Import] {
            let request = request(operation);
            assert_eq!(
                decode_request(&encode_request(&request).unwrap()).unwrap(),
                request
            );
            let mut response = Response::for_request(&request);
            assert_eq!(
                decode_response(&encode_response(&response).unwrap()).unwrap(),
                response
            );
            assert!(response.matches_request(&request));
            response.request_id.push('x');
            assert!(!response.matches_request(&request));
        }
    }

    #[mononoke::test]
    fn rejects_wrong_version_status_unknown_fields_and_incomplete_frames() {
        let mut request = request(Operation::Warm);
        request.version += 1;
        assert_eq!(encode_request(&request), Err(ProtocolError::InvalidVersion));
        request.version = PROTOCOL_VERSION;
        let mut response = Response::for_request(&request);
        response.status = Status::Completed;
        assert_eq!(
            encode_response(&response),
            Err(ProtocolError::InvalidStatus)
        );
        let mut frame = encode_request(&request).unwrap();
        frame.pop();
        assert_eq!(decode_request(&frame), Err(ProtocolError::InvalidFrame));
        let frame = b"{\"version\":1,\"request_id\":\"x\",\"repo_name\":\"repo\",\"operation\":\"warm\",\"refs\":{},\"unknown\":true}\n";
        assert_eq!(decode_request(frame), Err(ProtocolError::InvalidFrame));
        assert_eq!(
            decode_request(&vec![b'x'; MAX_FRAME_BYTES + 1]),
            Err(ProtocolError::InvalidFrame)
        );
    }

    #[mononoke::test]
    fn rejects_ambiguous_refs_and_invalid_identifiers() {
        for name in [
            "refs/heads/a..b",
            "refs/heads/a.lock",
            "refs/heads/a@{b",
            "refs/heads/.a",
            "refs/heads/a//b",
            "refs/heads/a*",
            "refs/trees/a",
        ] {
            let mut request = request(Operation::Import);
            request.refs = BTreeMap::from([(name.to_owned(), "1".repeat(40))]);
            assert_eq!(request.validate(), Err(ProtocolError::InvalidRefs));
        }
        let mut request = request(Operation::Warm);
        request.request_id = "bad\nidentifier".to_owned();
        assert_eq!(request.validate(), Err(ProtocolError::InvalidRequestId));
    }
}
