/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use structopt::StructOpt;

#[derive(StructOpt)]
struct Cli {
    path: PathBuf,
}

fn main() {
    let args = Cli::from_args();
    println!("{:?}", args.path);
}
