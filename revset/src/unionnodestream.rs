// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::BlobRepo;
use futures::Async;
use futures::Poll;
use futures::stream::Stream;
use mercurial_types::HgNodeHash;
use mononoke_types::Generation;
use std::boxed::Box;
use std::collections::HashSet;
use std::collections::hash_set::IntoIter;
use std::iter::IntoIterator;
use std::mem::replace;
use std::sync::Arc;

use failure::Error;

use NodeStream;
use setcommon::*;

pub struct UnionNodeStream {
    inputs: Vec<(InputStream, Poll<Option<(HgNodeHash, Generation)>, Error>)>,
    current_generation: Option<Generation>,
    accumulator: HashSet<HgNodeHash>,
    drain: Option<IntoIter<HgNodeHash>>,
}

impl UnionNodeStream {
    pub fn new<I>(repo: &Arc<BlobRepo>, inputs: I) -> Self
    where
        I: IntoIterator<Item = Box<NodeStream>>,
    {
        let hash_and_gen = inputs
            .into_iter()
            .map({ move |i| (add_generations(i, repo.clone()), Ok(Async::NotReady)) });
        UnionNodeStream {
            inputs: hash_and_gen.collect(),
            current_generation: None,
            accumulator: HashSet::new(),
            drain: None,
        }
    }

    pub fn boxed(self) -> Box<NodeStream> {
        Box::new(self)
    }

    fn gc_finished_inputs(&mut self) {
        self.inputs.retain(|&(_, ref state)| {
            if let Ok(Async::Ready(None)) = *state {
                false
            } else {
                true
            }
        });
    }

    fn update_current_generation(&mut self) {
        if all_inputs_ready(&self.inputs) {
            self.current_generation = self.inputs
                .iter()
                .filter_map(|&(_, ref state)| match state {
                    &Ok(Async::Ready(Some((_, gen_id)))) => Some(gen_id),
                    &Ok(Async::NotReady) => panic!("All states ready, yet some not ready!"),
                    _ => None,
                })
                .max();
        }
    }

    fn accumulate_nodes(&mut self) {
        let mut found_hashes = false;
        for &mut (_, ref mut state) in self.inputs.iter_mut() {
            if let Ok(Async::Ready(Some((hash, gen_id)))) = *state {
                if Some(gen_id) == self.current_generation {
                    found_hashes = true;
                    self.accumulator.insert(hash);
                    *state = Ok(Async::NotReady);
                }
            }
        }
        if !found_hashes {
            self.current_generation = None;
        }
    }
}

impl Stream for UnionNodeStream {
    type Item = HgNodeHash;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // This feels wrong, but in practice it's fine - it should be quick to hit a return, and
        // the standard futures::executor expects you to only return NotReady if blocked on I/O.
        loop {
            // Start by trying to turn as many NotReady as possible into real items
            poll_all_inputs(&mut self.inputs);

            // Empty the drain if any - return all items for this generation
            let next_in_drain = self.drain.as_mut().and_then(|drain| drain.next());
            if next_in_drain.is_some() {
                return Ok(Async::Ready(next_in_drain));
            } else {
                self.drain = None;
            }

            // Return any errors
            {
                if self.inputs.iter().any(|&(_, ref state)| state.is_err()) {
                    let inputs = replace(&mut self.inputs, Vec::new());
                    let (_, err) = inputs
                        .into_iter()
                        .find(|&(_, ref state)| state.is_err())
                        .unwrap();
                    return Err(err.unwrap_err());
                }
            }

            self.gc_finished_inputs();

            // If any input is not ready (we polled above), wait for them all to be ready
            if !all_inputs_ready(&self.inputs) {
                return Ok(Async::NotReady);
            }

            match self.current_generation {
                None => if self.accumulator.is_empty() {
                    self.update_current_generation();
                } else {
                    let full_accumulator = replace(&mut self.accumulator, HashSet::new());
                    self.drain = Some(full_accumulator.into_iter());
                },
                Some(_) => self.accumulate_nodes(),
            }
            // If we cannot ever output another node, we're done.
            if self.inputs.is_empty() && self.drain.is_none() && self.accumulator.is_empty() {
                return Ok(Async::Ready(None));
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use {NodeStream, SingleNodeHash};
    use async_unit;
    use errors::ErrorKind;
    use fixtures::{branch_even, branch_uneven, branch_wide, linear};
    use futures::executor::spawn;
    use setcommon::{NotReadyEmptyStream, RepoErrorStream};
    use std::sync::Arc;
    use tests::assert_node_sequence;
    use tests::string_to_nodehash;

    #[test]
    fn union_identical_node() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));

