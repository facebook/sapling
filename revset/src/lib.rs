// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate asyncmemo;
#[macro_use]
extern crate error_chain;
extern crate futures;
extern crate mercurial_types;
extern crate repoinfo;

use futures::stream::Stream;
use mercurial_types::NodeHash;

mod setcommon;

mod intersectnodestream;
pub use intersectnodestream::IntersectNodeStream;

mod unionnodestream;
pub use unionnodestream::UnionNodeStream;

mod singlenodehash;
pub use singlenodehash::SingleNodeHash;

mod setdifferencenodestream;
pub use setdifferencenodestream::SetDifferenceNodeStream;

pub mod errors;

pub type NodeStream = Stream<Item = NodeHash, Error = errors::Error> + 'static;

#[cfg(test)]
extern crate ascii;
#[cfg(test)]
extern crate blobrepo;
#[cfg(test)]
extern crate branch_even;
#[cfg(test)]
extern crate branch_uneven;
#[cfg(test)]
extern crate branch_wide;
#[cfg(test)]
extern crate linear;

#[cfg(test)]
use ascii::AsciiString;
#[cfg(test)]
use futures::executor::spawn;
#[cfg(test)]
use mercurial_types::Repo;
#[cfg(test)]
use repoinfo::RepoGenCache;
#[cfg(test)]
use std::collections::HashSet;
#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
fn string_to_nodehash(hash: &'static str) -> NodeHash {
    NodeHash::from_ascii_str(&AsciiString::from_ascii(hash)
        .expect("Can't turn string to AsciiString"))
        .expect("Can't turn AsciiString to NodeHash")
}

#[cfg(test)]
/// Accounting for reordering within generations, ensure that a NodeStream gives the expected
/// NodeHashes for testing.
fn assert_node_sequence<I, R>(
    repo_generation: RepoGenCache<R>,
    repo: &Arc<R>,
    hashes: I,
    stream: Box<NodeStream>,
) where
    I: IntoIterator<Item = NodeHash>,
    R: Repo,
{
    let mut nodestream = spawn(stream);
    let mut received_hashes = HashSet::new();

    for expected in hashes {
        // If we pulled it in earlier, we've found it.
        if received_hashes.remove(&expected) {
            continue;
        }


        let mut generation_task = spawn(repo_generation.get(&repo.clone(), expected));
        let expected_generation = match generation_task.wait_future() {
            Ok(gen) => gen,
            Err(e) => panic!("Unexpected error {:?}", e),
        };

        // Keep pulling in hashes until we either find this one, or move on to a new generation
        let mut node_generation = expected_generation;
        while node_generation == expected_generation {
            match nodestream.wait_stream() {
                Some(Ok(hash)) => {
                    if hash == expected {
                        break;
                    }
                    let mut generation_task = spawn(repo_generation.get(&repo.clone(), hash));
                    node_generation = match generation_task.wait_future() {
                        Ok(gen) => gen,
                        Err(e) => panic!("Unexpected error {:?}", e),
                    };
                    if node_generation == expected_generation {
                        received_hashes.insert(hash);
                    }
                }
                Some(Err(e)) => panic!("Unexpected error {:?}", e),
                None => panic!("Unexpected end of stream"),
            };
        }
        assert!(
            node_generation == expected_generation,
            "Did not receive expected node before change of generation"
        );
    }

    assert!(received_hashes.is_empty(), "Too many nodes received");

    assert!(
        if let None = nodestream.wait_stream() {
            true
        } else {
            false
        },
        "Too many nodes received"
    );
}
