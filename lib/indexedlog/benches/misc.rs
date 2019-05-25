// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use indexedlog::base16::Base16Iter;
use indexedlog::utils::{open_dir, ScopedFileLock};
use minibench::{bench, elapsed};
use tempfile::tempdir;

fn main() {
    bench("base16 iterating 1M bytes", || {
        let x = vec![4u8; 1000000];
        elapsed(|| {
            let y: u8 = Base16Iter::from_base256(&x).sum();
            assert_eq!(y, (4 * 1000000) as u8);
        })
    });

    bench("lock a directory", || {
        let dir = tempdir().unwrap();
        let mut file = open_dir(dir.path()).unwrap();
        elapsed(|| {
            let _lock = ScopedFileLock::new(&mut file, true).unwrap();
        })
    });

    bench("open and lock a directory", || {
        let dir = tempdir().unwrap();
        elapsed(|| {
            let mut file = open_dir(dir.path()).unwrap();
            let _lock = ScopedFileLock::new(&mut file, true).unwrap();
        })
    });
}
