/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

mod api;
mod blocking;
mod builder;
mod client;
mod errors;
mod name;
mod response;

pub use crate::api::{EdenApi, ProgressCallback};
pub use crate::blocking::EdenApiBlocking;
pub use crate::builder::Builder;
pub use crate::client::Client;
pub use crate::errors::{ConfigError, EdenApiError};
pub use crate::name::RepoName;
pub use crate::response::{BlockingFetch, Entries, Fetch, ResponseMeta};

// Re-export for convenience.
pub use http_client::{Progress, Stats};
