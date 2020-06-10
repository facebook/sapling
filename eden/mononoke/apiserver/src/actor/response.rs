/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

use actix_web::{self, dev::BodyStream, Body, HttpRequest, HttpResponse, Json, Responder};
use anyhow::Error;
use bytes::Bytes;
use futures_ext::{BoxStream, FutureExt, StreamExt};
use futures_old::Stream;
use serde::{Deserialize, Serialize};
use tokio::{self, sync::mpsc};
use tokio_compat::runtime::TaskExecutor;

use edenapi_types::{DataEntry, DataResponse, HistoryResponse, WireHistoryEntry};
use types::RepoPathBuf;

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
    GetFileHistory {
        history: Vec<Changeset>,
    },
    GetLastCommitOnPath {
        commit: Changeset,
    },
    IsAncestor {
        answer: bool,
    },

    // NOTE: Please add serializable responses before this line
    #[serde(skip)]
    GetRawFile(TaskExecutor, FileStream),
    #[serde(skip)]
    GetBlobContent(TaskExecutor, FileStream),
    #[serde(skip)]
    EdenGetData(DataResponse),
    #[serde(skip)]
    EdenGetHistory(HistoryResponse),
    #[serde(skip)]
    EdenGetTrees(DataResponse),
    #[serde(skip)]
    EdenPrefetchTrees(DataResponse),
    #[serde(skip)]
    EdenGetDataStream(TaskExecutor, StreamingDataResponse),
    #[serde(skip)]
    EdenGetHistoryStream(TaskExecutor, StreamingHistoryResponse),
    #[serde(skip)]
    EdenGetTreesStream(TaskExecutor, StreamingDataResponse),
    #[serde(skip)]
    EdenPrefetchTreesStream(TaskExecutor, StreamingDataResponse),
}

fn hostname() -> Option<String> {
    hostname::get().ok()?.into_string().ok()
}

fn binary_response(content: Bytes) -> HttpResponse {
    HttpResponse::Ok()
        .content_type("application/octet-stream")
        .header("x-served-by", hostname().unwrap_or_default())
        .body(Body::Binary(content.into()))
}

fn cbor_response(content: impl Serialize) -> HttpResponse {
    let content = serde_cbor::to_vec(&content).unwrap();
    HttpResponse::Ok()
        .content_type("application/cbor")
        .header("x-served-by", hostname().unwrap_or_default())
        .body(Body::Binary(content.into()))
}

fn streaming_cbor_response<S, I>(executor: TaskExecutor, entries: S) -> HttpResponse
where
    S: Stream<Item = I, Error = Error> + Send + 'static,
    I: Serialize + Sync + Send + 'static,
{
    let (tx, rx) = mpsc::channel(1);
    executor.spawn(entries.forward(tx).discard());

    let stream = rx
        .map_err(|e| failure::Error::from_boxed_compat(e.into()))
        .and_then(|entry| Ok(serde_cbor::to_vec(&entry)?))
        .map(Bytes::from)
        .map_err(|e| failure::Error::from_boxed_compat(e.into()))
        .from_err()
        .boxify();

    HttpResponse::Ok()
        .content_type("application/cbor")
        .header("x-served-by", hostname().unwrap_or_default())
        .body(Body::Streaming(stream as BodyStream))
}

fn streaming_binary_response(executor: TaskExecutor, stream: FileStream) -> HttpResponse {
    let (tx, rx) = mpsc::channel(1);
    executor.spawn(stream.into_bytes_stream().forward(tx).discard());

    let stream = rx
        .map_err(|e| failure::Error::from_boxed_compat(e.into()))
        .from_err()
        .boxify();

    HttpResponse::Ok()
        .content_type("application/octet-stream")
        .header("x-served-by", hostname().unwrap_or_default())
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
            GetFileHistory { history } => Json(history).respond_to(req),
            GetLastCommitOnPath { commit } => Json(commit).respond_to(req),
            IsAncestor { answer } => Ok(binary_response({
                if answer {
                    "true".into()
                } else {
                    "false".into()
                }
            })),
            GetRawFile(executor, stream) | GetBlobContent(executor, stream) => {
                Ok(streaming_binary_response(executor, stream))
            }
            EdenGetData(response) => Ok(cbor_response(response)),
            EdenGetHistory(response) => Ok(cbor_response(response)),
            EdenGetTrees(response) => Ok(cbor_response(response)),
            EdenPrefetchTrees(response) => Ok(cbor_response(response)),
            EdenGetDataStream(executor, entries) => Ok(streaming_cbor_response(executor, entries)),
            EdenGetHistoryStream(executor, entries) => {
                Ok(streaming_cbor_response(executor, entries))
            }
            EdenGetTreesStream(executor, entries) => Ok(streaming_cbor_response(executor, entries)),
            EdenPrefetchTreesStream(executor, entries) => {
                Ok(streaming_cbor_response(executor, entries))
            }
        }
    }
}
