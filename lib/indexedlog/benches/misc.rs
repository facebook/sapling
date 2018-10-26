// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate indexedlog;
extern crate minibench;

use indexedlog::base16::Base16Iter;
use minibench::{bench, elapsed};

fn main() {
    bench("base16 iterating 1M bytes", || {
        let x = vec![4u8; 1000000];
        elapsed(|| {
            let y: u8 = Base16Iter::from_base256(&x).sum();
            assert_eq!(y, (4 * 1000000) as u8);
        })
    });
}
