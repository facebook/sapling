/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod builder;
mod client;
mod response;
mod retryable;

// Re-export for convenience.
pub use configmodel;
pub use edenapi_trait::api;
pub use edenapi_trait::errors;
pub use edenapi_trait::Entries;
pub use edenapi_trait::Response;
pub use edenapi_trait::ResponseMeta;
pub use edenapi_types as types;
pub use http_client::Stats;

pub use crate::api::EdenApi;
pub use crate::builder::Builder;
pub use crate::builder::HttpClientBuilder;
pub use crate::builder::DEFAULT_CORRELATOR;
pub use crate::client::Client;
pub use crate::errors::ConfigError;
pub use crate::errors::EdenApiError;
pub use crate::response::BlockingResponse;

pub type Result<T> = std::result::Result<T, EdenApiError>;
