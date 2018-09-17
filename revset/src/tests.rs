// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use NodeStream;
use blobrepo::BlobRepo;
use failure::Error;
use futures::{Future, Stream};
use futures::executor::spawn;
use mercurial_types::HgNodeHash;
use mercurial_types::nodehash::HgChangesetId;
use mononoke_types::ChangesetId;
use std::collections::HashSet;
use std::sync::Arc;

pub fn string_to_nodehash(hash: &'static str) -> HgNodeHash {
    HgNodeHash::from_static_str(hash).expect("Can't turn string to HgNodeHash")
}

pub fn string_to_bonsai(repo: &Arc<BlobRepo>, s: &'static str) -> ChangesetId {
    let node = string_to_nodehash(s);
    repo.get_bonsai_from_hg(&HgChangesetId::new(node))
        .wait()
        .unwrap()
        .unwrap()
}

// TODO(stash): remove assert_node_sequence, use assert_changesets_sequence instead
/// Accounting for reordering within generations, ensure that a NodeStream gives the expected
/// NodeHashes for testing.
pub fn assert_node_sequence<I>(repo: &Arc<BlobRepo>, hashes: I, stream: Box<NodeStream>)
where
    I: IntoIterator<Item = HgNodeHash>,
{
    let mut nodestream = spawn(stream);
    let mut received_hashes = HashSet::new();

    for expected in hashes {
        // If we pulled it in earlier, we've found it.
        if received_hashes.remove(&expected) {
            continue;
        }

        let expected_generation = repo.clone()
            .get_generation_number(&HgChangesetId::new(expected))
            .wait()
            .expect("Unexpected error");

        // Keep pulling in hashes until we either find this one, or move on to a new generation
        loop {
            let hash = nodestream
                .wait_stream()
                .expect("Unexpected end of stream")
                .expect("Unexpected error");

            if hash == expected {
                break;
            }

            let node_generation = repo.clone()
                .get_generation_number(&HgChangesetId::new(expected))
                .wait()
                .expect("Unexpected error");

            assert!(
                node_generation == expected_generation,
                "Did not receive expected node {:?} before change of generation from {:?} to {:?}",
                expected,
                node_generation,
                expected_generation,
            );

            received_hashes.insert(hash);
        }
    }

    assert!(
        received_hashes.is_empty(),
        "Too few nodes received: {:?}",
        received_hashes
    );

    let next_node = nodestream.wait_stream();
    assert!(
        next_node.is_none(),
        "Too many nodes received: {:?}",
        next_node.unwrap()
    );
}

pub fn assert_changesets_sequence<I>(
    repo: &Arc<BlobRepo>,
    hashes: I,
    stream: Box<Stream<Item = ChangesetId, Error = Error>>,
) where
    I: IntoIterator<Item = ChangesetId>,
{
    let mut nodestream = spawn(stream);
    let mut received_hashes = HashSet::new();

    for expected in hashes {
        // If we pulled it in earlier, we've found it.
        if received_hashes.remove(&expected) {
            continue;
        }

        let expected_generation = repo.clone()
            .get_generation_number_by_bonsai(&expected)
            .wait()
            .expect("Unexpected error");

        // Keep pulling in hashes until we either find this one, or move on to a new generation
        loop {
            let hash = nodestream
                .wait_stream()
                .expect("Unexpected end of stream")
                .expect("Unexpected error");

            if hash == expected {
                break;
            }

            let node_generation = repo.clone()
                .get_generation_number_by_bonsai(&expected)
                .wait()
                .expect("Unexpected error");

            assert!(
                node_generation == expected_generation,
                "Did not receive expected node {:?} before change of generation from {:?} to {:?}",
                expected,
                node_generation,
                expected_generation,
            );

            received_hashes.insert(hash);
        }
    }

    assert!(
        received_hashes.is_empty(),
        "Too few nodes received: {:?}",
        received_hashes
    );

    let next_node = nodestream.wait_stream();
    assert!(
        next_node.is_none(),
        "Too many nodes received: {:?}",
        next_node.unwrap()
    );
}
