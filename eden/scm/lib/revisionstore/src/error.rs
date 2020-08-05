/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use thiserror::Error;
use url::Url;

use http::status::StatusCode;
use http_client::Method;

#[derive(Debug, Error)]
#[error("Empty Mutable Pack")]
pub struct EmptyMutablePack;

#[derive(Error, Debug)]
pub enum FetchError {
    #[error("Failed to fetch due to http error {}: {} {}", .0, .1, .2)]
    Http(StatusCode, Url, Method),
    #[error("Unexpected end of stream for http fetch: {} {}", .0, .1)]
    EndOfStream(Url, Method),
}
