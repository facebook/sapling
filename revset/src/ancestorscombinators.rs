// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

/// Union and intersection can be made more efficient if the streams are uninterrupted streams of
/// ancestors. For example:
///
/// A-o   o-B
///    \ /
///     o - C
///     |
///     o
///     |
///    ...
///
/// UnionNodeStream(A, B) would poll both streams until they are exhausted. That means that node C
/// and all of its ancestors would be generated twice. This is not necessary.
/// For IntersectNodeStream(A, B) the problem is even more acute. The stream will return just one
/// entry, however it will generate all ancestors of A and B twice, and there can be lots of them!
///
/// The stream below aims to solve the aforementioned problems. It's primary usage is in
/// Mercurial pull to find commits that need to be sent to a client.
use std::collections::{BTreeMap, HashSet};
use std::collections::hash_set::IntoIter;
use std::iter;
use std::sync::Arc;

use futures::{Async, IntoFuture, Poll};
use futures::future::Future;
use futures::stream::{self, empty, iter_ok, Peekable, Stream};
use futures_ext::{SelectAll, StreamExt};

use blobrepo::BlobRepo;
use mercurial_types::HgNodeHash;
use mercurial_types::nodehash::HgChangesetId;
use mononoke_types::Generation;
use repoinfo::RepoGenCache;

use NodeStream;
use UniqueHeap;
use errors::*;
use setcommon::*;

/// As the name suggests, it's a difference of unions of ancestors of nodes.
/// In mercurial revset's terms it's (::A) - (::B), where A and B are sets of nodes.
/// In Mononoke revset's terms it's equivalent to
///
/// ```
///   let include: Vec<HgNodeHash> = vec![ ... ];
///   let exclude: Vec<HgNodeHash> = vec![ ... ];
///   ...
///   let mut include_ancestors = vec![];
///   for i in include.clone() {
///     include_ancestors.push(
///         AncestorsNodeStream::new(&repo, repo_generation.clone(), i).boxify()
///     );
///   }
///
///   let mut exclude_ancestors = vec![];
///   for i in exclude.clone() {
///     exclude_ancestors.push(
///         AncestorsNodeStream::new(&repo, repo_generation.clone(), i).boxify()
///     );
///   }
///
///   let include_ancestors = UnionNodeStream::new(
///     &repo, repo_generation.clone(), include_ancestors
///   ).boxify();
///   let exclude_ancestors = UnionNodeStream::new(
///     &repo, repo_generation.clone(), exclude_ancestors
///   ).boxify();
///   let expected =
///     SetDifferenceNodeStream::new(
///         &repo, repo_generation.clone(), include_ancestors, exclude_ancestors
///    );
/// ```
///

pub struct DifferenceOfUnionsOfAncestorsNodeStream {
    repo: Arc<BlobRepo>,
    repo_generation: RepoGenCache,

    // Nodes that we know about, grouped by generation.
    next_generation: BTreeMap<Generation, HashSet<HgNodeHash>>,

    // The generation of the nodes in `drain`. All nodes with bigger generation has already been
    // returned
    current_generation: Generation,

    // Parents of entries from `drain`. We fetch generation number for them.
    pending_changesets:
        SelectAll<Box<Stream<Item = (HgNodeHash, Generation), Error = Error> + Send>>,

    // Stream of (Hashset, Generation) that needs to be excluded
    exclude_ancestors: Peekable<stream::Fuse<GroupedByGenenerationStream>>,

    // Nodes which generation is equal to `current_generation`. They will be returned from the
    // stream unless excluded.
    drain: iter::Peekable<IntoIter<HgNodeHash>>,

    // max heap of all relevant unique generation numbers
    sorted_unique_generations: UniqueHeap<Generation>,
}

