/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(feature = "generated")]
mod version;
#[cfg(feature = "generated")]
use self::version as imp;

#[cfg(not(feature = "generated"))]
mod fallback;
pub use imp::*;

#[cfg(not(feature = "generated"))]
use self::fallback as imp;
