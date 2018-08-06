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
use std::collections::HashMap;
use std::collections::hash_map::IntoIter;
use std::iter::IntoIterator;
use std::mem::replace;
use std::sync::Arc;

use NodeStream;
use errors::*;
use setcommon::*;

pub struct IntersectNodeStream {
    inputs: Vec<(InputStream, Poll<Option<(HgNodeHash, Generation)>, Error>)>,
    current_generation: Option<Generation>,
    accumulator: HashMap<HgNodeHash, usize>,
    drain: Option<IntoIter<HgNodeHash, usize>>,
}

impl IntersectNodeStream {
    pub fn new<I>(repo: &Arc<BlobRepo>, inputs: I) -> Self
    where
        I: IntoIterator<Item = Box<NodeStream>>,
    {
        let hash_and_gen = inputs
            .into_iter()
            .map({ move |i| (add_generations(i, repo.clone()), Ok(Async::NotReady)) });
        IntersectNodeStream {
            inputs: hash_and_gen.collect(),
            current_generation: None,
            accumulator: HashMap::new(),
            drain: None,
        }
    }

    pub fn boxed(self) -> Box<NodeStream> {
        Box::new(self)
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
                .min();
        }
    }

    fn accumulate_nodes(&mut self) {
        let mut found_hashes = false;
        for &mut (_, ref mut state) in self.inputs.iter_mut() {
            if let Ok(Async::Ready(Some((hash, gen_id)))) = *state {
                if Some(gen_id) == self.current_generation {
                    *self.accumulator.entry(hash).or_insert(0) += 1;
                }
                // Inputs of higher generation than the current one get consumed and dropped
                if Some(gen_id) >= self.current_generation {
                    found_hashes = true;
                    *state = Ok(Async::NotReady);
                }
            }
        }
        if !found_hashes {
            self.current_generation = None;
        }
    }

    fn any_input_finished(&self) -> bool {
        if self.inputs.is_empty() {
            true
        } else {
            self.inputs
                .iter()
                .map(|&(_, ref state)| match state {
                    &Ok(Async::Ready(None)) => true,
                    _ => false,
                })
                .any(|done| done)
        }
    }
}

