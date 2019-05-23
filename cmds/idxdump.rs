// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// declare dependencies on other crates
extern crate clap;
// mercurial stuff
#[macro_use]
extern crate failure_ext as failure;
// 3rd party command line parser
extern crate mercurial;

// Import symbols from std:: (standard library)
use std::str::FromStr;

// Just need `App` from clap
use crate::failure::Result;
use clap::App;

// Get `Revlog` and `RevIdx` from revlog module.
use mercurial::revlog::{RevIdx, Revlog};

fn run() -> Result<()> {
    // Define command line args and parse command line
    let matches = App::new("idxdump")
        .version("0.0.0")
        .about("dump index entries")
        .args_from_usage(concat!(
            "<IDXFILE>               'index file'\n",
            "[<REV>]                 'revision index'"
        ))
        .get_matches();

    // get path to index file
    let idxpath = matches.value_of("IDXFILE").unwrap();

    // Get optional index of entry within index file to start dumping from
    let revidx: Option<RevIdx> = match matches.value_of("REV").map(FromStr::from_str) {
        Some(Ok(v)) => Some(v),
        Some(Err(err)) => bail_msg!("idx malformed: {:?}", err),
        None => None,
    };

    // Construct a `Revlog` from the index file
    let revlog = match Revlog::from_idx_no_data(idxpath) {
        Ok(revlog) => revlog,
        Err(err) => bail_msg!("failed to load idx {}: {:?}", idxpath, err),
    };

    // Print the header, using its `Debug` implementation
    println!("Header: {:?}", revlog.get_header());

    // Construct an iterator over the revlog index
    let iter = &mut revlog.into_iter();

    // If we're not starting at the first version, seek to the starting point
    if let Some(revidx) = revidx {
        iter.seek(revidx)
    }

    // for each entry, get a parsed index entry, and the sequence number in the iteration
    // (`enumerate()` takes an iterator returning T and turns it into an iterator returning
    // `(usize, T)` tuples)
    for (idx, entry) in iter.enumerate() {
        println!("{:?}: {:?}", revidx.unwrap_or(RevIdx::zero()) + idx, entry)
    }

    Ok(())
}

fn main() {
    if let Err(ref e) = run() {
        println!("Failed: {}", e);

        for e in e.iter_chain() {
            println!("caused by: {}", e);
        }

        std::process::exit(1);
    }
}
