/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Implement [`BytesOwner`] and [`TextOwner`] for common types.

use memmap::Mmap;

use crate::BytesOwner;
use crate::TextOwner;

impl BytesOwner for Vec<u8> {}
impl BytesOwner for Box<[u8]> {}
impl BytesOwner for String {}
impl BytesOwner for Mmap {}
#[cfg(feature = "frombytes")]
impl BytesOwner for bytes::Bytes {}

impl TextOwner for String {}
