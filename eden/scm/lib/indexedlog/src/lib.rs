/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![allow(dead_code)]

//! # Indexed Log
//!
//! Indexed Log provides an integrity-checked, append-only storage
//! with index support.
//!
//! See [log::Log] for the main structure. The index can be used independently.
//! See [index::Index] for details.

#[macro_use]
mod macros;

pub mod base16;
mod change_detect;
pub mod config;
mod errors;
pub mod index;
pub mod lock;
pub mod log;
pub mod multi;
mod page_out;
mod repair;
pub mod rotate;
pub mod utils;

#[cfg(all(unix, feature = "sigbus-handler"))]
mod sigbus;

#[cfg_attr(
    not(all(target_os = "linux", feature = "btrfs")),
    path = "dummy_btrfs.rs"
)]
mod btrfs;

pub use errors::Error;
pub use errors::Result;
pub use repair::DefaultOpenOptions;
pub use repair::OpenWithRepair;
pub use repair::Repair;

#[cfg(test)]
dev_logger::init!();
