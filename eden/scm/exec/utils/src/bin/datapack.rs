/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use structopt::StructOpt;

use revisionstore::datapack::DataPack;
use revisionstore::datastore::DataStore;
use revisionstore::localstore::ExtStoredPolicy;
use revisionstore::uniondatastore::UnionDataStore;
use types::Key;
use types::Node;
use types::RepoPathBuf;

#[derive(StructOpt)]
struct Cli {
    #[structopt(short = "n", parse(try_from_str = Node::from_str))]
    node: Node,
    #[structopt(short = "p")]
    path: PathBuf,
}

fn main() {
    let args = Cli::from_args();
    let pack = DataPack::new(&args.path, ExtStoredPolicy::Use).unwrap();
    let mut store = UnionDataStore::new();
    store.add(pack);

    let key = Key::new(RepoPathBuf::new(), args.node);
    let result = store.get(&key).unwrap().unwrap();
    println!("{:?}", String::from_utf8_lossy(&result));
}
