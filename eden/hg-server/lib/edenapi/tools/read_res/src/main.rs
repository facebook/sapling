/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! read_res -- Read the content of EdenAPI responses
//!
//! This program allows querying the contents of
//! EdenAPI CBOR file, tree, and history responses.

#![deny(warnings)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{prelude::*, stdin, stdout};
use std::path::PathBuf;

use anyhow::{anyhow, Result};
use serde::de::{Deserialize, DeserializeOwned};
use serde_cbor::Deserializer;
use sha1::{Digest, Sha1};
use structopt::StructOpt;

use edenapi_types::{
    wire::{
        ToApi, WireBookmarkEntry, WireCloneData, WireCommitHashToLocationResponse,
        WireCommitLocationToHashResponse, WireFileEntry, WireHistoryResponseChunk, WireIdMapEntry,
        WireTreeEntry,
    },
    CommitRevlogData, FileError, TreeError, WireHistoryEntry,
};
use types::{HgId, Key, Parents, RepoPathBuf};

#[derive(Debug, StructOpt)]
#[structopt(name = "read_res", about = "Read the content of EdenAPI responses")]
enum Args {
    Tree(TreeArgs),
    File(FileArgs),
    History(HistoryArgs),
    CommitRevlogData(CommitRevlogDataArgs),
    CommitLocationToHash(CommitLocationToHashArgs),
    CommitHashToLocation(CommitHashToLocationArgs),
    Clone(CloneArgs),
    FullIdmapClone(CloneArgs),
    Bookmark(BookmarkArgs),
}

#[derive(Debug, StructOpt)]
#[structopt(about = "Read the content of a CBOR tree response")]
enum TreeArgs {
    Ls(DataLsArgs),
    Cat(DataCatArgs),
    Check(DataCheckArgs),
}

#[derive(Debug, StructOpt)]
#[structopt(about = "Read the content of a CBOR file response")]
enum FileArgs {
    Ls(DataLsArgs),
    Cat(DataCatArgs),
    Check(DataCheckArgs),
}

#[derive(Debug, StructOpt)]
#[structopt(about = "List the file or tree entries in the response")]
struct DataLsArgs {
    #[structopt(help = "Input CBOR file (stdin is used if omitted)")]
    input: Option<PathBuf>,
    #[structopt(long, short, help = "Only look at the first N entries")]
    limit: Option<usize>,
}

#[derive(Debug, StructOpt)]
#[structopt(about = "Get the content of an entry")]
struct DataCatArgs {
    #[structopt(help = "Input CBOR file (stdin used if omitted)")]
    input: Option<PathBuf>,
    #[structopt(long, short, help = "Output file (stdout used if omitted)")]
    output: Option<PathBuf>,
    #[structopt(long, short, help = "Path of desired data entry")]
    path: String,
    #[structopt(long, short, help = "Node hash of desired data entry")]
    hgid: String,
    #[structopt(long, short, help = "Only look at the first N entries")]
    limit: Option<usize>,
    #[structopt(long, short, help = "Debug print entire message instead of just data")]
    debug: bool,
}

#[derive(Debug, StructOpt)]
#[structopt(about = "Check the validity of node hashes for all entries")]
struct DataCheckArgs {
    #[structopt(help = "Input CBOR file (stdin is used if omitted)")]
    input: Option<PathBuf>,
    #[structopt(long, short, help = "Only look at the first N entries")]
    limit: Option<usize>,
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
    #[structopt(long, short, help = "Only look at the first N entries")]
    limit: Option<usize>,
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
    #[structopt(long, short, help = "Only look at the first N entries")]
    limit: Option<usize>,
}

#[derive(Debug, StructOpt)]
#[structopt(about = "Read the content of a CBOR commit data response")]
enum CommitRevlogDataArgs {
    Ls(CommitRevlogDataLsArgs),
    Show(CommitRevlogDataShowArgs),
    Check(CommitRevlogDataCheckArgs),
}

