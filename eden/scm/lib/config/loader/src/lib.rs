/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! `configloader` is about how to load config locally and remotely for a repo.
//!
//! It is heavyweight because of the remote config logic. There are lightweight
//! choices if you do not need logic to load configs, for example, if you can
//! just get a config from elsewhere.
//!
//! If you're looking for just reading configs, use `&dyn configmodel::Config`.
//! If you're looking for reading configs and some extra features like setting
//! configs, use `configset::ConfigSet`.

pub mod hg;

pub use configmodel;
pub use configmodel::convert;
pub use configmodel::error;
pub use configmodel::Config;
pub use configmodel::Error;
pub use configmodel::Result;
pub use configset::config;
pub use error::Errors;
// Re-export
pub use minibytes::Text;

#[cfg(feature = "fb")]
pub mod fb;

mod builtin_static;
