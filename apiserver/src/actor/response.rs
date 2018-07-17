// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::result::Result;

use actix_web::{Body, HttpRequest, HttpResponse, Responder};
use bytes::Bytes;

use errors::ErrorKind;

pub enum MononokeRepoResponse {
    GetRawFile { content: Bytes },
}

fn binary_response(content: Bytes) -> HttpResponse {
    HttpResponse::Ok()
        .content_type("application/octet-stream")
        .body(Body::Binary(content.into()))
}

impl Responder for MononokeRepoResponse {
    type Item = HttpResponse;
    type Error = ErrorKind;

    fn respond_to<S: 'static>(self, _req: &HttpRequest<S>) -> Result<Self::Item, Self::Error> {
        use self::MononokeRepoResponse::*;

        match self {
            GetRawFile { content } => Ok(binary_response(content)),
        }
    }
}
