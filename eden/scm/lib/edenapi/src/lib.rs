/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod blocking;
mod builder;
mod client;
mod response;

pub use edenapi_trait::api;
pub use edenapi_trait::errors;

pub use crate::api::{EdenApi, ProgressCallback};
pub use crate::blocking::EdenApiBlocking;
pub use crate::builder::Builder;
pub use crate::builder::HttpClientBuilder;
pub use crate::client::Client;
pub use crate::errors::{ConfigError, EdenApiError};
pub use crate::response::BlockingResponse;
pub use edenapi_trait::{Entries, Response, ResponseMeta};

// Re-export for convenience.
pub use configmodel;
pub use edenapi_types as types;
pub use http_client::{Progress, Stats};

pub type Result<T> = std::result::Result<T, EdenApiError>;
