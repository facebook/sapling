// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::tree::Link;
use failure::{format_err, Fallible};
use std::collections::{BTreeMap, HashMap};
use types::{Node, PathComponentBuf, RepoPath, RepoPathBuf};

pub trait Store {
    fn get(&self, path: &RepoPath, node: &Node) -> Fallible<BTreeMap<PathComponentBuf, Link>>;

    fn insert(
        &mut self,
        path: RepoPathBuf,
        node: Node,
        data: BTreeMap<PathComponentBuf, Link>,
    ) -> Fallible<()>;
}

pub struct TestStore(HashMap<RepoPathBuf, HashMap<Node, BTreeMap<PathComponentBuf, Link>>>);

impl TestStore {
    pub fn new() -> Self {
        TestStore(HashMap::new())
    }
}

impl Store for TestStore {
    fn get(&self, path: &RepoPath, node: &Node) -> Fallible<BTreeMap<PathComponentBuf, Link>> {
        let result = self
            .0
            .get(path)
            .and_then(|node_hash| node_hash.get(node))
            .map(|link| link.clone());
        result.ok_or_else(|| format_err!("Could not find manifest entry for ({}, {})", path, node))
    }

    fn insert(
        &mut self,
        path: RepoPathBuf,
        node: Node,
        data: BTreeMap<PathComponentBuf, Link>,
    ) -> Fallible<()> {
        self.0
            .entry(path)
            .or_insert(HashMap::new())
            .insert(node, data);
        Ok(())
    }
}