#[derive(Debug, StructOpt)]
#[structopt(about = "List hashes in a CommitRevlogData response")]
struct CommitRevlogDataLsArgs {
    #[structopt(help = "Input CBOR file (stdin is used if omitted)")]
    input: Option<PathBuf>,
    #[structopt(long, short, help = "Only look at the first N entries")]
    limit: Option<usize>,
}

#[derive(Debug, StructOpt)]
#[structopt(about = "Show entry for a single commit id")]
struct CommitRevlogDataShowArgs {
    #[structopt(help = "Input CBOR file (stdin is used if omitted)")]
    input: Option<PathBuf>,
    #[structopt(long, short, help = "Output file (stdout used if omitted)")]
    output: Option<PathBuf>,
    #[structopt(long, short, help = "HgId of desired commit revlog data")]
    hgid: String,
    #[structopt(long, short, help = "Return the contents from start byte onward")]
    start: Option<usize>,
    #[structopt(long, short, help = "Return the contents up to the end byte")]
    end: Option<usize>,
}

#[derive(Debug, StructOpt)]
#[structopt(about = "Checks that the hashes match contents in a CommitRevlogData response")]
struct CommitRevlogDataCheckArgs {
    #[structopt(help = "Input CBOR file (stdin is used if omitted)")]
    input: Option<PathBuf>,
    #[allow(dead_code)]
    #[structopt(long, short, help = "Only look at the first N entries")]
    limit: Option<usize>,
}

#[derive(Debug, StructOpt)]
#[structopt(about = "Read the contents of a commit location-to-hash request")]
struct CommitLocationToHashArgs {
    #[structopt(help = "Input CBOR file (stdin is used if omitted)")]
    input: Option<PathBuf>,
    #[allow(dead_code)]
    #[structopt(long, short, help = "Output file (stdout used if omitted)")]
    output: Option<PathBuf>,
    #[structopt(long, short, help = "Look at items starting with index start")]
    start: Option<usize>,
    #[structopt(long, short, help = "Only look at N entries")]
    limit: Option<usize>,
}

#[derive(Debug, StructOpt)]
#[structopt(about = "Read the contents of a commit location-to-hash request")]
struct CommitHashToLocationArgs {
    #[structopt(help = "Input CBOR file (stdin is used if omitted)")]
    input: Option<PathBuf>,
    #[allow(dead_code)]
    #[structopt(long, short, help = "Output file (stdout used if omitted)")]
    output: Option<PathBuf>,
    #[structopt(long, short, help = "Look at items starting with index start")]
    start: Option<usize>,
    #[structopt(long, short, help = "Only look at N entries")]
    limit: Option<usize>,
}

#[derive(Debug, StructOpt)]
#[structopt(about = "Read the contents of a clone data request")]
struct CloneArgs {
    #[structopt(help = "Input CBOR file (stdin is used if omitted)")]
    input: Option<PathBuf>,
}

#[derive(Debug, StructOpt)]
#[structopt(about = "Read the content of a CBOR bookmark response")]
struct BookmarkArgs {
    #[structopt(help = "Input CBOR file (stdin is used if omitted)")]
    input: Option<PathBuf>,
    #[allow(dead_code)]
    #[structopt(long, short, help = "Output file (stdout used if omitted)")]
    output: Option<PathBuf>,
    #[structopt(long, short, help = "Only look at the first N entries")]
    limit: Option<usize>,
}

fn main() -> Result<()> {
    match Args::from_args() {
        Args::Tree(args) => cmd_tree(args),
        Args::File(args) => cmd_file(args),
        Args::History(args) => cmd_history(args),
        Args::CommitRevlogData(args) => cmd_commit_revlog_data(args),
        Args::CommitLocationToHash(args) => cmd_commit_location_to_hash(args),
        Args::CommitHashToLocation(args) => cmd_commit_hash_to_location(args),
        Args::Clone(args) => cmd_clone(args),
        Args::FullIdmapClone(args) => cmd_full_idmap_clone(args),
        Args::Bookmark(args) => cmd_bookmark(args),
    }
}