fn make_pending(
    repo: Arc<BlobRepo>,
    repo_generation: RepoGenCache,
    hash: HgNodeHash,
) -> Box<Stream<Item = (HgNodeHash, Generation), Error = Error> + Send> {
    let new_repo = repo.clone();

    Box::new(
        Ok::<_, Error>(hash)
            .into_future()
            .and_then(move |hash| {
                new_repo
                    .get_changeset_parents(&HgChangesetId::new(hash))
                    .map(|parents| parents.into_iter().map(|cs| cs.into_nodehash()))
                    .map_err(|err| err.context(ErrorKind::ParentsFetchFailed).into())
            })
            .map(|parents| iter_ok::<_, Error>(parents))
            .flatten_stream()
            .and_then(move |node_hash| {
                repo_generation
                    .get(&repo, node_hash)
                    .map(move |gen_id| (node_hash, gen_id))
                    .map_err(|err| err.context(ErrorKind::GenerationFetchFailed).into())
            }),
    )
}

impl DifferenceOfUnionsOfAncestorsNodeStream {
    pub fn new(
        repo: &Arc<BlobRepo>,
        repo_generation: RepoGenCache,
        hash: HgNodeHash,
    ) -> Box<NodeStream> {
        Self::new_with_excludes(repo, repo_generation, vec![hash], vec![])
    }

    pub fn new_union(
        repo: &Arc<BlobRepo>,
        repo_generation: RepoGenCache,
        hashes: Vec<HgNodeHash>,
    ) -> Box<NodeStream> {
        Self::new_with_excludes(repo, repo_generation, hashes, vec![])
    }

    pub fn new_with_excludes(
        repo: &Arc<BlobRepo>,
        repo_generation: RepoGenCache,
        hashes: Vec<HgNodeHash>,
        excludes: Vec<HgNodeHash>,
    ) -> Box<NodeStream> {
        let excludes = if !excludes.is_empty() {
            Self::new_union(repo, repo_generation.clone(), excludes)
        } else {
            empty().boxify()
        };

        add_generations(
            stream::iter_ok(hashes.into_iter()).boxify(),
            repo_generation.clone(),
            repo.clone(),
        ).collect()
            .map({
                let repo = repo.clone();
                move |hashes_generations| {
                    let mut next_generation = BTreeMap::new();
                    let mut sorted_unique_generations = UniqueHeap::new();
                    for (hash, generation) in hashes_generations {
                        next_generation
                            .entry(generation.clone())
                            .or_insert_with(HashSet::new)
                            .insert(hash);
                        // insert into our sorted list of generations
                        sorted_unique_generations.push(generation);
                    }

                    let excludes = add_generations(excludes, repo_generation.clone(), repo.clone());
                    Self {
                        repo: repo.clone(),
                        repo_generation,
                        next_generation,
                        // Start with a fake state - maximum generation number and no entries
                        // for it (see drain below)
                        current_generation: Generation::max_gen(),
                        pending_changesets: SelectAll::new(),
                        exclude_ancestors: GroupedByGenenerationStream::new(excludes)
                            .fuse()
                            .peekable(),
                        drain: hashset!{}.into_iter().peekable(),
                        sorted_unique_generations,
                    }.boxify()
                }
            })
            .map_err(|err| err.context(ErrorKind::GenerationFetchFailed))
            .from_err()
            .flatten_stream()
            .boxify()
    }

    fn exclude_node(
        &mut self,
        node: HgNodeHash,
        current_generation: Generation,
    ) -> Poll<bool, Error> {
        loop {
            match try_ready!(self.exclude_ancestors.peek()) {
                Some(entry) => {
                    if entry.1 == current_generation {
                        return Ok(Async::Ready(entry.0.contains(&node)));
                    } else if entry.1 < current_generation {
                        return Ok(Async::Ready(false));
                    }
                }
                None => {
                    return Ok(Async::Ready(false));
                }
            };

            // Current generation in `exclude_ancestors` is bigger than `current_generation`.
            // We need to skip
            try_ready!(self.exclude_ancestors.poll());
        }
    }

    fn update_generation(&mut self) {
        let highest_generation = self.sorted_unique_generations
            .pop()
            .expect("Expected a non empty heap of generations");
        let new_generation = self.next_generation
            .remove(&highest_generation)
            .expect("Highest generation doesn't exist");
        self.current_generation = highest_generation;
        self.drain = new_generation.into_iter().peekable();
    }
}

