// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::BlobRepo;
use futures::{Async, Poll};
use futures::stream::Stream;
use mercurial_types::HgNodeHash;
use mononoke_types::Generation;
use repoinfo::RepoGenCache;
use std::boxed::Box;
use std::collections::HashSet;
use std::sync::Arc;

use NodeStream;
use errors::*;
use setcommon::*;

pub struct SetDifferenceNodeStream {
    keep_input: InputStream,
    next_keep: Async<Option<(HgNodeHash, Generation)>>,

    remove_input: InputStream,
    next_remove: Async<Option<(HgNodeHash, Generation)>>,

    remove_nodes: HashSet<HgNodeHash>,
    remove_generation: Option<Generation>,
}

impl SetDifferenceNodeStream {
    pub fn new(
        repo: &Arc<BlobRepo>,
        repo_generation: RepoGenCache,
        keep_input: Box<NodeStream>,
        remove_input: Box<NodeStream>,
    ) -> SetDifferenceNodeStream {
        SetDifferenceNodeStream {
            keep_input: add_generations(keep_input, repo_generation.clone(), repo.clone()),
            next_keep: Async::NotReady,
            remove_input: add_generations(remove_input, repo_generation, repo.clone()),
            next_remove: Async::NotReady,

            remove_nodes: HashSet::new(),
            remove_generation: None,
        }
    }

    pub fn boxed(self) -> Box<NodeStream> {
        return Box::new(self);
    }

    fn next_keep(&mut self) -> Result<&Async<Option<(HgNodeHash, Generation)>>> {
        if self.next_keep.is_not_ready() {
            self.next_keep = self.keep_input.poll()?;
        }
        Ok(&self.next_keep)
    }

    fn next_remove(&mut self) -> Result<&Async<Option<(HgNodeHash, Generation)>>> {
        if self.next_remove.is_not_ready() {
            self.next_remove = self.remove_input.poll()?;
        }
        Ok(&self.next_remove)
    }
}

impl Stream for SetDifferenceNodeStream {
    type Item = HgNodeHash;
    type Error = Error;
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // This feels wrong, but in practice it's fine - it should be quick to hit a return, and
        // the standard futures::executor expects you to only return NotReady if blocked on I/O.
        loop {
            let (keep_hash, keep_gen) = match self.next_keep()? {
                &Async::NotReady => return Ok(Async::NotReady),
                &Async::Ready(None) => return Ok(Async::Ready(None)),
                &Async::Ready(Some((hash, gen))) => (hash, gen),
            };

            // Clear nodes that won't affect future results
            if self.remove_generation != Some(keep_gen) {
                self.remove_nodes.clear();
                self.remove_generation = Some(keep_gen);
            }

            // Gather the current generation's remove hashes
            loop {
                let remove_hash = match self.next_remove()? {
                    &Async::NotReady => return Ok(Async::NotReady),
                    &Async::Ready(Some((hash, gen))) if gen == keep_gen => hash,
                    &Async::Ready(Some((_, gen))) if gen > keep_gen => {
                        // Refers to a generation that's already past (probably nothing on keep
                        // side of this generation). Skip it.
                        self.next_remove = Async::NotReady;
                        continue;
                    }
                    _ => break, // Either no more or gen < keep_gen
                };
                self.remove_nodes.insert(remove_hash);
                self.next_remove = Async::NotReady; // will cause polling of remove_input
            }

            self.next_keep = Async::NotReady; // will cause polling of keep_input

            if !self.remove_nodes.contains(&keep_hash) {
                return Ok(Async::Ready(Some(keep_hash)));
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use SingleNodeHash;
    use UnionNodeStream;
    use async_unit;
    use futures::executor::spawn;
    use linear;
    use merge_even;
    use merge_uneven;
    use repoinfo::RepoGenCache;
    use setcommon::NotReadyEmptyStream;
    use std::sync::Arc;
    use tests::assert_node_sequence;
    use tests::string_to_nodehash;

    #[test]
    fn difference_identical_node() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));
            let repo_generation = RepoGenCache::new(10);

            let head_hash = string_to_nodehash("a5ffa77602a066db7d5cfb9fb5823a0895717c5a");
            let nodestream = SetDifferenceNodeStream::new(
                &repo,
                repo_generation.clone(),
                SingleNodeHash::new(head_hash.clone(), &repo).boxed(),
                SingleNodeHash::new(head_hash.clone(), &repo).boxed(),
            ).boxed();

