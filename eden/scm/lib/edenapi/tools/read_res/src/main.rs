/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! read_res -- Read the content of EdenAPI responses
//!
//! This program allows querying the contents of
//! EdenAPI CBOR data and history responses.

#![deny(warnings)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{prelude::*, stdin, stdout};
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use serde::de::DeserializeOwned;
use structopt::StructOpt;

use types::{
    api::{DataResponse, HistoryResponse},
    Key, Parents, RepoPathBuf, Validity, WireHistoryEntry,
};

#[derive(Debug, StructOpt)]
#[structopt(name = "read_res", about = "Extract data from EdenAPI responses")]
enum Args {
    Ls(LsArgs),
    Cat(CatArgs),
    Check(CheckArgs),
    History(HistoryArgs),
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

#[derive(Debug, StructOpt)]
#[structopt(about = "Read the content of a CBOR history response")]
enum HistoryArgs {
    Ls(HistLsArgs),
    Show(HistShowArgs),
}

#[derive(Debug, StructOpt)]
#[structopt(about = "List files in this history response")]
struct HistLsArgs {
    #[structopt(help = "Input CBOR file (stdin is used if omitted)")]
    input: Option<PathBuf>,
}

#[derive(Debug, StructOpt)]
#[structopt(about = "Show history for a single file")]
struct HistShowArgs {
    #[structopt(help = "Input CBOR file (stdin is used if omitted)")]
    input: Option<PathBuf>,
    #[structopt(long, short, help = "Only show entries for given file")]
    file: Option<String>,
    #[structopt(long, short, help = "Only show number of entries per file")]
    count: bool,
}

fn main() -> Result<()> {
    match Args::from_args() {
        Args::Ls(args) => cmd_ls(args),
        Args::Cat(args) => cmd_cat(args),
        Args::Check(args) => cmd_check(args),
        Args::History(args) => cmd_history(args),
    }
}

fn cmd_ls(args: LsArgs) -> Result<()> {
    let response: DataResponse = read_input(args.input)?;
    for entry in response.entries {
        println!("{}", entry.key());
    }
    Ok(())
}

fn cmd_cat(args: CatArgs) -> Result<()> {
    let path = RepoPathBuf::from_string(args.path)?;
    let hgid = args.hgid.parse()?;
    let key = Key::new(path, hgid);

    let response: DataResponse = read_input(args.input)?;
    let entry = response
        .entries
        .into_iter()
        .find(|entry| entry.key() == &key)
        .ok_or_else(|| anyhow!("Key not found"))?;

    write_output(args.output, &entry.data().0)
}

fn cmd_check(args: CheckArgs) -> Result<()> {
    let response: DataResponse = read_input(args.input)?;
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

fn cmd_history(args: HistoryArgs) -> Result<()> {
    match args {
        HistoryArgs::Ls(args) => cmd_history_ls(args),
        HistoryArgs::Show(args) => cmd_history_show(args),
    }
}

fn cmd_history_ls(args: HistLsArgs) -> Result<()> {
    let response: HistoryResponse = read_input(args.input)?;
    // Deduplicate and sort paths.
    let mut paths = BTreeSet::new();
    for (path, _) in response.entries {
        paths.insert(path.into_string());
    }
    for path in paths {
        println!("{}", path);
    }
    Ok(())
}

fn cmd_history_show(args: HistShowArgs) -> Result<()> {
    let response: HistoryResponse = read_input(args.input)?;
    let map = make_history_map(response);
    match args.file {
        Some(ref path) => match map.get(path) {
            Some(entries) => print_history(path, entries, args.count),
            None => println!("Path not found in input: {}", path),
        },
        None => {
            for (path, entries) in &map {
                print_history(path, entries, args.count);
            }
        }
    }
    Ok(())
}

fn make_history_map(response: HistoryResponse) -> BTreeMap<String, Vec<WireHistoryEntry>> {
    let mut map = BTreeMap::new();
    for (path, entry) in response.entries {
        map.entry(path.into_string())
            .or_insert_with(Vec::new)
            .push(entry);
    }
    map
}

fn print_history(path: &str, entries: &[WireHistoryEntry], counts_only: bool) {
    if counts_only {
        println!("{}: {}", path, entries.len());
    } else {
        println!("{}:", path);
        for entry in entries {
            println!("  node: {}", entry.node);
            let parents = match entry.parents {
                Parents::Two(p1, p2) => format!("{} {}", p1, p2),
                Parents::One(p1) => format!("{}", p1),
                Parents::None => "None".to_string(),
            };
            println!("  parents: {}", parents);
            println!("  linknode: {}", entry.linknode);
            if let Some(path) = &entry.copyfrom {
                println!("  copyfrom: {}", path);
            }
            println!()
        }
        println!()
    }
}

fn read_input<T: DeserializeOwned>(path: Option<PathBuf>) -> Result<T> {
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
