/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::usize;

use bytes::{Buf, BufMut, ByteOrder, IntoBuf};
//use iovec::IoVec;

/// Implementation of `BufMut` to count the size of a resulting buffer
///
/// A "buffer" for counting the size of a resulting buffer. This effectively requires
/// the data to be serialized twice, but with luck inlining will result in most effort used
/// in generating actual data will be elided.
pub struct SizeCounter(usize);

impl SizeCounter {
    #[inline]
    pub fn new() -> Self {
        SizeCounter(0)
    }

    #[inline]
    pub fn size(&self) -> usize {
        self.0
    }

    #[inline]
    pub fn increment(&mut self, inc: usize) {
        self.0 += inc;
    }
}

impl BufMut for SizeCounter {
    #[inline]
    fn remaining_mut(&self) -> usize {
        usize::MAX
    }

    unsafe fn advance_mut(&mut self, _cnt: usize) {}

    unsafe fn bytes_mut(&mut self) -> &mut [u8] {
        unimplemented!("SizeCounter doesn't really have a buffer")
    }

    #[inline]
    fn has_remaining_mut(&self) -> bool {
        true
    }

    //unsafe fn bytes_vec_mut<'a>(&'a mut self, _dst: &mut [&'a mut IoVec]) -> usize {
    //    unimplemented!("SizeCounter doesn't really have a buffer")
    //}

    #[inline]
    fn put<T: IntoBuf>(&mut self, src: T)
    where
        Self: Sized,
    {
        let buf = src.into_buf();
        self.0 += buf.remaining();
    }

    #[inline]
    fn put_slice(&mut self, src: &[u8]) {
        self.0 += src.len()
    }

    #[inline]
    fn put_u8(&mut self, _n: u8) {
        self.0 += 1
    }
    #[inline]
    fn put_i8(&mut self, _n: i8) {
        self.0 += 1
    }

    #[inline]
    fn put_u16<T: ByteOrder>(&mut self, _n: u16) {
        self.0 += 2
    }
    #[inline]
    fn put_i16<T: ByteOrder>(&mut self, _n: i16) {
        self.0 += 2
    }

    #[inline]
    fn put_u32<T: ByteOrder>(&mut self, _n: u32) {
        self.0 += 4
    }
    #[inline]
    fn put_i32<T: ByteOrder>(&mut self, _n: i32) {
        self.0 += 4
    }

    #[inline]
    fn put_u64<T: ByteOrder>(&mut self, _n: u64) {
        self.0 += 8
    }
    #[inline]
    fn put_i64<T: ByteOrder>(&mut self, _n: i64) {
        self.0 += 8
    }

    #[inline]
    fn put_uint<T: ByteOrder>(&mut self, _n: u64, nbytes: usize) {
        self.0 += nbytes
    }
    #[inline]
    fn put_int<T: ByteOrder>(&mut self, _n: i64, nbytes: usize) {
        self.0 += nbytes
    }

    #[inline]
    fn put_f32<T: ByteOrder>(&mut self, _n: f32) {
        self.0 += 4
    }
    #[inline]
    fn put_f64<T: ByteOrder>(&mut self, _n: f64) {
        self.0 += 8
    }
}
