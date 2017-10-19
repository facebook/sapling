// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(never_type)]

extern crate futures;

extern crate futures_ext;
extern crate linknodes;
extern crate mercurial_types;

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::Hash;
use std::mem;
use std::ptr;
use std::sync::Mutex;

use futures::future::{err, ok, FutureResult};
use linknodes::{Error as LinknodeError, ErrorKind as LinknodeErrorKind, Linknodes};
use mercurial_types::{NodeHash, RepoPath};

pub struct MemLinknodes {
    linknodes: Mutex<HashMap<(RepoPath, NodeHash), NodeHash>>,
}

impl MemLinknodes {
    pub fn new() -> Self {
        MemLinknodes {
            linknodes: Mutex::new(HashMap::new()),
        }
    }
}

impl Linknodes for MemLinknodes {
    type Get = FutureResult<NodeHash, LinknodeError>;
    type Effect = FutureResult<(), LinknodeError>;

    fn add(&self, path: RepoPath, node: &NodeHash, linknode: &NodeHash) -> Self::Effect {
        let mut linknodes = self.linknodes.lock().unwrap();
        match linknodes.entry((path.clone(), *node)) {
            Entry::Occupied(occupied) => err(
                LinknodeErrorKind::AlreadyExists(path, *node, Some(*occupied.get()), *linknode)
                    .into(),
            ),
            Entry::Vacant(vacant) => {
                vacant.insert(*linknode);
                ok(())
            }
        }
    }

    fn get(&self, path: RepoPath, node: &NodeHash) -> Self::Get {
        let linknodes = self.linknodes.lock().unwrap();
        match get_pair(&linknodes, &path, node) {
            Some(node) => ok(*node),
            None => err(LinknodeErrorKind::NotFound(path.clone(), *node).into()),
        }
    }
}

// Turns (&T, &U) into &(T, U) as cheaply as possible.
// From https://stackoverflow.com/a/46044391/1418918.
fn get_pair<'a, 'b, T, U, V>(
    map: &'a HashMap<(T, U), V>,
    t_val: &'b T,
    u_val: &'b U,
) -> Option<&'a V>
where
    T: Eq + Hash,
    U: Eq + Hash,
{
    let k = unsafe {
        // Use a shallow copy to make t_val and u_val adjacent.
        // IMPORTANT: This bypasses Rust's ownership rules. The only reason this is safe is that
        // destructors on `k` are disabled using the `mem::ManuallyDrop` wrapper right below.
        let k: (T, U) = (ptr::read(t_val), ptr::read(u_val));

        // Make sure never to drop k, even if `get` panics.
        mem::ManuallyDrop::new(k)
    };

    map.get(&k)
}
