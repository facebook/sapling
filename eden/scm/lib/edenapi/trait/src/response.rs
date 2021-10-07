/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::pin::Pin;

use futures::prelude::*;
use http::{header, HeaderMap, StatusCode, Version};

use http_client::{AsyncResponse as AsyncHttpResponse, Response as HttpResponse, Stats};

use crate::errors::EdenApiError;

const SERVER_HEADER: &str = "server";
const REQUEST_ID_HEADER: &str = "x-request-id";
const TW_TASK_HEADER: &str = "x-tw-task";
const TW_VERSION_HEADER: &str = "x-tw-task-version";
const TW_CANARY_HEADER: &str = "x-tw-canary-id";
const SERVER_LOAD_HEADER: &str = "x-load";

/// A generic `Stream` of "entries" representing the deserialized content
/// of a streaming response from the server.
pub type Entries<T> = Pin<Box<dyn Stream<Item = Result<T, EdenApiError>> + Send + 'static>>;
pub type StatsFuture = Pin<Box<dyn Future<Output = Result<Stats, EdenApiError>> + Send + 'static>>;

/// The result of a data fetching operation, which may have involved
/// several individual HTTP requests.
pub struct Response<T> {
    /// A `Stream` containing the combined responses for all of the requests.
    /// There are no ordering guarantees; entries from different HTTP responses
    /// may be arbitrarily interleaved.
    pub entries: Entries<T>,

    /// A `Future` that returns the aggregated transfer stastics for the
    /// all of the HTTP requests involved in the fetching operation. Will
    /// only resolve once all of the requests have completed.
    pub stats: StatsFuture,
}

impl<T: Send + 'static> Response<T> {
    pub fn empty() -> Self {
        Self {
            entries: stream::empty().boxed(),
            stats: future::ok(Stats::default()).boxed(),
        }
    }

    /// Flatten the response into a `Vec`.
    pub async fn flatten(self) -> Result<Vec<T>, EdenApiError> {
        self.entries.try_collect().await
    }
}

/// Metadata extracted from the headers of an individual HTTP response.
#[derive(Debug, Default)]
pub struct ResponseMeta {
    pub version: Version,
    pub status: StatusCode,
    pub server: Option<String>,
    pub request_id: Option<String>,
    pub tw_task_handle: Option<String>,
    pub tw_task_version: Option<String>,
    pub tw_canary_id: Option<String>,
    pub server_load: Option<usize>,
    pub content_length: Option<usize>,
    pub content_encoding: Option<String>,
}

impl ResponseMeta {
    fn from_parts(version: Version, status: StatusCode, headers: &HeaderMap) -> Self {
        Self {
            version,
            status,
            server: get_header(headers, SERVER_HEADER),
            request_id: get_header(headers, REQUEST_ID_HEADER),
            tw_task_handle: get_header(headers, TW_TASK_HEADER),
            tw_task_version: get_header(headers, TW_VERSION_HEADER),
            tw_canary_id: get_header(headers, TW_CANARY_HEADER),
            server_load: get_header(headers, SERVER_LOAD_HEADER).and_then(|l| l.parse().ok()),
            content_length: get_header(headers, header::CONTENT_LENGTH.as_str())
                .and_then(|l| l.parse().ok()),
            content_encoding: get_header(headers, header::CONTENT_ENCODING.as_str()),
        }
    }
}

impl From<&HttpResponse> for ResponseMeta {
    fn from(res: &HttpResponse) -> Self {
        Self::from_parts(res.version(), res.status(), res.headers())
    }
}

impl From<&AsyncHttpResponse> for ResponseMeta {
    fn from(res: &AsyncHttpResponse) -> Self {
        Self::from_parts(res.version(), res.status(), res.headers())
    }
}

fn get_header(headers: &HeaderMap, name: &str) -> Option<String> {
    Some(headers.get(name)?.to_str().ok()?.into())
}
