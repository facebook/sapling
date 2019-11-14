/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// A small subset of xdiff bindings that is just enough to run xdl_diff
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

mod bindgen;

pub use bindgen::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::raw::{c_char, c_int, c_void};

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
            ptr: a.as_ptr() as *mut c_char,
            size: a.len() as i64,
        };
        let mut b_mmfile = mmfile_t {
            ptr: b.as_ptr() as *mut c_char,
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
