// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

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

/// Just implement Weight in terms of memory use for any type which implements `HeapSizeOf`.
/// XXX Not sure how well this matches the O(1) requirement...
impl<T> Weight for T
where
    T: HeapSizeOf,
{
    #[inline]
    fn get_weight(&self) -> usize {
        mem::size_of::<Self>() + self.heap_size_of_children()
    }
}
