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

use std::convert::TryFrom;
use std::fs::File;
use std::io::{prelude::*, stdin, stdout};
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{anyhow, ensure, Result};
use serde_json::Value;
use structopt::StructOpt;

use types::{
    api::{DataRequest, HistoryRequest, TreeRequest},
    HgId, Key, RepoPathBuf,
};

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

macro_rules! convert {
    ($args:ident, $parse_fn:ident) => {{
        let json = read_input($args.input)?;
        let req = $parse_fn(&json)?;
        let bytes = serde_cbor::to_vec(&req)?;
        eprintln!("Generated request: {:#?}", &req);
        write_output($args.output, &bytes)
    }};
}

fn main() -> Result<()> {
    match Command::from_args() {
        Command::Data(args) => convert!(args, parse_data_req),
        Command::History(args) => convert!(args, parse_history_req),
        Command::Tree(args) => convert!(args, parse_tree_req),
    }
}

/// Parse a `DataRequest` from JSON.
///
/// The request is represented as a JSON array of path/filenode pairs.
///
/// Example request:
///
///     ```json
///     [
///       ["path/to/file_1", "48f43af456d770b6a78e1ace628319847e05cc24"],
///       ["path/to/file_2", "7dcd6ede35eaaa5b1b16a341b19993e59f9b0dbf"],
///       ["path/to/file_3", "218d708a9f8c3e37cfd7ab916c537449ac5419cd"],
///     ]
///     ```
///
fn parse_data_req(json: &Value) -> Result<DataRequest> {
    Ok(DataRequest {
        keys: parse_keys(json)?,
    })
}

/// Parse a `HistoryRequest` from JSON.
///
/// The request is represented as a JSON object containing a required
/// "keys" field consisting of an array of path/filenode pairs (similar
/// to a data request) as well as an optional depth parameter.
///
/// Example request:
///
///     ```json
///     {
///       "keys": [
///         ["path/to/file_1", "48f43af456d770b6a78e1ace628319847e05cc24"],
///         ["path/to/file_2", "7dcd6ede35eaaa5b1b16a341b19993e59f9b0dbf"],
///         ["path/to/file_3", "218d708a9f8c3e37cfd7ab916c537449ac5419cd"],
///       ],
///       "depth": 1
///     }
///     ```
///
fn parse_history_req(json: &Value) -> Result<HistoryRequest> {
    let json = json
        .as_object()
        .ok_or_else(|| anyhow!("input must be a JSON object"))?;
    let depth = json.get("depth").and_then(|d| d.as_u64()).map(|d| d as u32);
    let keys = {
        let json_keys = json
            .get("keys")
            .ok_or_else(|| anyhow!("missing field: keys"))?;
        parse_keys(json_keys)?
    };

    Ok(HistoryRequest { keys, depth })
}

/// Parse a `TreeRequest` from JSON.
///
/// The request is represented as a JSON object containing the fields
/// needed for a "gettreepack"-style tree request. Note that most
/// EdenAPI tree requests are actually performed using a `DataRequest`
/// for the desired tree nodes; `TreeRequest`s are only used in situations
/// where behavior similar to Mercurial's `gettreepack` wire protocol
/// command is desired.
///
/// Example request:
///
///     ```json
///     {
///         "rootdir": "path/to/root/dir",
///         "mfnodes": [
///             "8722607999fc5ce35e9af56e6da2c823923291dd",
///             "b7d7ffb1a37c86f00558ff132e57c56bca29dc04"
///         ],
///         "basemfnodes": [
///             "26d6acbabf823b844917f04cfbe6747c80983119",
///             "111caaed68164b939f6e2f58680b462ebc3174c7"
///         ],
///         "depth": 1
///     }
///     ```
///
fn parse_tree_req(json: &Value) -> Result<TreeRequest> {
    let obj = json
        .as_object()
        .ok_or_else(|| anyhow!("input must be a JSON object"))?;

    let rootdir = obj
        .get("rootdir")
        .ok_or_else(|| anyhow!("missing field: rootdir"))?;
    let rootdir = rootdir
        .as_str()
        .ok_or_else(|| anyhow!("rootdir field must be a string"))?;
    let rootdir = RepoPathBuf::from_string(rootdir.to_string())?;

    let mfnodes = obj
        .get("mfnodes")
        .ok_or_else(|| anyhow!("missing field: mfnodes"))?;
    let mfnodes = parse_hashes(mfnodes)?;

    let basemfnodes = obj
        .get("basemfnodes")
        .ok_or_else(|| anyhow!("missing field: basemfnodes"))?;
    let basemfnodes = parse_hashes(basemfnodes)?;

    let depth = obj
        .get("depth")
        .and_then(|d| d.as_u64())
        .map(|d| d as usize);

    Ok(TreeRequest {
        rootdir,
        mfnodes,
        basemfnodes,
        depth,
    })
}

fn parse_keys(json: &Value) -> Result<Vec<Key>> {
    let arr = json
        .as_array()
        .ok_or_else(|| anyhow!("input must be a JSON array"))?;

    let mut keys = Vec::new();
    for i in arr.iter() {
        let json_key = i
            .as_array()
            .ok_or_else(|| anyhow!("array items must be [path, hash] arrays"))?;

        ensure!(
            json_key.len() == 2,
            "array items must be [path, hash] arrays"
        );

        // Cast slice into 2-element array reference so we can destructure it.
        let [path, hash] = <&[_; 2]>::try_from(&json_key[..2])?;

        let path = path
            .as_str()
            .ok_or_else(|| anyhow!("path must be a string"))?;
        let hash = hash
            .as_str()
            .ok_or_else(|| anyhow!("hash must be a string"))?;

        let key = make_key(&path, hash)?;
        keys.push(key);
    }

    Ok(keys)
}

fn parse_hashes(json: &Value) -> Result<Vec<HgId>> {
    let array = json
        .as_array()
        .ok_or_else(|| anyhow!("node hashes must be a passed as an array"))?;
    let mut hashes = Vec::new();
    for hex in array {
        let hex = hex
            .as_str()
            .ok_or_else(|| anyhow!("node hashes must be strings"))?;
        let hash = HgId::from_str(hex)?;
        hashes.push(hash);
    }
    Ok(hashes)
}

fn make_key(path: &str, hash: &str) -> Result<Key> {
    let path = if path.is_empty() {
        RepoPathBuf::new()
    } else {
        RepoPathBuf::from_string(path.to_string())?
    };
    let hgid = HgId::from_str(hash)?;
    Ok(Key::new(path, hgid))
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
