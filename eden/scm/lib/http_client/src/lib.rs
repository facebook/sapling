/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! A simple HTTP client built on top of libcurl.

#![deny(warnings)]

mod errors;
mod handler;
mod request;
mod response;

pub use errors::{CertOrKeyMissing, HttpClientError};
pub use request::Request;
pub use response::Response;
