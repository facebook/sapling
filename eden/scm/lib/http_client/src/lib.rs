/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! An async-compatible HTTP client built on top of libcurl.

#![deny(warnings)]

mod cbor;
mod client;
mod driver;
mod errors;
mod handler;
mod header;
mod pool;
mod progress;
mod receiver;
mod request;
mod response;
mod stats;

pub use cbor::CborStream;
pub use client::{HttpClient, ResponseStream, StatsFuture};
pub use errors::{Abort, CertOrKeyMissing, HttpClientError};
pub use header::Header;
pub use progress::Progress;
pub use receiver::Receiver;
pub use request::{Request, StreamRequest};
pub use response::{AsyncBody, AsyncResponse, Response};
pub use stats::Stats;
