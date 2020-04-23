/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! data_util -- Extract data from EdenAPI responses
//!
//! This program allows querying the contents of
//! an EdenAPI CBOR data response, and extracting
//! the raw data contained therein.

#![deny(warnings)]

use std::fs::File;
use std::io::{prelude::*, stdin, stdout};
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use structopt::StructOpt;

use types::{api::DataResponse, Key, RepoPathBuf, Validity};

#[derive(Debug, StructOpt)]
#[structopt(name = "data_util", about = "Extract data from EdenAPI responses")]
enum Args {
    Ls(LsArgs),
    Cat(CatArgs),
    Check(CheckArgs),
}

#[derive(Debug, StructOpt)]
#[structopt(about = "List the data entries in the response")]
struct LsArgs {
    #[structopt(help = "Input CBOR file (stdin is used if omitted)")]
    input: Option<PathBuf>,
}

#[derive(Debug, StructOpt)]
#[structopt(about = "Get the content of a data entry")]
struct CatArgs {
    #[structopt(help = "Input CBOR file (stdin used if omitted)")]
    input: Option<PathBuf>,
    #[structopt(long, short, help = "Output file (stdout used if omitted)")]
    output: Option<PathBuf>,
    #[structopt(long, short, help = "Path of desired data entry")]
    path: String,
    #[structopt(long, short, help = "Node hash of desired data entry")]
    hgid: String,
}

#[derive(Debug, StructOpt)]
#[structopt(about = "Check the validity of node hashes for all entries")]
struct CheckArgs {
    #[structopt(help = "Input CBOR file (stdin is used if omitted)")]
    input: Option<PathBuf>,
}

fn main() -> Result<()> {
    match Args::from_args() {
        Args::Ls(args) => cmd_ls(args),
        Args::Cat(args) => cmd_cat(args),
        Args::Check(args) => cmd_check(args),
    }
}

fn cmd_ls(args: LsArgs) -> Result<()> {
    let response = read_input(args.input)?;
    for entry in response.entries {
        println!("{}", entry.key());
    }
    Ok(())
}

fn cmd_cat(args: CatArgs) -> Result<()> {
    let path = RepoPathBuf::from_string(args.path)?;
    let hgid = args.hgid.parse()?;
    let key = Key::new(path, hgid);

    let response = read_input(args.input)?;
    let entry = response
        .entries
        .into_iter()
        .find(|entry| entry.key() == &key)
        .ok_or_else(|| anyhow!("Key not found"))?;

    write_output(args.output, &entry.data().0)
}

fn cmd_check(args: CheckArgs) -> Result<()> {
    let response = read_input(args.input)?;
    for entry in response.entries {
        match entry.data().1 {
            Validity::Valid => {}
            Validity::Redacted => {
                println!("{} [Contents redacted]", entry.key());
            }
            Validity::InvalidEmptyPath(e) => {
                println!("{} [Possible flat manifest hash] {}", entry.key(), e);
            }
            Validity::Invalid(e) => {
                println!("{} [Invalid hash] {}", entry.key(), e);
            }
        }
    }
    Ok(())
}

fn read_input(path: Option<PathBuf>) -> Result<DataResponse> {
    Ok(match path {
        Some(path) => {
            eprintln!("Reading from file: {:?}", &path);
            let file = File::open(&path)?;
            serde_cbor::from_reader(file)?
        }
        None => {
            eprintln!("Reading from stdin");
            serde_cbor::from_reader(stdin())?
        }
    })
}

fn write_output(path: Option<PathBuf>, content: &[u8]) -> Result<()> {
    match path {
        Some(path) => {
            eprintln!("Writing to file: {:?}", &path);
            let mut file = File::create(&path)?;
            file.write_all(content)?;
        }
        None => {
            stdout().write_all(content)?;
        }
    }
    Ok(())
}
