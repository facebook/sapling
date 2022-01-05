/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub mod base16;
pub mod errors;
pub mod key;
pub mod radix;
pub mod traits;

pub use errors::ErrorKind as Error;
pub type Result<T> = std::result::Result<T, Error>;
