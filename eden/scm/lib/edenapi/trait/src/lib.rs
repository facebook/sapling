/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub mod api;
pub mod errors;
pub mod response;

// Re-export for convenience.
pub use configmodel;
pub use edenapi_types as types;

pub use crate::api::EdenApi;
pub use crate::errors::ConfigError;
pub use crate::errors::EdenApiError;
pub use crate::response::Entries;
pub use crate::response::Response;
pub use crate::response::ResponseMeta;

pub type Result<T> = std::result::Result<T, EdenApiError>;