fn cmd_tree(args: TreeArgs) -> Result<()> {
    match args {
        TreeArgs::Ls(args) => cmd_tree_ls(args),
        TreeArgs::Cat(args) => cmd_tree_cat(args),
        TreeArgs::Check(args) => cmd_tree_check(args),
    }
}

fn cmd_file(args: FileArgs) -> Result<()> {
    match args {
        FileArgs::Ls(args) => cmd_file_ls(args),
        FileArgs::Cat(args) => cmd_file_cat(args),
        FileArgs::Check(args) => cmd_file_check(args),
    }
}

fn cmd_tree_ls(args: DataLsArgs) -> Result<()> {
    let entries: Vec<WireTreeEntry> = read_input(args.input, args.limit)?;
    for entry in entries.into_iter().filter_map(to_api) {
        println!("{}", entry?.key());
    }
    Ok(())
}

fn cmd_tree_cat(args: DataCatArgs) -> Result<()> {
    let path = RepoPathBuf::from_string(args.path)?;
    let hgid = args.hgid.parse()?;
    let key = Key::new(path, hgid);

    let entries: Vec<WireTreeEntry> = read_input(args.input, args.limit)?;
    let entry = entries
        .into_iter()
        .filter_map(to_api)
        .filter_map(|r| r.ok())
        .find(|entry| entry.key() == &key)
        .ok_or_else(|| anyhow!("Key not found"))?;

    if args.debug {
        write_output(args.output, format!("{:?}\n", &entry))
    } else {
        write_output(args.output, &entry.data()?)
    }
}

fn cmd_tree_check(args: DataCheckArgs) -> Result<()> {
    let entries: Vec<WireTreeEntry> = read_input(args.input, args.limit)?;
    for entry in entries.into_iter().filter_map(to_api) {
        let entry = entry?;
        match entry.data() {
            Ok(_) => {}
            Err(TreeError::MaybeHybridManifest(e)) => {
                println!("{} [Possible flat manifest hash] {}", entry.key(), e);
            }
            Err(TreeError::Corrupt(e)) => {
                println!("{} [Invalid hash] {}", entry.key(), e);
            }
            Err(TreeError::MissingField(e)) => {
                println!("{} [Missing field] {}", entry.key(), e);
            }
        }
    }
    Ok(())
}

fn cmd_file_ls(args: DataLsArgs) -> Result<()> {
    let entries: Vec<WireFileEntry> = read_input(args.input, args.limit)?;
    for entry in entries.into_iter().filter_map(to_api) {
        println!("{}", entry.key());
    }
    Ok(())
}

fn cmd_file_cat(args: DataCatArgs) -> Result<()> {
    let path = RepoPathBuf::from_string(args.path)?;
    let hgid = args.hgid.parse()?;
    let key = Key::new(path, hgid);

    let entries: Vec<WireFileEntry> = read_input(args.input, args.limit)?;
    let entry = entries
        .into_iter()
        .filter_map(to_api)
        .find(|entry| entry.key() == &key)
        .ok_or_else(|| anyhow!("Key not found"))?;

    if args.debug {
        write_output(args.output, format!("{:?}\n", &entry))
    } else {
        write_output(args.output, &entry.data()?)
    }
}

