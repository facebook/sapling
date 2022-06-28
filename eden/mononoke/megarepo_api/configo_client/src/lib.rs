/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![cfg_attr(not(fbcode_build), allow(unused_crate_dependencies))]
#![feature(trait_alias)]

#[cfg(fbcode_build)]
mod facebook;
#[cfg(fbcode_build)]
pub use facebook::ConfigObject;
#[cfg(fbcode_build)]
pub use facebook::MononokeConfigoClient;

// There is no way to implement this for non-fbcode builds
// and it's worth having this crate's users know this, so
// let's not even export any stubs.
