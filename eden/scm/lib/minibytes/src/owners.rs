/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Implement [`BytesOwner`] and [`TextOwner`] for common types.

use crate::{BytesOwner, TextOwner};
use memmap::Mmap;

impl BytesOwner for Vec<u8> {}
impl BytesOwner for Box<[u8]> {}
impl BytesOwner for String {}
impl BytesOwner for Mmap {}

impl TextOwner for String {}
