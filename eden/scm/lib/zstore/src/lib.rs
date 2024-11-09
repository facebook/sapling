/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Blob store backed by zstd delta compression.

mod errors;
mod zstore;

pub use errors::Error;
pub use errors::Result;
pub use indexedlog::Repair;

pub use crate::zstore::sha1;
pub use crate::zstore::Id20;
pub use crate::zstore::OpenOptions;
pub use crate::zstore::Zstore;
