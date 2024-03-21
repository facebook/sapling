/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
