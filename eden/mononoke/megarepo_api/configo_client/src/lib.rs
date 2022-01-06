/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]
#![feature(trait_alias)]

#[cfg(fbcode_build)]
mod facebook;
#[cfg(fbcode_build)]
pub use facebook::{ConfigObject, MononokeConfigoClient};

// There is no way to implement this for non-fbcode builds
// and it's worth having this crate's users know this, so
// let's not even export any stubs.