            assert_node_sequence(repo_generation, &repo, vec![], nodestream);
        });
    }

    #[test]
    fn difference_node_and_empty() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));
            let repo_generation = RepoGenCache::new(10);

            let head_hash = string_to_nodehash("a5ffa77602a066db7d5cfb9fb5823a0895717c5a");
            let nodestream = SetDifferenceNodeStream::new(
                &repo,
                repo_generation.clone(),
                SingleNodeHash::new(head_hash.clone(), &repo).boxed(),
                Box::new(NotReadyEmptyStream { poll_count: 0 }),
            ).boxed();

            assert_node_sequence(repo_generation, &repo, vec![head_hash], nodestream);
        });
    }

    #[test]
    fn difference_empty_and_node() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));
            let repo_generation = RepoGenCache::new(10);

            let head_hash = string_to_nodehash("a5ffa77602a066db7d5cfb9fb5823a0895717c5a");
            let nodestream = SetDifferenceNodeStream::new(
                &repo,
                repo_generation.clone(),
                Box::new(NotReadyEmptyStream { poll_count: 0 }),
                SingleNodeHash::new(head_hash.clone(), &repo).boxed(),
            ).boxed();

            assert_node_sequence(repo_generation, &repo, vec![], nodestream);
        });
    }

    #[test]
    fn difference_two_nodes() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));
            let repo_generation = RepoGenCache::new(10);

            let nodestream = SetDifferenceNodeStream::new(
                &repo,
                repo_generation.clone(),
                SingleNodeHash::new(
                    string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                    &repo,
                ).boxed(),
            ).boxed();

            assert_node_sequence(
                repo_generation,
                &repo,
                vec![
                    string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                ],
                nodestream,
            );
        });
    }

    #[test]
    fn difference_error_node() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));
            let repo_generation = RepoGenCache::new(10);

            let nodehash = string_to_nodehash("0000000000000000000000000000000000000000");
            let mut nodestream = spawn(
                SetDifferenceNodeStream::new(
                    &repo,
                    repo_generation,
                    Box::new(RepoErrorStream { hash: nodehash }),
                    SingleNodeHash::new(nodehash.clone(), &repo).boxed(),
                ).boxed(),
            );

            match nodestream.wait_stream() {
                Some(Err(err)) => match err.downcast::<ErrorKind>() {
                    Ok(ErrorKind::RepoError(hash)) => assert_eq!(hash, nodehash),
                    Ok(bad) => panic!("unexpected error {:?}", bad),
                    Err(bad) => panic!("unknown error {:?}", bad),
                },
                Some(Ok(bad)) => panic!("unexpected success {:?}", bad),
                None => panic!("no result"),
            };
        });
    }

    #[test]
    fn slow_ready_difference_nothing() {
        async_unit::tokio_unit_test(|| {
            // Tests that we handle an input staying at NotReady for a while without panicing
            let repeats = 10;
            let repo = Arc::new(linear::getrepo(None));
            let repo_generation = RepoGenCache::new(10);
            let mut nodestream = SetDifferenceNodeStream::new(
                &repo,
                repo_generation,
                Box::new(NotReadyEmptyStream {
                    poll_count: repeats,
                }),
                Box::new(NotReadyEmptyStream {
                    poll_count: repeats,
                }),
            ).boxed();

            // Keep polling until we should be done.
            for _ in 0..repeats + 1 {
                match nodestream.poll() {
                    Ok(Async::Ready(None)) => return,
                    Ok(Async::NotReady) => (),
                    x => panic!("Unexpected poll result {:?}", x),
                }
            }
            panic!(
                "Set difference of something that's not ready {} times failed to complete",
                repeats
            );
        });
    }

    #[test]
    fn difference_union_with_single_node() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));
            let repo_generation = RepoGenCache::new(10);

            let inputs: Vec<Box<NodeStream>> = vec![
                SingleNodeHash::new(
                    string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                    &repo,
                ).boxed(),
            ];
            let nodestream =
                UnionNodeStream::new(&repo, repo_generation.clone(), inputs.into_iter()).boxed();

            let nodestream = SetDifferenceNodeStream::new(
                &repo,
                repo_generation.clone(),
                nodestream,
                SingleNodeHash::new(
                    string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                    &repo,
                ).boxed(),
            ).boxed();

            assert_node_sequence(
                repo_generation,
                &repo,
                vec![
                    string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157"),
                    string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                ],
                nodestream,
            );
        });
    }

    #[test]
    fn difference_single_node_with_union() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));
            let repo_generation = RepoGenCache::new(10);

            let inputs: Vec<Box<NodeStream>> = vec![
                SingleNodeHash::new(
                    string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                    &repo,
                ).boxed(),
            ];
            let nodestream =
                UnionNodeStream::new(&repo, repo_generation.clone(), inputs.into_iter()).boxed();

            let nodestream = SetDifferenceNodeStream::new(
                &repo,
                repo_generation.clone(),
                SingleNodeHash::new(
                    string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                    &repo,
                ).boxed(),
                nodestream,
            ).boxed();

            assert_node_sequence(repo_generation, &repo, vec![], nodestream);
        });
    }

    #[test]
    fn difference_merge_even() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(merge_even::getrepo(None));
            let repo_generation = RepoGenCache::new(10);

            // Top three commits in my hg log -G -r 'all()' output
            let inputs: Vec<Box<NodeStream>> = vec![
                SingleNodeHash::new(
                    string_to_nodehash("babf5e4dea7ffcf32c3740ff2f1351de4e15c889"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("4f7f3fd428bec1a48f9314414b063c706d9c1aed"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("16839021e338500b3cf7c9b871c8a07351697d68"),
                    &repo,
                ).boxed(),
            ];
            let left_nodestream =
                UnionNodeStream::new(&repo, repo_generation.clone(), inputs.into_iter()).boxed();

            // Everything from base to just before merge on one side
            let inputs: Vec<Box<NodeStream>> = vec![
                SingleNodeHash::new(
                    string_to_nodehash("4f7f3fd428bec1a48f9314414b063c706d9c1aed"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("b65231269f651cfe784fd1d97ef02a049a37b8a0"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("d7542c9db7f4c77dab4b315edd328edf1514952f"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("15c40d0abc36d47fb51c8eaec51ac7aad31f669c"),
                    &repo,
                ).boxed(),
            ];
            let right_nodestream =
                UnionNodeStream::new(&repo, repo_generation.clone(), inputs.into_iter()).boxed();

            let nodestream = SetDifferenceNodeStream::new(
                &repo,
                repo_generation.clone(),
                left_nodestream,
                right_nodestream,
            ).boxed();

            assert_node_sequence(
                repo_generation,
                &repo,
                vec![
                    string_to_nodehash("babf5e4dea7ffcf32c3740ff2f1351de4e15c889"),
                    string_to_nodehash("16839021e338500b3cf7c9b871c8a07351697d68"),
                ],
                nodestream,
            );
        });
    }

    #[test]
    fn difference_merge_uneven() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(merge_uneven::getrepo(None));
            let repo_generation = RepoGenCache::new(10);

            // Merge commit, and one from each branch
            let inputs: Vec<Box<NodeStream>> = vec![
                SingleNodeHash::new(
                    string_to_nodehash("75742e6fc286a359b39a89fdfa437cc7e2a0e1ce"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("4f7f3fd428bec1a48f9314414b063c706d9c1aed"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("16839021e338500b3cf7c9b871c8a07351697d68"),
                    &repo,
                ).boxed(),
            ];
            let left_nodestream =
                UnionNodeStream::new(&repo, repo_generation.clone(), inputs.into_iter()).boxed();

            // Everything from base to just before merge on one side
            let inputs: Vec<Box<NodeStream>> = vec![
                SingleNodeHash::new(
                    string_to_nodehash("16839021e338500b3cf7c9b871c8a07351697d68"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("1d8a907f7b4bf50c6a09c16361e2205047ecc5e5"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("3cda5c78aa35f0f5b09780d971197b51cad4613a"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("15c40d0abc36d47fb51c8eaec51ac7aad31f669c"),
                    &repo,
                ).boxed(),
            ];
            let right_nodestream =
                UnionNodeStream::new(&repo, repo_generation.clone(), inputs.into_iter()).boxed();

            let nodestream = SetDifferenceNodeStream::new(
                &repo,
                repo_generation.clone(),
                left_nodestream,
                right_nodestream,
            ).boxed();

            assert_node_sequence(
                repo_generation,
                &repo,
                vec![
                    string_to_nodehash("75742e6fc286a359b39a89fdfa437cc7e2a0e1ce"),
                    string_to_nodehash("4f7f3fd428bec1a48f9314414b063c706d9c1aed"),
                ],
                nodestream,
            );
        });
    }
}
