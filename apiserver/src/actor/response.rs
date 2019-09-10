// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::BTreeMap;

use actix_web::{self, dev::BodyStream, Body, HttpRequest, HttpResponse, Json, Responder};
use bytes::Bytes;
use failure::Error;
use futures::Stream;
use futures_ext::{BoxStream, StreamExt};
use hostname::get_hostname;
use serde::{Deserialize, Serialize};
use serde_cbor;

use types::{
    api::{DataResponse, HistoryResponse},
    DataEntry, RepoPathBuf, WireHistoryEntry,
};

use super::file_stream::FileStream;
use super::model::{Changeset, Entry, EntryLight, EntryWithSizeAndContentHash};

type StreamingDataResponse = BoxStream<DataEntry, Error>;
type StreamingHistoryResponse = BoxStream<(RepoPathBuf, WireHistoryEntry), Error>;

#[derive(Serialize, Deserialize)]
pub enum MononokeRepoResponse {
    ListDirectory {
        files: Vec<Entry>,
    },
    ListDirectoryUnodes {
        files: Vec<EntryLight>,
    },
    GetTree {
        files: Vec<EntryWithSizeAndContentHash>,
    },
    GetChangeset {
        changeset: Changeset,
    },
    GetBranches {
        branches: BTreeMap<String, String>,
    },
    GetLastCommitOnPath {
        commit: Changeset,
    },
    IsAncestor {
        answer: bool,
    },

    // NOTE: Please add serializable responses before this line
    #[serde(skip)]
    GetRawFile(FileStream),
    #[serde(skip)]
    GetBlobContent(FileStream),
    #[serde(skip)]
    EdenGetData(DataResponse),
    #[serde(skip)]
    EdenGetHistory(HistoryResponse),
    #[serde(skip)]
    EdenGetTrees(DataResponse),
    #[serde(skip)]
    EdenPrefetchTrees(DataResponse),
    #[serde(skip)]
    EdenGetDataStream(StreamingDataResponse),
    #[serde(skip)]
    EdenGetHistoryStream(StreamingHistoryResponse),
    #[serde(skip)]
    EdenGetTreesStream(StreamingDataResponse),
    #[serde(skip)]
    EdenPrefetchTreesStream(StreamingDataResponse),
}

fn binary_response(content: Bytes) -> HttpResponse {
    HttpResponse::Ok()
        .content_type("application/octet-stream")
        .header("x-served-by", get_hostname().unwrap_or_default())
        .body(Body::Binary(content.into()))
}

fn cbor_response(content: impl Serialize) -> HttpResponse {
    let content = serde_cbor::to_vec(&content).unwrap();
    HttpResponse::Ok()
        .content_type("application/cbor")
        .header("x-served-by", get_hostname().unwrap_or_default())
        .body(Body::Binary(content.into()))
}

fn streaming_cbor_response<S, I>(entries: S) -> HttpResponse
where
    S: Stream<Item = I, Error = Error> + Send + 'static,
    I: Serialize,
{
    let stream = entries
        .and_then(|entry| Ok(serde_cbor::to_vec(&entry)?))
        .map(Bytes::from)
        .from_err()
        .boxify();
    HttpResponse::Ok()
        .content_type("application/cbor")
        .header("x-served-by", get_hostname().unwrap_or_default())
        .body(Body::Streaming(stream as BodyStream))
}

fn streaming_binary_response(stream: FileStream) -> HttpResponse {
    let stream = stream.into_bytes_stream().from_err().boxify();

    HttpResponse::Ok()
        .content_type("application/octet-stream")
        .header("x-served-by", get_hostname().unwrap_or_default())
        .body(Body::Streaming(stream))
}

impl Responder for MononokeRepoResponse {
    type Item = HttpResponse;
    type Error = actix_web::Error;

    fn respond_to<S: 'static>(self, req: &HttpRequest<S>) -> Result<Self::Item, Self::Error> {
        use self::MononokeRepoResponse::*;

        match self {
            ListDirectory { files } => Json(files).respond_to(req),
            ListDirectoryUnodes { files } => Json(files).respond_to(req),
            GetTree { files } => Json(files).respond_to(req),
            GetChangeset { changeset } => Json(changeset).respond_to(req),
            GetBranches { branches } => Json(branches).respond_to(req),
            GetLastCommitOnPath { commit } => Json(commit).respond_to(req),
            IsAncestor { answer } => Ok(binary_response({
                if answer {
                    "true".into()
                } else {
                    "false".into()
                }
            })),
            GetRawFile(stream) | GetBlobContent(stream) => Ok(streaming_binary_response(stream)),
            EdenGetData(response) => Ok(cbor_response(response)),
            EdenGetHistory(response) => Ok(cbor_response(response)),
            EdenGetTrees(response) => Ok(cbor_response(response)),
            EdenPrefetchTrees(response) => Ok(cbor_response(response)),
            EdenGetDataStream(entries) => Ok(streaming_cbor_response(entries)),
            EdenGetHistoryStream(entries) => Ok(streaming_cbor_response(entries)),
            EdenGetTreesStream(entries) => Ok(streaming_cbor_response(entries)),
            EdenPrefetchTreesStream(entries) => Ok(streaming_cbor_response(entries)),
        }
    }
}
