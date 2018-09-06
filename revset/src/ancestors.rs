// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// The ancestors of the current node are itself, plus the union of all ancestors of all parents.
// Have a Vec of current generation nodes - as they're output, push their parents onto the next
// generation Vec. Once current generation Vec is empty, rotate.

use std::collections::{BTreeMap, HashSet};
use std::collections::hash_set::IntoIter;
use std::sync::Arc;

use failure::{err_msg, prelude::*};

use futures::{Async, Poll};
use futures::future::Future;
use futures::stream::{iter_ok, Stream};

use UniqueHeap;
use blobrepo::BlobRepo;
use mercurial_types::HgNodeHash;
use mercurial_types::nodehash::HgChangesetId;
use mononoke_types::Generation;

use IntersectNodeStream;
use NodeStream;
use errors::*;

pub struct AncestorsNodeStream {
    repo: BlobRepo,
    next_generation: BTreeMap<Generation, HashSet<HgNodeHash>>,
    pending_changesets: Box<Stream<Item = (HgNodeHash, Generation), Error = Error> + Send>,
    drain: IntoIter<HgNodeHash>,

    // max heap of all relevant unique generation numbers
    sorted_unique_generations: UniqueHeap<Generation>,
}

fn make_pending(
    repo: BlobRepo,
    hashes: IntoIter<HgNodeHash>,
) -> Box<Stream<Item = (HgNodeHash, Generation), Error = Error> + Send> {
    let size = hashes.size_hint().0;
    let new_repo_changesets = repo.clone();
    let new_repo_gennums = repo.clone();

    Box::new(
        iter_ok::<_, Error>(hashes)
            .map(move |hash| {
                new_repo_changesets
                    .get_changeset_parents(&HgChangesetId::new(hash))
                    .map_err(|err| err.chain_err(ErrorKind::ParentsFetchFailed).into())
            })
            .buffered(size)
            .map(|parents| iter_ok::<_, Error>(parents.into_iter()))
            .flatten()
            .and_then(move |node_cs| {
                new_repo_gennums
                    .get_generation_number(&node_cs)
                    .and_then(move |genopt| {
                        genopt.ok_or_else(|| err_msg(format!("{} not found", node_cs)))
                    })
                    .map(move |gen_id| (*node_cs.as_nodehash(), gen_id))
                    .map_err(|err| err.chain_err(ErrorKind::GenerationFetchFailed).into())
            }),
    )
}

impl AncestorsNodeStream {
    pub fn new(repo: &BlobRepo, hash: HgNodeHash) -> Self {
        let node_set: HashSet<HgNodeHash> = hashset!{hash};
        AncestorsNodeStream {
            repo: repo.clone(),
            next_generation: BTreeMap::new(),
            pending_changesets: make_pending(repo.clone(), node_set.clone().into_iter()),
            drain: node_set.into_iter(),
            sorted_unique_generations: UniqueHeap::new(),
        }
    }

    pub fn boxed(self) -> Box<NodeStream> {
        Box::new(self)
    }
}

impl Stream for AncestorsNodeStream {
    type Item = HgNodeHash;
    type Error = Error;
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // Empty the drain if any - return all items for this generation
        let next_in_drain = self.drain.next();
        if next_in_drain.is_some() {
            return Ok(Async::Ready(next_in_drain));
        }

        // Wait until we've drained pending_changesets - we can't continue until we know about all
        // parents of the just-output generation
        loop {
            match self.pending_changesets.poll()? {
                Async::Ready(Some((hash, generation))) => {
                    self.next_generation
                        .entry(generation)
                        .or_insert_with(HashSet::new)
                        .insert(hash);
                    // insert into our sorted list of generations
                    self.sorted_unique_generations.push(generation);
                }
                Async::NotReady => return Ok(Async::NotReady),
                Async::Ready(None) => break,
            };
        }

        if self.next_generation.is_empty() {
            // All parents output - nothing more to send
            return Ok(Async::Ready(None));
        }

        let highest_generation = self.sorted_unique_generations
            .pop()
            .expect("Expected a non empty heap of generations");
        let current_generation = self.next_generation
            .remove(&highest_generation)
            .expect("Highest generation doesn't exist");
        self.pending_changesets =
            make_pending(self.repo.clone(), current_generation.clone().into_iter());
        self.drain = current_generation.into_iter();
        Ok(Async::Ready(Some(self.drain.next().expect(
            "Cannot create a generation without at least one node hash",
        ))))
    }
}

pub fn common_ancestors<I>(repo: &BlobRepo, nodes: I) -> Box<NodeStream>
where
    I: IntoIterator<Item = HgNodeHash>,
{
    let nodes_iter = nodes
        .into_iter()
        .map({ move |node| AncestorsNodeStream::new(repo, node).boxed() });
    IntersectNodeStream::new(&Arc::new(repo.clone()), nodes_iter).boxed()
}

