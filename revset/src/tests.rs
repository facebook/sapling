// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::BlobRepo;
use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use failure::{err_msg, Error};
use futures::executor::spawn;
use futures::{Future, Stream};
use futures_ext::{BoxFuture, FutureExt, StreamExt};
use mercurial_types::nodehash::HgChangesetId;
use mercurial_types::HgNodeHash;
use mononoke_types::{ChangesetId, Generation};
use singlechangesetid::single_changeset_id;
use std::any::Any;
use std::collections::HashMap;
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;
use BonsaiNodeStream;

pub fn string_to_nodehash(hash: &str) -> HgNodeHash {
    HgNodeHash::from_str(hash).expect("Can't turn string to HgNodeHash")
}

pub fn string_to_bonsai(repo: &Arc<BlobRepo>, s: &str) -> ChangesetId {
    let ctx = CoreContext::test_mock();
    let node = string_to_nodehash(s);
    repo.get_bonsai_from_hg(ctx, &HgChangesetId::new(node))
        .wait()
        .unwrap()
        .unwrap()
}

pub struct TestChangesetFetcher {
    repo: Arc<BlobRepo>,
}

impl TestChangesetFetcher {
    pub fn new(repo: Arc<BlobRepo>) -> Self {
        Self { repo }
    }
}

impl ChangesetFetcher for TestChangesetFetcher {
    fn get_generation_number(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
    ) -> BoxFuture<Generation, Error> {
        self.repo
            .get_generation_number_by_bonsai(ctx, &cs_id)
            .and_then(move |genopt| genopt.ok_or_else(|| err_msg(format!("{} not found", cs_id))))
            .boxify()
    }

    fn get_parents(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
    ) -> BoxFuture<Vec<ChangesetId>, Error> {
        self.repo
            .get_changeset_parents_by_bonsai(ctx, &cs_id)
            .boxify()
    }

    fn get_stats(&self) -> HashMap<String, Box<Any>> {
        HashMap::new()
    }
}

pub fn assert_changesets_sequence<I>(
    ctx: CoreContext,
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

        let expected_generation = repo
            .clone()
            .get_generation_number_by_bonsai(ctx.clone(), &expected)
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

            let node_generation = repo
                .clone()
                .get_generation_number_by_bonsai(ctx.clone(), &expected)
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

pub fn get_single_bonsai_streams(
    ctx: CoreContext,
    repo: &Arc<BlobRepo>,
    hashes: &[&str],
) -> Vec<Box<BonsaiNodeStream>> {
    hashes
        .iter()
        .map(|hash| {
            single_changeset_id(ctx.clone(), string_to_bonsai(&repo.clone(), hash), &repo).boxify()
        })
        .collect()
}
