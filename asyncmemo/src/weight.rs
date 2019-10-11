/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

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

impl<A, B> Weight for (A, B)
where
    A: Weight,
    B: Weight,
{
    #[inline]
    fn get_weight(&self) -> usize {
        self.0.get_weight() + self.1.get_weight()
    }
}

impl<A, B, C> Weight for (A, B, C)
where
    A: Weight,
    B: Weight,
    C: Weight,
{
    #[inline]
    fn get_weight(&self) -> usize {
        self.0.get_weight() + self.1.get_weight() + self.2.get_weight()
    }
}

impl<A> Weight for Option<A>
where
    A: Weight,
{
    #[inline]
    fn get_weight(&self) -> usize {
        let inner_size = self.as_ref().map(Weight::get_weight).unwrap_or(0);

        mem::size_of::<Self>() - mem::size_of::<A>() + inner_size
    }
}
