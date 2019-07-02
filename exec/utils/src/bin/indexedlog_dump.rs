// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! # indexedlog-dump
//!
//! Dump Index or Log contents.
//! Usage: `indexedlog-dump INDEX_FILE or LOG_DIRECTORY`.

// Write code paths are not used.
#![allow(dead_code)]

extern crate indexedlog;
use std::{env, path::Path};

fn main() {
    for arg in env::args().skip(1) {
        let path = Path::new(&arg);
        if let Ok(meta) = indexedlog::log::LogMetadata::read_file(path) {
            println!("Metadata File {:?}\n{:?}\n", path, meta);
        } else if path.is_dir() {
            // Treate it as Log.
            let log = indexedlog::log::Log::open(path, Vec::new()).unwrap();
            println!("Log Directory {:?}:\n{:#?}\n", path, log);
        } else if path.is_file() {
            // Treate it as Index.
            let idx = indexedlog::index::OpenOptions::new().open(path).unwrap();
            println!("Index File {:?}\n{:?}\n", path, idx);
        } else {
            println!(
                "Dump intexedlog content\n\n\
                 To dump entries in a Log directory, run:\n\n    \
                 indexedlog-dump DIR\n\n\
                 To dump entries in an Index file, run:\n\n    \
                 indexedlog-dump FILE"
            );
            break;
        }
    }
}
