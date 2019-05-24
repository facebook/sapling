// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::BTreeMap;

use actix_web::{self, dev::BodyStream, Body, HttpRequest, HttpResponse, Json, Responder};
use bytes::Bytes;
use futures::Stream;
use serde::Serialize;
use serde_cbor;

use types::api::{DataResponse, HistoryResponse};

use super::lfs::BatchResponse;
use super::model::{Changeset, Entry, EntryWithSizeAndContentHash};

type SendBodyStream = Box<Stream<Item = Bytes, Error = actix_web::Error> + Send + 'static>;

pub enum MononokeRepoResponse {
    GetRawFile {
        content: Bytes,
    },
    GetHgFile {
        content: Bytes,
    },
    GetFileHistory {
        history: SendBodyStream,
    },
    GetBlobContent {
        content: Bytes,
    },
    ListDirectory {
        files: Box<dyn Iterator<Item = Entry> + Send>,
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
    IsAncestor {
        answer: bool,
    },
    DownloadLargeFile {
        content: Bytes,
    },
    LfsBatch {
        response: BatchResponse,
    },
    UploadLargeFile {},
    EdenGetData(DataResponse),
    EdenGetHistory(HistoryResponse),
    EdenGetTrees(DataResponse),
    EdenPrefetchTrees(DataResponse),
}

fn binary_response(content: Bytes) -> HttpResponse {
    HttpResponse::Ok()
        .content_type("application/octet-stream")
        .body(Body::Binary(content.into()))
}

fn cbor_response(content: impl Serialize) -> HttpResponse {
    let content = serde_cbor::to_vec(&content).unwrap();
    HttpResponse::Ok()
        .content_type("application/cbor")
        .body(Body::Binary(content.into()))
}

fn streaming_response(stream: SendBodyStream) -> HttpResponse {
    HttpResponse::Ok()
        .content_type("application/octet-stream")
        .body(Body::Streaming(stream as BodyStream))
}

impl Responder for MononokeRepoResponse {
    type Item = HttpResponse;
    type Error = actix_web::Error;

    fn respond_to<S: 'static>(self, req: &HttpRequest<S>) -> Result<Self::Item, Self::Error> {
        use self::MononokeRepoResponse::*;

        match self {
            GetRawFile { content } | GetBlobContent { content } | GetHgFile { content } => {
                Ok(binary_response(content))
            }
            GetFileHistory { history } => Ok(streaming_response(history)),
            ListDirectory { files } => Json(files.collect::<Vec<_>>()).respond_to(req),
            GetTree { files } => Json(files).respond_to(req),
            GetChangeset { changeset } => Json(changeset).respond_to(req),
            GetBranches { branches } => Json(branches).respond_to(req),
            IsAncestor { answer } => Ok(binary_response({
                if answer {
                    "true".into()
                } else {
                    "false".into()
                }
            })),
            DownloadLargeFile { content } => Ok(binary_response(content.into())),
            LfsBatch { response } => Json(response).respond_to(req),
            UploadLargeFile {} => Ok(HttpResponse::Ok().into()),
            EdenGetData(response) => Ok(cbor_response(response)),
            EdenGetHistory(response) => Ok(cbor_response(response)),
            EdenGetTrees(response) => Ok(cbor_response(response)),
            EdenPrefetchTrees(response) => Ok(cbor_response(response)),
        }
    }
}
