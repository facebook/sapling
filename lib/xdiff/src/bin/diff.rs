// Copyright 2019 Facebook, Inc.

//! A simple binary that computes the diff between two files.

use std::env;
use std::fs;
use xdiff::{diff_unified, DiffFile, DiffOpts};

fn main() -> Result<(), std::io::Error> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        eprintln!("usage: {} FILE1 FILE2\n", &args[0]);
        std::process::exit(1);
    }

    let a = fs::read(&args[1])?;
    let b = fs::read(&args[2])?;

    let diff = diff_unified(
        Some(DiffFile::new(&args[1], &a)),
        Some(DiffFile::new(&args[2], &b)),
        DiffOpts { context: 3 },
    );

    print!("{}", String::from_utf8_lossy(&diff));
    Ok(())
}
