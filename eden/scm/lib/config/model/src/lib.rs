/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! # configmodel
//!
//! Provides a trait definition for config reading.

pub mod config;
pub mod convert;
pub mod error;

pub use config::Config;
pub use config::ConfigExt;
pub use config::ValueLocation;
pub use config::ValueSource;
pub use error::Error;
pub type Result<T> = std::result::Result<T, Error>;

// Re-export
pub use minibytes::Text;
#[cfg(feature = "convert-regex")]
pub use regex::Regex;
