/*
 * Copyright (c) Facebook, Inc. and its affiliates.
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
#[cfg(not(feature = "generated"))]
use self::fallback as imp;

pub use imp::*;
