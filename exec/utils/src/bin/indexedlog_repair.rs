// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! # indexedlog-repair
//!
//! Repair indexed log by truncating bad entries and nuking indexes.
//! Usage: `indexedlog-repair LOG_DIRECTORY`.

// Write code paths are not used.
#![allow(dead_code)]

extern crate indexedlog;
use std::{env, path::Path};

fn main() {
    for arg in env::args().skip(1) {
        let path = Path::new(&arg);
        assert!(path.is_dir());
        let log = indexedlog::log::Log::open(path, Vec::new()).unwrap();
        println!("Repairing {:?}", path);
        unsafe { log.repair() }.unwrap();
        println!("Done");
    }
}
