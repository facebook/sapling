/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! # metalog
//!
//! See [`MetaLog`] for the main structure.

pub mod constants;
mod errors;
mod export;
mod metalog;
mod metalog_ext;
mod resolve;

pub use errors::Error;
pub use errors::Result;
pub use indexedlog::Repair;
#[cfg(test)]
use parking_lot::Mutex;

pub use crate::metalog::resolver;
pub use crate::metalog::CommitOptions;
pub use crate::metalog::Id20;
pub use crate::metalog::MetaLog;

#[cfg(test)]
/// Lock for the environment.  This should be acquired by tests that rely on particular
/// environment variable values that might be overwritten by other tests.
pub static ENV_LOCK: Mutex<()> = Mutex::new(());