            let head_hash = string_to_nodehash("a5ffa77602a066db7d5cfb9fb5823a0895717c5a");
            let inputs: Vec<Box<NodeStream>> = vec![
                SingleNodeHash::new(head_hash.clone(), &repo).boxed(),
                SingleNodeHash::new(head_hash.clone(), &repo).boxed(),
            ];
            let nodestream = UnionNodeStream::new(&repo, inputs.into_iter()).boxed();

            assert_node_sequence(&repo, vec![head_hash.clone()], nodestream);
        });
    }

    #[test]
    fn union_error_node() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));

            let nodehash = string_to_nodehash("0000000000000000000000000000000000000000");
            let inputs: Vec<Box<NodeStream>> = vec![
                Box::new(RepoErrorStream { hash: nodehash }),
                SingleNodeHash::new(nodehash.clone(), &repo).boxed(),
            ];
            let mut nodestream = spawn(UnionNodeStream::new(&repo, inputs.into_iter()).boxed());

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
    fn union_three_nodes() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));

            // Note that these are *not* in generation order deliberately.
            let inputs: Vec<Box<NodeStream>> = vec![
                SingleNodeHash::new(
                    string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                    &repo,
                ).boxed(),
            ];
            let nodestream = UnionNodeStream::new(&repo, inputs.into_iter()).boxed();

            // But, once I hit the asserts, I expect them in generation order.
            assert_node_sequence(
                &repo,
                vec![
                    string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                    string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157"),
                    string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                ],
                nodestream,
            );
        });
    }

    #[test]
    fn union_nothing() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));

            let inputs: Vec<Box<NodeStream>> = vec![];
            let nodestream = UnionNodeStream::new(&repo, inputs.into_iter()).boxed();
            assert_node_sequence(&repo, vec![], nodestream);
        });
    }

    #[test]
    fn union_nesting() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));

            // Note that these are *not* in generation order deliberately.
            let inputs: Vec<Box<NodeStream>> = vec![
                SingleNodeHash::new(
                    string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                    &repo,
                ).boxed(),
            ];

            let nodestream = UnionNodeStream::new(&repo, inputs.into_iter()).boxed();

            let inputs: Vec<Box<NodeStream>> = vec![
                nodestream,
                SingleNodeHash::new(
                    string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157"),
                    &repo,
                ).boxed(),
            ];
            let nodestream = UnionNodeStream::new(&repo, inputs.into_iter()).boxed();

            assert_node_sequence(
                &repo,
                vec![
                    string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                    string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157"),
                    string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                ],
                nodestream,
            );
        });
    }

    #[test]
    fn slow_ready_union_nothing() {
        async_unit::tokio_unit_test(|| {
            // Tests that we handle an input staying at NotReady for a while without panicing
            let repeats = 10;
            let repo = Arc::new(linear::getrepo(None));
            let inputs: Vec<Box<NodeStream>> = vec![
                Box::new(NotReadyEmptyStream {
                    poll_count: repeats,
                }),
            ];
            let mut nodestream = UnionNodeStream::new(&repo, inputs.into_iter()).boxed();

            // Keep polling until we should be done.
            for _ in 0..repeats + 1 {
                match nodestream.poll() {
                    Ok(Async::Ready(None)) => return,
                    Ok(Async::NotReady) => (),
                    x => panic!("Unexpected poll result {:?}", x),
                }
            }
            panic!(
                "Union of something that's not ready {} times failed to complete",
                repeats
            );
        });
    }

    #[test]
    fn union_branch_even_repo() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(branch_even::getrepo(None));

            // Two nodes should share the same generation number
            let inputs: Vec<Box<NodeStream>> = vec![
                SingleNodeHash::new(
                    string_to_nodehash("3cda5c78aa35f0f5b09780d971197b51cad4613a"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("d7542c9db7f4c77dab4b315edd328edf1514952f"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("4f7f3fd428bec1a48f9314414b063c706d9c1aed"),
                    &repo,
                ).boxed(),
            ];
            let nodestream = UnionNodeStream::new(&repo, inputs.into_iter()).boxed();

            assert_node_sequence(
                &repo,
                vec![
                    string_to_nodehash("4f7f3fd428bec1a48f9314414b063c706d9c1aed"),
                    string_to_nodehash("3cda5c78aa35f0f5b09780d971197b51cad4613a"),
                    string_to_nodehash("d7542c9db7f4c77dab4b315edd328edf1514952f"),
                ],
                nodestream,
            );
        });
    }

    #[test]
    fn union_branch_uneven_repo() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(branch_uneven::getrepo(None));

            // Two nodes should share the same generation number
            let inputs: Vec<Box<NodeStream>> = vec![
                SingleNodeHash::new(
                    string_to_nodehash("3cda5c78aa35f0f5b09780d971197b51cad4613a"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("d7542c9db7f4c77dab4b315edd328edf1514952f"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("4f7f3fd428bec1a48f9314414b063c706d9c1aed"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("bc7b4d0f858c19e2474b03e442b8495fd7aeef33"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("264f01429683b3dd8042cb3979e8bf37007118bc"),
                    &repo,
                ).boxed(),
            ];
            let nodestream = UnionNodeStream::new(&repo, inputs.into_iter()).boxed();

            assert_node_sequence(
                &repo,
                vec![
                    string_to_nodehash("264f01429683b3dd8042cb3979e8bf37007118bc"),
                    string_to_nodehash("bc7b4d0f858c19e2474b03e442b8495fd7aeef33"),
                    string_to_nodehash("4f7f3fd428bec1a48f9314414b063c706d9c1aed"),
                    string_to_nodehash("3cda5c78aa35f0f5b09780d971197b51cad4613a"),
                    string_to_nodehash("d7542c9db7f4c77dab4b315edd328edf1514952f"),
                ],
                nodestream,
            );
        });
    }

    #[test]
    fn union_branch_wide_repo() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(branch_wide::getrepo(None));

            // Two nodes should share the same generation number
            let inputs: Vec<Box<NodeStream>> = vec![
                SingleNodeHash::new(
                    string_to_nodehash("49f53ab171171b3180e125b918bd1cf0af7e5449"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("4685e9e62e4885d477ead6964a7600c750e39b03"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("c27ef5b7f15e9930e5b93b1f32cc2108a2aabe12"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("9e8521affb7f9d10e9551a99c526e69909042b20"),
                    &repo,
                ).boxed(),
            ];
            let nodestream = UnionNodeStream::new(&repo, inputs.into_iter()).boxed();

            assert_node_sequence(
                &repo,
                vec![
                    string_to_nodehash("49f53ab171171b3180e125b918bd1cf0af7e5449"),
                    string_to_nodehash("c27ef5b7f15e9930e5b93b1f32cc2108a2aabe12"),
                    string_to_nodehash("4685e9e62e4885d477ead6964a7600c750e39b03"),
                    string_to_nodehash("9e8521affb7f9d10e9551a99c526e69909042b20"),
                ],
                nodestream,
            );
        });
    }
}