impl Stream for DifferenceOfUnionsOfAncestorsNodeStream {
    type Item = HgNodeHash;
    type Error = Error;
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            // Empty the drain if any - return all items for this generation
            while self.drain.peek().is_some() {
                let current_generation = self.current_generation;

                let next_in_drain = *self.drain.peek().unwrap();
                if try_ready!(self.exclude_node(next_in_drain, current_generation)) {
                    self.drain.next();
                    continue;
                } else {
                    let next_in_drain = self.drain.next();
                    self.pending_changesets.push(make_pending(
                        self.repo.clone(),
                        self.repo_generation.clone(),
                        next_in_drain.unwrap(),
                    ));
                    return Ok(Async::Ready(next_in_drain));
                }
            }

            // Wait until we've drained pending_changesets - we can't continue until we
            // know about all parents of the just-output generation
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

            self.update_generation();
        }
    }
}

/// Stream that transforms any input stream and returns pairs of generation number and
/// a set with all of the nodes for this generation number. For example, for a stream like
///
///     o A
///    / \
/// B o   o C
///   |   |
/// D o   o E
///    \ /
///     o F
///
/// GroupedByGenenerationStream will return (4, {A}), (3, {B, C}), (2, {D, E}), (1, {F})
struct GroupedByGenenerationStream {
    input: stream::Fuse<InputStream>,
    current_generation: Option<Generation>,
    hashes: HashSet<HgNodeHash>,
}

impl GroupedByGenenerationStream {
    pub fn new(input: InputStream) -> Self {
        Self {
            input: input.fuse(),
            current_generation: None,
            hashes: hashset!{},
        }
    }
}

impl Stream for GroupedByGenenerationStream {
    type Item = (HashSet<HgNodeHash>, Generation);
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            match try_ready!(self.input.poll()) {
                Some(item) => {
                    if self.current_generation.is_none() {
                        self.current_generation = Some(item.1);
                    }

                    if self.current_generation == Some(item.1) {
                        self.hashes.insert(item.0);
                    } else if self.current_generation > Some(item.1) {
                        let res = (self.hashes.clone(), self.current_generation.take().unwrap());
                        self.hashes = hashset!{item.0};
                        self.current_generation = Some(item.1);
                        return Ok(Async::Ready(Some(res)));
                    } else {
                        panic!("unexpected current_generation");
                    }
                }
                None => {
                    if self.current_generation.is_none() {
                        return Ok(Async::Ready(None));
                    } else {
                        let res = (self.hashes.clone(), self.current_generation.take().unwrap());
                        self.hashes = hashset!{};
                        return Ok(Async::Ready(Some(res)));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use async_unit;
    use futures::executor::spawn;
    use linear;
    use merge_uneven;
    use tests::assert_node_sequence;
    use tests::string_to_nodehash;

    #[test]
    fn grouped_by_generation_simple() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));
            let repo_generation = RepoGenCache::new(10);

            let nodestream = DifferenceOfUnionsOfAncestorsNodeStream::new(
                &repo,
                repo_generation.clone(),
                string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157"),
            ).boxify();
            let inputstream = add_generations(nodestream, repo_generation.clone(), repo.clone());

            let res = spawn(GroupedByGenenerationStream::new(inputstream).collect())
                .wait_future()
                .expect("failed to finish groupped stream");

            assert_eq!(
                res,
                vec![
                    (
                        hashset!{string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157")},
                        Generation::new(8),
                    ),
                    (
                        hashset!{string_to_nodehash("0ed509bf086fadcb8a8a5384dc3b550729b0fc17")},
                        Generation::new(7),
                    ),
                    (
                        hashset!{string_to_nodehash("eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b")},
                        Generation::new(6),
                    ),
                    (
                        hashset!{string_to_nodehash("cb15ca4a43a59acff5388cea9648c162afde8372")},
                        Generation::new(5),
                    ),
                    (
                        hashset!{string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0")},
                        Generation::new(4),
                    ),
                    (
                        hashset!{string_to_nodehash("607314ef579bd2407752361ba1b0c1729d08b281")},
                        Generation::new(3),
                    ),
                    (
                        hashset!{string_to_nodehash("3e0e761030db6e479a7fb58b12881883f9f8c63f")},
                        Generation::new(2),
                    ),
                    (
                        hashset!{string_to_nodehash("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")},
                        Generation::new(1),
                    ),
                ],
            );
        });
    }

