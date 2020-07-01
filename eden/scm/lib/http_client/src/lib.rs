/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! A simple HTTP client built on top of libcurl.

#![deny(warnings)]

mod client;
mod driver;
mod errors;
mod handler;
mod header;
mod progress;
mod receiver;
mod request;
mod response;
mod stats;

pub use client::HttpClient;
pub use errors::{Abort, CertOrKeyMissing, HttpClientError};
pub use header::Header;
pub use progress::Progress;
pub use receiver::Receiver;
pub use request::{Request, StreamRequest};
pub use response::{AsyncResponse, Response};
pub use stats::Stats;
