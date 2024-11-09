/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

pub mod base16;
pub mod errors;
pub mod key;
pub mod radix;
pub mod traits;

pub use errors::ErrorKind as Error;
pub type Result<T> = std::result::Result<T, Error>;
