/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

pub mod client;
mod errors;
mod filechangedetector;
pub mod filesystem;
pub mod git;
pub mod metadata;
pub mod sparse;
pub mod status;
pub mod util;
pub mod wait;
pub mod walker;
mod watchman_client;
pub mod workingcopy;

pub use workingcopy::WorkingCopy;
