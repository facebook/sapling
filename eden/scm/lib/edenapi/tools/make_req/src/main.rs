/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! make_req - Make EdenAPI CBOR request payloads
//!
//! This program translates human-editable JSON files into valid
//! CBOR EdenAPI request payloads, which can be used alongside tools
//! like curl to send test requests to the EdenAPI server. This
//! is primarily useful for integration tests and ad-hoc testing.

#![deny(warnings)]

use std::fmt::Debug;
use std::fs::File;
use std::io::{prelude::*, stdin, stdout};
use std::path::PathBuf;

use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use structopt::StructOpt;

use edenapi_types::{json::FromJson, CompleteTreeRequest, DataRequest, HistoryRequest};

#[derive(Debug, StructOpt)]
#[structopt(name = "make_req", about = "Make EdenAPI CBOR request payloads")]
enum Command {
    Data(Args),
    History(Args),
    Tree(Args),
}

#[derive(Debug, StructOpt)]
struct Args {
    #[structopt(long, short, help = "Input JSON file (stdin is used if omitted)")]
    input: Option<PathBuf>,
    #[structopt(long, short, help = "Output CBOR file (stdout is used if omitted)")]
    output: Option<PathBuf>,
}

fn main() -> Result<()> {
    match Command::from_args() {
        Command::Data(args) => make_req::<DataRequest>(args),
        Command::History(args) => make_req::<HistoryRequest>(args),
        Command::Tree(args) => make_req::<CompleteTreeRequest>(args),
    }
}

fn make_req<R: FromJson + Serialize + Debug>(args: Args) -> Result<()> {
    let json = read_input(args.input)?;
    let req = R::from_json(&json)?;
    let bytes = serde_cbor::to_vec(&req)?;
    eprintln!("Generated request: {:#?}", &req);
    write_output(args.output, &bytes)
}

fn read_input(path: Option<PathBuf>) -> Result<Value> {
    Ok(match path {
        Some(path) => {
            eprintln!("Reading from file: {:?}", &path);
            let file = File::open(&path)?;
            serde_json::from_reader(file)?
        }
        None => {
            eprintln!("Reading from stdin");
            serde_json::from_reader(stdin())?
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
