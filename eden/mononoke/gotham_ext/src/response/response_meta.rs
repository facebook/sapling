/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use futures::channel::oneshot::Receiver;
use gotham_derive::StateData;

use super::error_meta::ErrorMeta;
use crate::content_encoding::ContentCompression;

#[derive(Debug, Copy, Clone)]
pub enum HeadersMeta {
    Sized(u64),
    Chunked,
    Compressed(ContentCompression),
}

impl HeadersMeta {
    pub fn content_length(&self) -> Option<u64> {
        match self {
            Self::Sized(s) => Some(*s),
            Self::Compressed(..) => None,
            Self::Chunked => None,
        }
    }
}

pub struct BodyMeta {
    pub bytes_sent: u64,
    pub error_meta: ErrorMeta<Error>,
}

pub struct ResponseMeta {
    headers: HeadersMeta,
    body: BodyMeta,
}

impl ResponseMeta {
    pub fn headers(&self) -> &HeadersMeta {
        &self.headers
    }

    pub fn body(&self) -> &BodyMeta {
        &self.body
    }
}

enum PendingBodyMeta {
    Immediate(u64),
    Deferred(Receiver<BodyMeta>),
    Error(Error),
}

#[derive(StateData)]
pub struct PendingResponseMeta {
    headers: HeadersMeta,
    body: PendingBodyMeta,
}

impl PendingResponseMeta {
    /// Instantiate PendingResponseMeta for a response that will be sent synchronously (i.e. a
    /// set of bytes).
    pub fn immediate(size: u64) -> Self {
        Self {
            headers: HeadersMeta::Sized(size),
            body: PendingBodyMeta::Immediate(size),
        }
    }

    /// Instantiate PendingResponseMeta for a response that will not be sent synchronously (i.e. a
    /// stream).
    pub fn deferred(headers: HeadersMeta, receiver: Receiver<BodyMeta>) -> Self {
        Self {
            headers,
            body: PendingBodyMeta::Deferred(receiver),
        }
    }

    pub fn error(error: Error) -> Self {
        Self {
            headers: HeadersMeta::Sized(0),
            body: PendingBodyMeta::Error(error),
        }
    }

    /// Wait for this response to finish, and return the associated ResponseMeta. For a
    pub async fn finish(self) -> ResponseMeta {
        let Self { headers, body } = self;

        let body = match body {
            PendingBodyMeta::Immediate(bytes_sent) => BodyMeta {
                bytes_sent,
                error_meta: ErrorMeta::new(),
            },
            PendingBodyMeta::Deferred(receiver) => match receiver.await {
                Ok(body) => body,
                Err(_) => BodyMeta {
                    bytes_sent: 0,
                    error_meta: ErrorMeta::one_error(Error::msg("Deferred body meta was not sent")),
                },
            },
            PendingBodyMeta::Error(e) => BodyMeta {
                bytes_sent: 0,
                error_meta: ErrorMeta::one_error(e),
            },
        };

        ResponseMeta { headers, body }
    }
}