impl Stream for IntersectNodeStream {
    type Item = HgNodeHash;
    type Error = Error;
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // This feels wrong, but in practice it's fine - it should be quick to hit a return, and
        // the standard futures::executor expects you to only return NotReady if blocked on I/O.
        loop {
            // Start by trying to turn as many NotReady as possible into real items
            poll_all_inputs(&mut self.inputs);

            // Empty the drain if any - return all items for this generation
            while self.drain.is_some() {
                let next_in_drain = self.drain.as_mut().and_then(|drain| drain.next());
                if next_in_drain.is_some() {
                    let (hash, count) = next_in_drain.expect("is_some() said this was safe");
                    if count == self.inputs.len() {
                        return Ok(Async::Ready(Some(hash)));
                    }
                } else {
                    self.drain = None;
                }
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

            // If any input is not ready (we polled above), wait for them all to be ready
            if !all_inputs_ready(&self.inputs) {
                return Ok(Async::NotReady);
            }

            match self.current_generation {
                None => if self.accumulator.is_empty() {
                    self.update_current_generation();
                } else {
                    let full_accumulator = replace(&mut self.accumulator, HashMap::new());
                    self.drain = Some(full_accumulator.into_iter());
                },
                Some(_) => self.accumulate_nodes(),
            }
            // If we cannot ever output another node, we're done.
            if self.drain.is_none() && self.accumulator.is_empty() && self.any_input_finished() {
                return Ok(Async::Ready(None));
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use {NodeStream, SingleNodeHash, UnionNodeStream};
    use async_unit;
    use fixtures::linear;
    use fixtures::unshared_merge_even;
    use fixtures::unshared_merge_uneven;
    use futures::executor::spawn;
    use setcommon::NotReadyEmptyStream;
    use std::sync::Arc;
    use tests::assert_node_sequence;
    use tests::string_to_nodehash;

    #[test]
    fn intersect_identical_node() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));

            let head_hash = string_to_nodehash("a5ffa77602a066db7d5cfb9fb5823a0895717c5a");
            let inputs: Vec<Box<NodeStream>> = vec![
                SingleNodeHash::new(head_hash.clone(), &repo).boxed(),
                SingleNodeHash::new(head_hash.clone(), &repo).boxed(),
            ];
            let nodestream = IntersectNodeStream::new(&repo, inputs.into_iter()).boxed();

            assert_node_sequence(&repo, vec![head_hash.clone()], nodestream);
        });
    }

    #[test]
    fn intersect_three_different_nodes() {
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
            let nodestream = IntersectNodeStream::new(&repo, inputs.into_iter()).boxed();

            assert_node_sequence(&repo, vec![], nodestream);
        });
    }

    #[test]
    fn intersect_three_identical_nodes() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));

            let inputs: Vec<Box<NodeStream>> = vec![
                SingleNodeHash::new(
                    string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                    &repo,
                ).boxed(),
            ];
            let nodestream = IntersectNodeStream::new(&repo, inputs.into_iter()).boxed();

            assert_node_sequence(
                &repo,
                vec![
                    string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                ],
                nodestream,
            );
        });
    }

    #[test]
    fn intersect_nesting() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));

            let inputs: Vec<Box<NodeStream>> = vec![
                SingleNodeHash::new(
                    string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                    &repo,
                ).boxed(),
            ];

            let nodestream = IntersectNodeStream::new(&repo, inputs.into_iter()).boxed();

            let inputs: Vec<Box<NodeStream>> = vec![
                nodestream,
                SingleNodeHash::new(
                    string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                    &repo,
                ).boxed(),
            ];
            let nodestream = IntersectNodeStream::new(&repo, inputs.into_iter()).boxed();

            assert_node_sequence(
                &repo,
                vec![
                    string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                ],
                nodestream,
            );
        });
    }

    #[test]
    fn intersection_of_unions() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));

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

            // This set has a different node sequence, so that we can demonstrate that we skip nodes
            // when they're not going to contribute.
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

            let nodestream2 = UnionNodeStream::new(&repo, inputs.into_iter()).boxed();

            let inputs: Vec<Box<NodeStream>> = vec![nodestream, nodestream2];
            let nodestream = IntersectNodeStream::new(&repo, inputs.into_iter()).boxed();

            assert_node_sequence(
                &repo,
                vec![
                    string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                    string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                ],
                nodestream,
            );
        });
    }

    #[test]
    fn intersect_error_node() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));

            let nodehash = string_to_nodehash("0000000000000000000000000000000000000000");
            let inputs: Vec<Box<NodeStream>> = vec![
                Box::new(RepoErrorStream { hash: nodehash }),
                SingleNodeHash::new(nodehash.clone(), &repo).boxed(),
            ];
            let mut nodestream = spawn(IntersectNodeStream::new(&repo, inputs.into_iter()).boxed());

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
    fn intersect_nothing() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));

            let inputs: Vec<Box<NodeStream>> = vec![];
            let nodestream = IntersectNodeStream::new(&repo, inputs.into_iter()).boxed();
            assert_node_sequence(&repo, vec![], nodestream);
        });
    }

    #[test]
    fn slow_ready_intersect_nothing() {
        async_unit::tokio_unit_test(|| {
            // Tests that we handle an input staying at NotReady for a while without panicing
            let repeats = 10;
            let repo = Arc::new(linear::getrepo(None));
            let inputs: Vec<Box<NodeStream>> = vec![
                Box::new(NotReadyEmptyStream {
                    poll_count: repeats,
                }),
            ];
            let mut nodestream = IntersectNodeStream::new(&repo, inputs.into_iter()).boxed();

            // Keep polling until we should be done.
            for _ in 0..repeats + 1 {
                match nodestream.poll() {
                    Ok(Async::Ready(None)) => return,
                    Ok(Async::NotReady) => (),
                    x => panic!("Unexpected poll result {:?}", x),
                }
            }
            panic!(
                "Intersect of something that's not ready {} times failed to complete",
                repeats
            );
        });
    }

    #[test]
    fn intersect_unshared_merge_even() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(unshared_merge_even::getrepo(None));

            // Post-merge, merge, and both unshared branches
            let inputs: Vec<Box<NodeStream>> = vec![
                SingleNodeHash::new(
                    string_to_nodehash("7fe9947f101acb4acf7d945e69f0d6ce76a81113"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("d592490c4386cdb3373dd93af04d563de199b2fb"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("33fb49d8a47b29290f5163e30b294339c89505a2"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("03b0589d9788870817d03ce7b87516648ed5b33a"),
                    &repo,
                ).boxed(),
            ];
            let left_nodestream = UnionNodeStream::new(&repo, inputs.into_iter()).boxed();

            // Four commits from one branch
            let inputs: Vec<Box<NodeStream>> = vec![
                SingleNodeHash::new(
                    string_to_nodehash("03b0589d9788870817d03ce7b87516648ed5b33a"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("2fa8b4ee6803a18db4649a3843a723ef1dfe852b"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("0b94a2881dda90f0d64db5fae3ee5695a38e7c8f"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("f61fdc0ddafd63503dcd8eed8994ec685bfc8941"),
                    &repo,
                ).boxed(),
            ];
            let right_nodestream = UnionNodeStream::new(&repo, inputs.into_iter()).boxed();

            let inputs: Vec<Box<NodeStream>> = vec![left_nodestream, right_nodestream];
            let nodestream = IntersectNodeStream::new(&repo, inputs.into_iter()).boxed();

            assert_node_sequence(
                &repo,
                vec![
                    string_to_nodehash("03b0589d9788870817d03ce7b87516648ed5b33a"),
                ],
                nodestream,
            );
        });
    }

    #[test]
    fn intersect_unshared_merge_uneven() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(unshared_merge_uneven::getrepo(None));

            // Post-merge, merge, and both unshared branches
            let inputs: Vec<Box<NodeStream>> = vec![
                SingleNodeHash::new(
                    string_to_nodehash("339ec3d2a986d55c5ac4670cca68cf36b8dc0b82)"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("396c60c14337b31ffd0b6aa58a026224713dc07d)"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("64011f64aaf9c2ad2e674f57c033987da4016f51"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("03b0589d9788870817d03ce7b87516648ed5b33a"),
                    &repo,
                ).boxed(),
            ];
            let left_nodestream = UnionNodeStream::new(&repo, inputs.into_iter()).boxed();

            // Four commits from one branch
            let inputs: Vec<Box<NodeStream>> = vec![
                SingleNodeHash::new(
                    string_to_nodehash("03b0589d9788870817d03ce7b87516648ed5b33a"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("2fa8b4ee6803a18db4649a3843a723ef1dfe852b"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("0b94a2881dda90f0d64db5fae3ee5695a38e7c8f"),
                    &repo,
                ).boxed(),
                SingleNodeHash::new(
                    string_to_nodehash("f61fdc0ddafd63503dcd8eed8994ec685bfc8941"),
                    &repo,
                ).boxed(),
            ];
            let right_nodestream = UnionNodeStream::new(&repo, inputs.into_iter()).boxed();

            let inputs: Vec<Box<NodeStream>> = vec![left_nodestream, right_nodestream];
            let nodestream = IntersectNodeStream::new(&repo, inputs.into_iter()).boxed();

            assert_node_sequence(
                &repo,
                vec![
                    string_to_nodehash("03b0589d9788870817d03ce7b87516648ed5b33a"),
                ],
                nodestream,
            );
        });
    }
}
