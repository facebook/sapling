/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::slice;

use libc::c_char;
use libc::size_t;

unsafe fn view_to_slice<'a, T, U>(ptr: *const T, length: size_t) -> &'a [U] {
    if ptr.is_null() {
        assert!(length == 0, "null slices must have zero length");
        &[]
    } else {
        // TODO: validate sizeof(T) * len < isize::MAX
        slice::from_raw_parts(ptr as *const U, length)
    }
}

#[repr(C)]
pub struct StringView {
    ptr: *const c_char,
    len: size_t,
}

impl StringView {
    pub fn slice<'a>(&'a self) -> &'a [u8] {
        unsafe { view_to_slice(self.ptr, self.len) }
    }
}

#[repr(C)]
pub struct ByteView {
    ptr: *const u8,
    len: size_t,
}

impl ByteView {
    pub fn slice<'a>(&'a self) -> &'a [u8] {
        unsafe { view_to_slice(self.ptr, self.len) }
    }
}