pub fn greatest_common_ancestor<I>(repo: &BlobRepo, nodes: I) -> Box<NodeStream>
where
    I: IntoIterator<Item = HgNodeHash>,
{
    Box::new(common_ancestors(repo, nodes).take(1))
}

#[cfg(test)]
mod test {
    use super::*;
    use async_unit;
    use fixtures::linear;
    use fixtures::merge_uneven;
    use fixtures::unshared_merge_uneven;
    use tests::assert_node_sequence;
    use tests::string_to_nodehash;

    #[test]
    fn linear_ancestors() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));

            let nodestream = AncestorsNodeStream::new(
                &repo,
                string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157"),
            ).boxed();

            assert_node_sequence(
                &repo,
                vec![
                    string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157"),
                    string_to_nodehash("0ed509bf086fadcb8a8a5384dc3b550729b0fc17"),
                    string_to_nodehash("eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b"),
                    string_to_nodehash("cb15ca4a43a59acff5388cea9648c162afde8372"),
                    string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                    string_to_nodehash("607314ef579bd2407752361ba1b0c1729d08b281"),
                    string_to_nodehash("3e0e761030db6e479a7fb58b12881883f9f8c63f"),
                    string_to_nodehash("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536"),
                ],
                nodestream,
            );
        });
    }

    #[test]
    fn merge_ancestors_from_merge() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(merge_uneven::getrepo(None));

            let nodestream = AncestorsNodeStream::new(
                &repo,
                string_to_nodehash("b47ca72355a0af2c749d45a5689fd5bcce9898c7"),
            ).boxed();

            assert_node_sequence(
                &repo,
                vec![
                    string_to_nodehash("b47ca72355a0af2c749d45a5689fd5bcce9898c7"),
                    string_to_nodehash("264f01429683b3dd8042cb3979e8bf37007118bc"),
                    string_to_nodehash("5d43888a3c972fe68c224f93d41b30e9f888df7c"),
                    string_to_nodehash("fc2cef43395ff3a7b28159007f63d6529d2f41ca"),
                    string_to_nodehash("bc7b4d0f858c19e2474b03e442b8495fd7aeef33"),
                    string_to_nodehash("795b8133cf375f6d68d27c6c23db24cd5d0cd00f"),
                    string_to_nodehash("4f7f3fd428bec1a48f9314414b063c706d9c1aed"),
                    string_to_nodehash("16839021e338500b3cf7c9b871c8a07351697d68"),
                    string_to_nodehash("1d8a907f7b4bf50c6a09c16361e2205047ecc5e5"),
                    string_to_nodehash("b65231269f651cfe784fd1d97ef02a049a37b8a0"),
                    string_to_nodehash("d7542c9db7f4c77dab4b315edd328edf1514952f"),
                    string_to_nodehash("3cda5c78aa35f0f5b09780d971197b51cad4613a"),
                    string_to_nodehash("15c40d0abc36d47fb51c8eaec51ac7aad31f669c"),
                ],
                nodestream,
            );
        });
    }

    #[test]
    fn merge_ancestors_one_branch() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(merge_uneven::getrepo(None));

            let nodestream = AncestorsNodeStream::new(
                &repo,
                string_to_nodehash("16839021e338500b3cf7c9b871c8a07351697d68"),
            ).boxed();

            assert_node_sequence(
                &repo,
                vec![
                    string_to_nodehash("16839021e338500b3cf7c9b871c8a07351697d68"),
                    string_to_nodehash("1d8a907f7b4bf50c6a09c16361e2205047ecc5e5"),
                    string_to_nodehash("3cda5c78aa35f0f5b09780d971197b51cad4613a"),
                    string_to_nodehash("15c40d0abc36d47fb51c8eaec51ac7aad31f669c"),
                ],
                nodestream,
            );
        });
    }

    #[test]
    fn unshared_merge_all() {
        async_unit::tokio_unit_test(|| {
            // The unshared_merge_uneven fixture has a commit after the merge. Pull in everything
            // by starting at the head and working back to the original unshared history commits
            let repo = Arc::new(unshared_merge_uneven::getrepo(None));

            let nodestream = AncestorsNodeStream::new(
                &repo,
                string_to_nodehash("339ec3d2a986d55c5ac4670cca68cf36b8dc0b82)"),
            ).boxed();

            assert_node_sequence(
                &repo,
                vec![
                    string_to_nodehash("339ec3d2a986d55c5ac4670cca68cf36b8dc0b82)"),
                    string_to_nodehash("396c60c14337b31ffd0b6aa58a026224713dc07d)"),
                    string_to_nodehash("64011f64aaf9c2ad2e674f57c033987da4016f51"),
                    string_to_nodehash("c1d5375bf73caab8725d759eaca56037c725c7d1"),
                    string_to_nodehash("e819f2dd9a01d3e63d9a93e298968df275e6ad7c"),
                    string_to_nodehash("5a3e8d5a475ec07895e64ec1e1b2ec09bfa70e4e"),
                    string_to_nodehash("76096af83f52cc9a225ccfd8ddfb05ea18132343"),
                    string_to_nodehash("33fb49d8a47b29290f5163e30b294339c89505a2"),
                    string_to_nodehash("03b0589d9788870817d03ce7b87516648ed5b33a"),
                    string_to_nodehash("2fa8b4ee6803a18db4649a3843a723ef1dfe852b"),
                    string_to_nodehash("f01e186c165a2fbe931fd1bf4454235398c591c9"),
                    string_to_nodehash("163adc0d0f5d2eb0695ca123addcb92bab202096"),
                    string_to_nodehash("0b94a2881dda90f0d64db5fae3ee5695a38e7c8f"),
                    string_to_nodehash("eee492dcdeaae18f91822c4359dd516992e0dbcd"),
                    string_to_nodehash("f61fdc0ddafd63503dcd8eed8994ec685bfc8941"),
                    string_to_nodehash("3775a86c64cceeaf68ffe3f012fc90774c42002b"),
                    string_to_nodehash("36ff88dd69c9966c9fad9d6d0457c52153039dde"),
                    string_to_nodehash("1700524113b1a3b1806560341009684b4378660b"),
                    string_to_nodehash("9d374b7e8180f933e3043ad1ffab0a9f95e2bac6"),
                ],
                nodestream,
            );
        });
    }

    #[test]
    fn no_common_ancestor() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(unshared_merge_uneven::getrepo(None));

            let nodestream = greatest_common_ancestor(
                &repo,
                vec![
                    string_to_nodehash("64011f64aaf9c2ad2e674f57c033987da4016f51"),
                    string_to_nodehash("1700524113b1a3b1806560341009684b4378660b"),
                ],
            );
            assert_node_sequence(&repo, vec![], nodestream);
        });
    }

    #[test]
    fn greatest_common_ancestor_different_branches() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(merge_uneven::getrepo(None));

            let nodestream = greatest_common_ancestor(
                &repo,
                vec![
                    string_to_nodehash("4f7f3fd428bec1a48f9314414b063c706d9c1aed"),
                    string_to_nodehash("3cda5c78aa35f0f5b09780d971197b51cad4613a"),
                ],
            );
            assert_node_sequence(
                &repo,
                vec![
                    string_to_nodehash("15c40d0abc36d47fb51c8eaec51ac7aad31f669c"),
                ],
                nodestream,
            );
        });
    }

    #[test]
    fn greatest_common_ancestor_same_branch() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(merge_uneven::getrepo(None));

            let nodestream = greatest_common_ancestor(
                &repo,
                vec![
                    string_to_nodehash("4f7f3fd428bec1a48f9314414b063c706d9c1aed"),
                    string_to_nodehash("264f01429683b3dd8042cb3979e8bf37007118bc"),
                ],
            );
            assert_node_sequence(
                &repo,
                vec![
                    string_to_nodehash("4f7f3fd428bec1a48f9314414b063c706d9c1aed"),
                ],
                nodestream,
            );
        });
    }

    #[test]
    fn all_common_ancestors_different_branches() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(merge_uneven::getrepo(None));

            let nodestream = common_ancestors(
                &repo,
                vec![
                    string_to_nodehash("4f7f3fd428bec1a48f9314414b063c706d9c1aed"),
                    string_to_nodehash("3cda5c78aa35f0f5b09780d971197b51cad4613a"),
                ],
            );
            assert_node_sequence(
                &repo,
                vec![
                    string_to_nodehash("15c40d0abc36d47fb51c8eaec51ac7aad31f669c"),
                ],
                nodestream,
            );
        });
    }

    #[test]
    fn all_common_ancestors_same_branch() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(merge_uneven::getrepo(None));

            let nodestream = common_ancestors(
                &repo,
                vec![
                    string_to_nodehash("4f7f3fd428bec1a48f9314414b063c706d9c1aed"),
                    string_to_nodehash("264f01429683b3dd8042cb3979e8bf37007118bc"),
                ],
            );
            assert_node_sequence(
                &repo,
                vec![
                    string_to_nodehash("4f7f3fd428bec1a48f9314414b063c706d9c1aed"),
                    string_to_nodehash("b65231269f651cfe784fd1d97ef02a049a37b8a0"),
                    string_to_nodehash("d7542c9db7f4c77dab4b315edd328edf1514952f"),
                    string_to_nodehash("15c40d0abc36d47fb51c8eaec51ac7aad31f669c"),
                ],
                nodestream,
            );
        });
    }
}
