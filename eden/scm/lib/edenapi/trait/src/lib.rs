/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

pub mod api;
pub mod errors;
pub mod response;

// Re-export for convenience.
pub use configmodel;
pub use edenapi_types as types;

pub use crate::api::SaplingRemoteApi;
pub use crate::errors::ConfigError;
pub use crate::errors::SaplingRemoteApiError;
pub use crate::response::Entries;
pub use crate::response::Response;
pub use crate::response::ResponseMeta;

pub type Result<T> = std::result::Result<T, SaplingRemoteApiError>;
