/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

mod zstdelta;

use std::env::args;
use std::fs::File;
use std::io;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::exit;

use crate::zstdelta::apply;
use crate::zstdelta::diff;

fn read(path: &Path) -> Vec<u8> {
    let mut buf = Vec::new();
    File::open(path)
        .expect("open")
        .read_to_end(&mut buf)
        .expect("read");
    buf
}

fn main() {
    let args: Vec<_> = args().skip(1).collect();
    if args.len() < 3 {
        println!("Usage: zstdelta -c base data > delta\n       zstdelta -d base delta > data\n");
        exit(1);
    }
    let base = read(&PathBuf::from(&args[1]));
    let data = read(&PathBuf::from(&args[2]));
    let out = if args[0] == "-c" {
        diff(&base, &data).expect("diff")
    } else {
        apply(&base, &data).expect("apply")
    };

    io::stdout().write_all(&out).expect("write");
}
