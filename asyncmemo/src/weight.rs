// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bytes::Bytes;
use std::mem;

use heapsize::HeapSizeOf;

/// Return the "weight" of a type.
///
/// This is an abstract value which can mean anything, but it typically
/// relates to memory consumption. The expectation is that calling `get_weight()`
/// fairly cheap - ideally O(1).
pub trait Weight {
    fn get_weight(&self) -> usize;
}

impl Weight for String {
    #[inline]
    fn get_weight(&self) -> usize {
        mem::size_of::<Self>() + self.heap_size_of_children()
    }
}

impl Weight for u32 {
    #[inline]
    fn get_weight(&self) -> usize {
        mem::size_of::<Self>()
    }
}

impl Weight for u64 {
    #[inline]
    fn get_weight(&self) -> usize {
        mem::size_of::<Self>()
    }
}

impl Weight for i32 {
    #[inline]
    fn get_weight(&self) -> usize {
        mem::size_of::<Self>()
    }
}

impl Weight for i64 {
    #[inline]
    fn get_weight(&self) -> usize {
        mem::size_of::<Self>()
    }
}

impl Weight for Bytes {
    #[inline]
    fn get_weight(&self) -> usize {
        self.len()
    }
}
