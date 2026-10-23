/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl debug bench

pub mod cmd;
pub mod dbio;
#[cfg(fbcode_build)]
mod facebook;
pub mod fsio;
pub mod r#gen;
#[cfg(not(fbcode_build))]
mod oss;
pub mod traversal;
pub mod types;

#[cfg(not(fbcode_build))]
use oss as facebook;
