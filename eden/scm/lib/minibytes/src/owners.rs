/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
