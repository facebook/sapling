/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// Include the mpatch bindings
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use std::os::raw::{c_char, c_int, c_void};

#[repr(C)]
pub struct mpatch_flist {
    pub base: *mut mpatch_frag,
    pub head: *mut mpatch_frag,
    pub tail: *mut mpatch_frag,
}

#[repr(C)]
pub struct mpatch_frag {
    pub start: c_int,
    pub end: c_int,
    pub len: c_int,
    pub data: *const c_char,
}

extern "C" {
    pub fn mpatch_decode(bin: *const c_char, len: isize, res: *mut *mut mpatch_flist) -> c_int;

    pub fn mpatch_calcsize(len: isize, l: *mut mpatch_flist) -> isize;

    pub fn mpatch_lfree(a: *mut mpatch_flist);

    pub fn mpatch_apply(
        buf: *mut c_char,
        orig: *const c_char,
        len: isize,
        l: *mut mpatch_flist,
    ) -> c_int;

    pub fn mpatch_fold(
        bins: *mut c_void,
        get_next_item: Option<
            unsafe extern "C" fn(arg1: *mut c_void, arg2: isize) -> *mut mpatch_flist,
        >,
        start: isize,
        end: isize,
    ) -> *mut mpatch_flist;
}
