// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{path::PathBuf, sync::Arc};

use bytes::Bytes;
use failure::Fallible;
use structopt::StructOpt;

use pathmatcher::AlwaysMatcher;
use revisionstore::{datapack::DataPack, datastore::DataStore, uniondatastore::UnionDataStore};
use types::{Key, Node, RepoPath};

#[derive(StructOpt)]
struct Cli {
    #[structopt(short = "n", parse(try_from_str = "Node::from_str"))]
    node: Node,
    #[structopt(
        short = "p",
        default_value = "/var/cache/hgcache/fbsource/packs/manifests"
    )]
    manifest_path: String,
}

fn main() {
    let args = Cli::from_args();
    let store = Arc::new(DataPackStore::new(PathBuf::from(args.manifest_path)).unwrap());
    let manifest = manifest::Tree::durable(store, args.node);

    for (file, _meta) in manifest.files(&AlwaysMatcher::new()).map(|x| x.unwrap()) {
        println!("{}", file);
    }
}

pub struct DataPackStore {
    union_store: UnionDataStore<DataPack>,
}

impl DataPackStore {
    pub fn new(dir: PathBuf) -> Fallible<Self> {
        let dirents = std::fs::read_dir(&dir)?
            .filter_map(|e| match e {
                Err(_) => None,
                Ok(entry) => {
                    let entrypath = entry.path();
                    if entrypath.extension() == Some("datapack".as_ref()) {
                        Some(entrypath.with_extension(""))
                    } else {
                        None
                    }
                }
            })
            .collect::<Vec<std::path::PathBuf>>();

        let mut union_store = UnionDataStore::new();
        for path in dirents {
            let datapack = DataPack::new(path.as_ref())?;
            union_store.add(datapack);
        }
        let store = DataPackStore { union_store };
        Ok(store)
    }
}

impl manifest::TreeStore for DataPackStore {
    fn get(&self, path: &RepoPath, node: Node) -> Fallible<Bytes> {
        let key = Key::new(path.to_owned(), node);
        let result = self.union_store.get(&key)?;
        Ok(Bytes::from(result))
    }

    fn insert(&self, _path: &RepoPath, _node: Node, _value: Bytes) -> Fallible<()> {
        unimplemented!("this binary doesn't do writes yet");
    }
}