fn cmd_file_check(args: DataCheckArgs) -> Result<()> {
    let entries: Vec<WireFileEntry> = read_input(args.input, args.limit)?;
    for entry in entries.into_iter().filter_map(to_api) {
        match entry.data() {
            Ok(_) => {}
            Err(FileError::Corrupt(e)) => {
                println!("{} [Invalid hash] {}", entry.key(), e);
            }
            Err(FileError::Redacted(..)) => {
                println!("{} [Contents redacted]", entry.key());
            }
            Err(FileError::Lfs(..)) => {
                println!("{} [LFS pointer]", entry.key());
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
    let chunks: Vec<WireHistoryResponseChunk> = read_input(args.input, args.limit)?;
    // Deduplicate and sort paths.
    let mut paths = BTreeSet::new();
    for chunk in chunks.into_iter().filter_map(to_api) {
        paths.insert(chunk.path.into_string());
    }
    for path in paths {
        println!("{}", path);
    }
    Ok(())
}

fn cmd_history_show(args: HistShowArgs) -> Result<()> {
    let chunks: Vec<WireHistoryResponseChunk> = read_input(args.input, args.limit)?;
    let map = make_history_map(chunks);
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

fn cmd_commit_revlog_data(args: CommitRevlogDataArgs) -> Result<()> {
    match args {
        CommitRevlogDataArgs::Ls(args) => cmd_commit_revlog_data_ls(args),
        CommitRevlogDataArgs::Show(args) => cmd_commit_revlog_data_show(args),
        CommitRevlogDataArgs::Check(args) => cmd_commit_revlog_data_check(args),
    }
}

fn cmd_commit_revlog_data_ls(args: CommitRevlogDataLsArgs) -> Result<()> {
    let commit_revlog_data_list: Vec<CommitRevlogData> = read_input(args.input, args.limit)?;
    for crd in commit_revlog_data_list {
        println!("{}", crd.hgid);
    }
    Ok(())
}

fn cmd_commit_revlog_data_show(args: CommitRevlogDataShowArgs) -> Result<()> {
    let commit_revlog_data_list: Vec<CommitRevlogData> = read_input(args.input, None)?;
    let hgid: HgId = args.hgid.parse()?;
    let bytes = commit_revlog_data_list
        .into_iter()
        .find(|crd| crd.hgid == hgid)
        .map(|crd| crd.revlog_data)
        .ok_or_else(|| anyhow!("HgId not found"))?;
    let start_bound = args.start.unwrap_or(0);
    let end_bound = args.end.unwrap_or_else(|| bytes.len());
    write_output(args.output, &bytes[start_bound..end_bound])?;
    Ok(())
}

fn cmd_commit_revlog_data_check(args: CommitRevlogDataCheckArgs) -> Result<()> {
    let commit_revlog_data_list: Vec<CommitRevlogData> = read_input(args.input, None)?;
    for crd in commit_revlog_data_list {
        let mut hasher = Sha1::new();
        hasher.input(crd.revlog_data);
        let result = HgId::from_byte_array(hasher.result().into());
        let hgid = crd.hgid;
        if result == hgid {
            println!("{} matches", hgid);
        } else {
            println!("ERROR. expected '{}' but got '{}'", hgid, result);
        }
    }
    Ok(())
}

fn cmd_commit_location_to_hash(args: CommitLocationToHashArgs) -> Result<()> {
    let commit_location_to_hash_response: Vec<WireCommitLocationToHashResponse> =
        read_input(args.input, None)?;
    let iter = commit_location_to_hash_response
        .iter()
        .skip(args.start.unwrap_or(0))
        .take(args.limit.unwrap_or(usize::MAX));
    for response in iter {
        println!(
            "LocationToHashRequest(known={}, dist={}, count={})",
            response.location.descendant, response.location.distance, response.count
        );
        for hgid in response.hgids.iter() {
            println!("  {}", hgid);
        }
    }
    Ok(())
}

fn cmd_commit_hash_to_location(args: CommitHashToLocationArgs) -> Result<()> {
    let mut response_list: Vec<WireCommitHashToLocationResponse> = read_input(args.input, None)?;
    response_list.sort_by_key(|r| r.hgid);
    let iter = response_list
        .iter()
        .skip(args.start.unwrap_or(0))
        .take(args.limit.unwrap_or(usize::MAX));
    for response in iter {
        println!(
            "{} =>\n    Location(descendant={}, dist={})",
            response.hgid, response.location.descendant, response.location.distance
        );
    }
    Ok(())
}

fn cmd_clone(args: CloneArgs) -> Result<()> {
    let mut wire_clone_data: Vec<WireCloneData> = read_input(args.input, None)?;
    let clone_data = wire_clone_data
        .pop()
        .ok_or_else(|| anyhow!("empty response for clone data"))?
        .to_api()?;
    println!("head_id: {}", clone_data.head_id);
    println!("flat_segments: [");
    for fs in clone_data.flat_segments.segments {
        println!(
            "  {}, {}, [{}]",
            fs.low,
            fs.high,
            fs.parents
                .iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    println!("]");
    let mut idmap_entries = clone_data
        .idmap
        .iter()
        .map(|(k, v)| format!("  {}: {}\n", k, v.to_hex()))
        .collect::<Vec<_>>();
    idmap_entries.sort();
    println!("idmap: {{\n{}}}", idmap_entries.join(""));
    Ok(())
}

fn cmd_full_idmap_clone(args: CloneArgs) -> Result<()> {
    let mut buffer = Vec::new();
    match args.input {
        None => {
            eprintln!("Reading from stdin");
            stdin().read_to_end(&mut buffer)?;
        }
        Some(path) => {
            eprintln!("Reading from file: {:?}", &path);
            let mut file = File::open(&path)?;
            file.read_to_end(&mut buffer)?;
        }
    };
    let mut deserializer = Deserializer::from_slice(&buffer);
    let wire_clone_data = WireCloneData::deserialize(&mut deserializer)?;
    let clone_data = wire_clone_data.to_api()?;
    println!("head_id: {}", clone_data.head_id);
    println!("flat_segments: [");
    for fs in clone_data.flat_segments.segments {
        println!(
            "  {}, {}, [{}]",
            fs.low,
            fs.high,
            fs.parents
                .iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    println!("]");
    let mut idmap_entries = deserializer
        .into_iter::<WireIdMapEntry>()
        .collect::<Result<Vec<_>, _>>()?;
    idmap_entries.sort_by(|x, y| x.dag_id.cmp(&y.dag_id));
    let idmap_entries = idmap_entries
        .into_iter()
        .map(|e| {
            Ok(format!(
                "  {}: {}\n",
                e.dag_id.to_api()?,
                e.hg_id.to_api()?.to_hex()
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    println!("idmap: {{\n{}}}", idmap_entries.join(""));
    Ok(())
}

fn cmd_bookmark(args: BookmarkArgs) -> Result<()> {
    let entries: Vec<WireBookmarkEntry> = read_input(args.input, args.limit)?;
    for entry in entries.into_iter().filter_map(to_api) {
        print! {"{}: ", entry.bookmark}
        match entry.hgid {
            Some(hgid) => println!("{}", hgid.to_string()),
            None => println!("Bookmark not found"),
        }
    }
    Ok(())
}

fn make_history_map(
    chunks: impl IntoIterator<Item = WireHistoryResponseChunk>,
) -> BTreeMap<String, Vec<WireHistoryEntry>> {
    let mut map = BTreeMap::new();
    for chunk in chunks.into_iter().filter_map(to_api) {
        map.entry(chunk.path.into_string())
            .or_insert_with(Vec::new)
            .extend_from_slice(&chunk.entries);
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

fn read_input<T: DeserializeOwned>(path: Option<PathBuf>, limit: Option<usize>) -> Result<Vec<T>> {
    Ok(match path {
        Some(path) => {
            eprintln!("Reading from file: {:?}", &path);
            let file = File::open(&path)?;
            Deserializer::from_reader(file)
                .into_iter()
                .take(limit.unwrap_or(usize::MAX))
                .collect::<Result<Vec<_>, _>>()?
        }
        None => {
            eprintln!("Reading from stdin");
            Deserializer::from_reader(stdin())
                .into_iter()
                .take(limit.unwrap_or(usize::MAX))
                .collect::<Result<Vec<_>, _>>()?
        }
    })
}

fn to_api<T: ToApi>(entry: T) -> Option<T::Api> {
    match entry.to_api() {
        Ok(api) => Some(api),
        Err(_) => {
            eprintln!("Failed to convert entry to API type");
            None
        }
    }
}

fn write_output(path: Option<PathBuf>, content: impl AsRef<[u8]>) -> Result<()> {
    match path {
        Some(path) => {
            eprintln!("Writing to file: {:?}", &path);
            let mut file = File::create(&path)?;
            file.write_all(content.as_ref())?;
        }
        None => {
            stdout().write_all(content.as_ref())?;
        }
    }
    Ok(())
}
