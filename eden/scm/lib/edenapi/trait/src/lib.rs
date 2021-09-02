/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub mod api;
pub mod errors;
pub mod response;

pub use crate::api::{EdenApi, ProgressCallback};
pub use crate::errors::{ConfigError, EdenApiError};
pub use crate::response::{Entries, Response, ResponseMeta};

// Re-export for convenience.
pub use configmodel;
pub use edenapi_types as types;

pub type Result<T> = std::result::Result<T, EdenApiError>;
