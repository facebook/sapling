// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// A small subset of xdiff bindings that is just enough to run xdl_diff
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use std::os::raw::{c_char, c_int, c_void};

#[repr(C)]
pub struct mmfile_t {
    pub ptr: *const c_char,
    pub size: i64,
}

#[repr(C)]
pub struct xpparam_t {
    pub flags: u64,
}

#[repr(C)]
pub struct xdemitcb_t {
    pub priv_: *mut c_void,
}

pub type xdl_emit_hunk_consume_func_t = Option<
    unsafe extern "C" fn(
        start_a: i64,
        count_a: i64,
        start_b: i64,
        count_b: i64,
        cb_data: *mut c_void,
    ) -> c_int,
>;

#[repr(C)]
pub struct xdemitconf_t {
    pub flags: u64,
    pub hunk_func: xdl_emit_hunk_consume_func_t,
}

extern "C" {
    pub fn xdl_diff(
        mf1: *const mmfile_t,
        mf2: *const mmfile_t,
        xpp: *const xpparam_t,
        xecfg: *const xdemitconf_t,
        ecb: *mut xdemitcb_t,
    ) -> c_int;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xdl_diff() {
        extern "C" fn hunk_func(a1: i64, a2: i64, b1: i64, b2: i64, _priv: *mut c_void) -> c_int {
            let mut _priv = unsafe { (_priv as *mut Vec<(i64, i64, i64, i64)>).as_mut() };
            if let Some(result) = _priv {
                result.push((a1, a2, b1, b2));
            }
            return 0;
        }

        let a = "a\nb\nc\nd\n".to_owned();
        let b = "a\nc\nd\ne\n".to_owned();
        let mut a_mmfile = mmfile_t {
            ptr: a.as_ptr() as *const c_char,
            size: a.len() as i64,
        };
        let mut b_mmfile = mmfile_t {
            ptr: b.as_ptr() as *const c_char,
            size: b.len() as i64,
        };
        let xpp = xpparam_t { flags: 0 };
        let xecfg = xdemitconf_t {
            flags: 0,
            hunk_func: Some(hunk_func),
        };
        let mut result: Vec<(i64, i64, i64, i64)> = Vec::new();
        let mut ecb = xdemitcb_t {
            priv_: &mut result as *mut Vec<(i64, i64, i64, i64)> as *mut c_void,
        };

        unsafe {
            xdl_diff(&mut a_mmfile, &mut b_mmfile, &xpp, &xecfg, &mut ecb);
        }
        assert_eq!(result, [(1, 1, 1, 0), (4, 0, 3, 1)]);
    }
}
