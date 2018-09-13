// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate crypto;

use bytes::Bytes;

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::iter;
use std::mem;
use std::sync::{Arc, Mutex};

use failure::Error;
use futures::future::Future;
use futures::stream;
use futures_ext::StreamExt;

use self::crypto::digest::Digest;
use self::crypto::sha2::Sha256;

use super::repo::BlobRepo;
use filenodes::FilenodeInfo;
use mercurial_types::{HgChangesetId, HgFileNodeId, RepoPath};

pub fn get_sha256_alias(contents: &Bytes) -> String {
    let mut hasher = Sha256::new();
    hasher.input(contents);
    let output = hasher.result_str();
    let alias_key = format!("alias.sha256.{}", output);
    alias_key
}

#[derive(Clone, Debug)]
pub struct IncompleteFilenodeInfo {
    pub path: RepoPath,
    pub filenode: HgFileNodeId,
    pub p1: Option<HgFileNodeId>,
    pub p2: Option<HgFileNodeId>,
    pub copyfrom: Option<(RepoPath, HgFileNodeId)>,
}

impl IncompleteFilenodeInfo {
    pub fn with_linknode(self, linknode: HgChangesetId) -> FilenodeInfo {
        let IncompleteFilenodeInfo {
            path,
            filenode,
            p1,
            p2,
            copyfrom,
        } = self;
        FilenodeInfo {
            path,
            filenode,
            p1,
            p2,
            copyfrom,
            linknode,
        }
    }
}

#[derive(Clone, Debug)]
pub struct IncompleteFilenodes {
    filenodes: Arc<Mutex<Vec<IncompleteFilenodeInfo>>>,
}

impl IncompleteFilenodes {
    pub fn new() -> Self {
        IncompleteFilenodes {
            filenodes: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn add(&self, filenode: IncompleteFilenodeInfo) {
        let mut filenodes = self.filenodes.lock().expect("lock poisoned");
        filenodes.push(filenode);
    }

    pub fn upload(
        &self,
        cs_id: HgChangesetId,
        repo: &BlobRepo,
    ) -> impl Future<Item = HgChangesetId, Error = Error> + Send {
        let filenodes = {
            let mut filenodes = self.filenodes.lock().expect("lock poisoned");
            mem::replace(&mut *filenodes, Vec::new())
        }.into_iter()
            .map({
                cloned!(cs_id);
                move |node_info| node_info.with_linknode(cs_id)
            });
        repo.get_filenodes()
            .add_filenodes(stream::iter_ok(filenodes).boxify(), &repo.get_repoid())
            .map(move |_| cs_id)
    }
}

/// Sort nodes of DAG topologically. Implemented as depth-first search with tail-call
/// eliminated. Complexity: `O(N)` from number of nodes.
pub fn sort_topological<T>(dag: &HashMap<T, Vec<T>>) -> Option<Vec<T>>
where
    T: Clone + Eq + Hash,
{
    enum Mark {
        Temporary,
        Marked,
    }

    enum Action<T> {
        Visit(T),
        Mark(T),
    }

    let mut marks = HashMap::new();
    let mut stack = Vec::new();
    let mut output = Vec::new();
    for node in dag.iter()
        .flat_map(|(n, ns)| iter::once(n).chain(ns))
        .collect::<HashSet<_>>()
    {
        stack.push(Action::Visit(node));
        while let Some(action) = stack.pop() {
            match action {
                Action::Visit(node) => {
                    if let Some(mark) = marks.get(node) {
                        match mark {
                            Mark::Temporary => return None, // cycle
                            Mark::Marked => continue,
                        }
                    }
                    marks.insert(node, Mark::Temporary);
                    stack.push(Action::Mark(node));
                    if let Some(children) = dag.get(node) {
                        for child in children {
                            stack.push(Action::Visit(child));
                        }
                    }
                }
                Action::Mark(node) => {
                    marks.insert(node, Mark::Marked);
                    output.push(node.clone());
                }
            }
        }
    }

    output.reverse();
    Some(output)
}

#[cfg(test)]
mod test {

    use super::*;

    #[test]
    fn sort_topological_test() {
        let res = sort_topological(&hashmap!{1 => vec![2]});
        assert_eq!(Some(vec![1, 2]), res);

        let res = sort_topological(&hashmap!{1 => vec![1]});
        assert_eq!(None, res);

        let res = sort_topological(&hashmap!{1 => vec![2], 2 => vec![3]});
        assert_eq!(Some(vec![1, 2, 3]), res);

        let res = sort_topological(&hashmap!{1 => vec![2, 3], 2 => vec![3]});
        assert_eq!(Some(vec![1, 2, 3]), res);

        let res = sort_topological(&hashmap!{1 => vec![2, 3], 2 => vec![4], 3 => vec![4]});
        assert!(Some(vec![1, 2, 3, 4]) == res || Some(vec![1, 3, 2, 4]) == res);
    }
}