    #[test]
    fn grouped_by_generation_merge_uneven() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(merge_uneven::getrepo(None));
            let repo_generation = RepoGenCache::new(10);

            let nodestream = DifferenceOfUnionsOfAncestorsNodeStream::new(
                &repo,
                repo_generation.clone(),
                string_to_nodehash("75742e6fc286a359b39a89fdfa437cc7e2a0e1ce"),
            ).boxify();
            let inputstream = add_generations(nodestream, repo_generation.clone(), repo.clone());

            let res = spawn(GroupedByGenenerationStream::new(inputstream).collect())
                .wait_future()
                .expect("failed to finish groupped stream");

            assert_eq!(
                res,
                vec![
                    (
                        hashset!{string_to_nodehash("75742e6fc286a359b39a89fdfa437cc7e2a0e1ce")},
                        Generation::new(10),
                    ),
                    (
                        hashset!{string_to_nodehash("264f01429683b3dd8042cb3979e8bf37007118bc")},
                        Generation::new(9),
                    ),
                    (
                        hashset!{string_to_nodehash("5d43888a3c972fe68c224f93d41b30e9f888df7c")},
                        Generation::new(8),
                    ),
                    (
                        hashset!{string_to_nodehash("fc2cef43395ff3a7b28159007f63d6529d2f41ca")},
                        Generation::new(7),
                    ),
                    (
                        hashset!{string_to_nodehash("bc7b4d0f858c19e2474b03e442b8495fd7aeef33")},
                        Generation::new(6),
                    ),
                    (
                        hashset!{string_to_nodehash("795b8133cf375f6d68d27c6c23db24cd5d0cd00f")},
                        Generation::new(5),
                    ),
                    (
                        hashset!{
                            string_to_nodehash("4f7f3fd428bec1a48f9314414b063c706d9c1aed"),
                            string_to_nodehash("16839021e338500b3cf7c9b871c8a07351697d68"),
                        },
                        Generation::new(4),
                    ),
                    (
                        hashset!{
                            string_to_nodehash("b65231269f651cfe784fd1d97ef02a049a37b8a0"),
                            string_to_nodehash("1d8a907f7b4bf50c6a09c16361e2205047ecc5e5"),
                        },
                        Generation::new(3),
                    ),
                    (
                        hashset!{
                            string_to_nodehash("d7542c9db7f4c77dab4b315edd328edf1514952f"),
                            string_to_nodehash("3cda5c78aa35f0f5b09780d971197b51cad4613a"),
                        },
                        Generation::new(2),
                    ),
                    (
                        hashset!{string_to_nodehash("15c40d0abc36d47fb51c8eaec51ac7aad31f669c")},
                        Generation::new(1),
                    ),
                ],
            );
        });
    }

    #[test]
    fn empty_ancestors_combinators() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));
            let repo_generation = RepoGenCache::new(10);

            let stream = DifferenceOfUnionsOfAncestorsNodeStream::new_union(
                &repo,
                repo_generation.clone(),
                vec![],
            ).boxify();

            assert_node_sequence(repo_generation.clone(), &repo, vec![], stream);

            let excludes = vec![
                string_to_nodehash("0ed509bf086fadcb8a8a5384dc3b550729b0fc17"),
            ];

            let stream = DifferenceOfUnionsOfAncestorsNodeStream::new_with_excludes(
                &repo,
                repo_generation.clone(),
                vec![],
                excludes,
            ).boxify();

            assert_node_sequence(repo_generation, &repo, vec![], stream);
        });
    }

    #[test]
    fn linear_ancestors_with_excludes() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));
            let repo_generation = RepoGenCache::new(10);

            let nodestream = DifferenceOfUnionsOfAncestorsNodeStream::new_with_excludes(
                &repo,
                repo_generation.clone(),
                vec![
                    string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157"),
                ],
                vec![
                    string_to_nodehash("0ed509bf086fadcb8a8a5384dc3b550729b0fc17"),
                ],
            ).boxify();

            assert_node_sequence(
                repo_generation,
                &repo,
                vec![
                    string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157"),
                ],
                nodestream,
            );
        });
    }

    #[test]
    fn linear_ancestors_with_excludes_empty() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));
            let repo_generation = RepoGenCache::new(10);

            let nodestream = DifferenceOfUnionsOfAncestorsNodeStream::new_with_excludes(
                &repo,
                repo_generation.clone(),
                vec![
                    string_to_nodehash("0ed509bf086fadcb8a8a5384dc3b550729b0fc17"),
                ],
                vec![
                    string_to_nodehash("0ed509bf086fadcb8a8a5384dc3b550729b0fc17"),
                ],
            ).boxify();

            assert_node_sequence(repo_generation, &repo, vec![], nodestream);
        });
    }

    #[test]
    fn ancestors_union() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(merge_uneven::getrepo(None));
            let repo_generation = RepoGenCache::new(10);

            let nodestream = DifferenceOfUnionsOfAncestorsNodeStream::new_union(
                &repo,
                repo_generation.clone(),
                vec![
                    string_to_nodehash("fc2cef43395ff3a7b28159007f63d6529d2f41ca"),
                    string_to_nodehash("16839021e338500b3cf7c9b871c8a07351697d68"),
                ],
            ).boxify();
            assert_node_sequence(
                repo_generation,
                &repo,
                vec![
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
    fn merge_ancestors_from_merge_excludes() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(merge_uneven::getrepo(None));
            let repo_generation = RepoGenCache::new(10);

            let nodestream = DifferenceOfUnionsOfAncestorsNodeStream::new_with_excludes(
                &repo,
                repo_generation.clone(),
                vec![
                    string_to_nodehash("75742e6fc286a359b39a89fdfa437cc7e2a0e1ce"),
                ],
                vec![
                    string_to_nodehash("fc2cef43395ff3a7b28159007f63d6529d2f41ca"),
                    string_to_nodehash("16839021e338500b3cf7c9b871c8a07351697d68"),
                ],
            ).boxify();

            assert_node_sequence(
                repo_generation,
                &repo,
                vec![
                    string_to_nodehash("75742e6fc286a359b39a89fdfa437cc7e2a0e1ce"),
                    string_to_nodehash("264f01429683b3dd8042cb3979e8bf37007118bc"),
                    string_to_nodehash("5d43888a3c972fe68c224f93d41b30e9f888df7c"),
                ],
                nodestream,
            );
        });
    }

    #[test]
    fn merge_ancestors_from_merge_excludes_union() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(merge_uneven::getrepo(None));
            let repo_generation = RepoGenCache::new(10);

            let nodestream = DifferenceOfUnionsOfAncestorsNodeStream::new_with_excludes(
                &repo,
                repo_generation.clone(),
                vec![
                    string_to_nodehash("75742e6fc286a359b39a89fdfa437cc7e2a0e1ce"),
                ],
                vec![
                    string_to_nodehash("16839021e338500b3cf7c9b871c8a07351697d68"),
                ],
            ).boxify();

            assert_node_sequence(
                repo_generation,
                &repo,
                vec![
                    string_to_nodehash("75742e6fc286a359b39a89fdfa437cc7e2a0e1ce"),
                    string_to_nodehash("264f01429683b3dd8042cb3979e8bf37007118bc"),
                    string_to_nodehash("5d43888a3c972fe68c224f93d41b30e9f888df7c"),
                    string_to_nodehash("fc2cef43395ff3a7b28159007f63d6529d2f41ca"),
                    string_to_nodehash("bc7b4d0f858c19e2474b03e442b8495fd7aeef33"),
                    string_to_nodehash("795b8133cf375f6d68d27c6c23db24cd5d0cd00f"),
                    string_to_nodehash("4f7f3fd428bec1a48f9314414b063c706d9c1aed"),
                    string_to_nodehash("b65231269f651cfe784fd1d97ef02a049a37b8a0"),
                    string_to_nodehash("d7542c9db7f4c77dab4b315edd328edf1514952f"),
                ],
                nodestream,
            );
        });
    }

}
