// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use clap::App;
use failure_ext::{bail_msg, Error, Result, ResultExt};
use mercurial_revlog::revlog::Revlog;
use std::{fs::File, io::Write, str::FromStr};

fn run() -> Result<()> {
    // Define command line args and parse command line
    let matches = App::new("dumprev")
        .version("0.0.0")
        .about("extract a revision from a revlog")
        .args_from_usage(concat!(
            "-d, --data=[DATAFILE]  'Data file if not inline'\n",
            "-w, --write=[DUMPFILE]  'Write data to file'\n",
            "<IDXFILE>               'index file'\n",
            "<REV>                   'revision index'"
        ))
        .get_matches();
    // Get path of index file; `unwrap()` is safe because parameter is non-optional
    let idxpath = matches.value_of("IDXFILE").unwrap();

    // Get optional datapath
    let datapath = matches.value_of("DATAFILE");

    // Also optional dumpfile
    let dumpfile = matches.value_of("write");

    // Get non-optional revision
    let revidx = FromStr::from_str(matches.value_of("REV").unwrap())
        .map_err(Error::from)
        .context("idx malformed")?;

    // Construct a `Revlog`
    let revlog =
        Revlog::from_idx_with_data(idxpath, datapath).context("failed to load idx and data")?;
    println!("made revlog {:?}", revlog.get_header());

    let entry = revlog.get_entry(revidx).context("failed to get entry")?;

    println!("Revlog[{:?}] = {:?}", revidx, entry);
    match revlog.get_rev(revidx) {
        Ok(ref rev) => {
            if entry.nodeid() != rev.nodeid() {
                println!(
                    "NOTE: hash mismatch: expected {}, got {}",
                    entry.nodeid(),
                    rev.nodeid()
                )
            }
            let revdata = rev.as_blob().as_slice();
            if let Some(dumpfile) = dumpfile {
                let mut file = match File::create(dumpfile) {
                    Ok(file) => file,
                    Err(err) => bail_msg!("Failed to create file {}: {:?}", dumpfile, err),
                };
                println!("Writing rev {:?} to {}", rev.nodeid(), dumpfile);
                if let Err(err) = file.write_all(revdata) {
                    bail_msg!("Failed to write {}: {:?}", dumpfile, err);
                }
            } else {
                println!(
                    "rev {:?}:\n{}",
                    rev.nodeid(),
                    String::from_utf8_lossy(revdata)
                );
            }
        }
        Err(err) => bail_msg!("failed to get chunk {:?}: {}", revidx, err),
    };

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
