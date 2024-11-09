/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
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

pub use crate::api::SaplingRemoteApi;
pub use crate::builder::Builder;
pub use crate::builder::HttpClientBuilder;
pub use crate::client::Client;
pub use crate::client::RECENT_DOGFOODING_REQUESTS;
pub use crate::errors::ConfigError;
pub use crate::errors::SaplingRemoteApiError;
pub use crate::response::BlockingResponse;

pub type Result<T> = std::result::Result<T, SaplingRemoteApiError>;
