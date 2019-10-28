// Copyright 2019 Facebook, Inc.

//! A simple binary that computes the diff between two files.

use std::fs;
use std::path::PathBuf;
use structopt::StructOpt;
use xdiff::{diff_unified, DiffFile, DiffOpts};

#[derive(Debug, StructOpt)]
#[structopt(name = "diff", about = "A showcase binary for xdiff diff library.")]

struct Opt {
    /// Input file
    #[structopt(parse(from_os_str))]
    file_a: PathBuf,

    /// Output file, stdout if not present
    #[structopt(parse(from_os_str))]
    file_b: PathBuf,
}

fn main() -> Result<(), std::io::Error> {
    let opt = Opt::from_args();

    let a = fs::read(&opt.file_a)?;
    let b = fs::read(&opt.file_b)?;

    let diff = diff_unified(
        Some(DiffFile::new(&opt.file_a.to_string_lossy().as_bytes(), &a)),
        Some(DiffFile::new(&opt.file_b.to_string_lossy().as_bytes(), &b)),
        DiffOpts { context: 3 },
    );

    print!("{}", String::from_utf8_lossy(&diff));
    Ok(())
}
