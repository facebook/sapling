/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! A simple binary that runs xdiff in a minimal way. This is mainly for
//! exposing xdiff logic so it can be used in command line for testing purpose.
//! It also serves as an example of how to use xdiff.
extern crate xdiff_sys;

use std::env;
use std::fs;
use std::os::raw::c_char;
use std::os::raw::c_int;
use std::os::raw::c_void;

use xdiff_sys::mmfile_t;
use xdiff_sys::xdemitcb_t;
use xdiff_sys::xdemitconf_t;
use xdiff_sys::xdl_diff_vendored;
use xdiff_sys::xpparam_t;

unsafe extern "C" fn hunk_func(a1: i64, a2: i64, b1: i64, b2: i64, _priv: *mut c_void) -> c_int {
    println!("@@ -{},{} +{},{} @@", a1, a2, b1, b2);
    0
}

fn main() -> Result<(), std::io::Error> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        eprintln!("usage: {} FILE1 FILE2\n", &args[0]);
        std::process::exit(1);
    }

    let mut a = fs::read(&args[1])?;
    let mut a_mmfile = mmfile_t {
        ptr: a.as_mut_ptr() as *mut c_char,
        size: a.len() as i64,
    };
    let mut b = fs::read(&args[2])?;
    let mut b_mmfile = mmfile_t {
        ptr: b.as_mut_ptr() as *mut c_char,
        size: b.len() as i64,
    };
    let xpp = xpparam_t {
        flags: 0,
        max_edit_cost: 0,
    };
    let xecfg = xdemitconf_t {
        flags: 0,
        hunk_func: Some(hunk_func),
    };
    let mut ecb = xdemitcb_t {
        priv_: std::ptr::null_mut(),
    };

    unsafe {
        xdl_diff_vendored(&mut a_mmfile, &mut b_mmfile, &xpp, &xecfg, &mut ecb);
    }
    Ok(())
}
