// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use futures::Async;
use futures::Poll;
use futures::stream::Stream;
use mercurial_types::{NodeHash, Repo};
use repoinfo::{Generation, RepoGenCache};
use std::boxed::Box;
use std::collections::HashSet;
use std::collections::hash_set::IntoIter;
use std::iter::IntoIterator;
use std::mem::replace;
use std::sync::Arc;

use NodeStream;
use errors::*;
use setcommon::*;

pub struct UnionNodeStream {
    inputs: Vec<(InputStream, Poll<Option<(NodeHash, Generation)>, Error>)>,
    current_generation: Option<Generation>,
    accumulator: HashSet<NodeHash>,
    drain: Option<IntoIter<NodeHash>>,
}

impl UnionNodeStream {
    pub fn new<I, R>(repo: &Arc<R>, repo_generation: RepoGenCache<R>, inputs: I) -> Self
    where
        I: IntoIterator<Item = Box<NodeStream>>,
        R: Repo,
    {
        let hash_and_gen = inputs.into_iter().map({
            move |i| {
                (
                    add_generations(i, repo_generation.clone(), repo.clone()),
                    Ok(Async::NotReady),
                )
            }
        });
        UnionNodeStream {
            inputs: hash_and_gen.collect(),
            current_generation: None,
            accumulator: HashSet::new(),
            drain: None,
        }
    }

    fn gc_finished_inputs(&mut self) {
        self.inputs
            .retain(|&(_, ref state)| if let Ok(Async::Ready(None)) = *state {
                false
            } else {
                true
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
    type Item = NodeHash;
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
    use assert_node_sequence;
    use futures::executor::spawn;
    use linear;
    use repoinfo::RepoGenCache;
    use setcommon::NotReadyEmptyStream;
    use std::sync::Arc;
    use string_to_nodehash;

    #[test]
    fn union_identical_node() {
        let repo = Arc::new(linear::getrepo());
        let repo_generation = RepoGenCache::new(10);

        let head_hash = string_to_nodehash("a5ffa77602a066db7d5cfb9fb5823a0895717c5a");
        let inputs: Vec<Box<NodeStream>> = vec![
            Box::new(SingleNodeHash::new(head_hash.clone(), &repo)),
            Box::new(SingleNodeHash::new(head_hash.clone(), &repo)),
        ];
        let nodestream = Box::new(UnionNodeStream::new(
            &repo,
            repo_generation,
            inputs.into_iter(),
        ));

        assert_node_sequence(vec![head_hash.clone()], nodestream);
    }
    #[test]
    fn union_error_node() {
        let repo = Arc::new(linear::getrepo());
        let repo_generation = RepoGenCache::new(10);

        let nodehash = string_to_nodehash("0000000000000000000000000000000000000000");
        let inputs: Vec<Box<NodeStream>> = vec![
            Box::new(SingleNodeHash::new(nodehash.clone(), &repo)),
            Box::new(SingleNodeHash::new(nodehash.clone(), &repo)),
        ];
        let mut nodestream = spawn(Box::new(UnionNodeStream::new(
            &repo,
            repo_generation,
            inputs.into_iter(),
        )));

        assert!(
            if let Some(Err(Error(ErrorKind::NoSuchNode(hash), _))) = nodestream.wait_stream() {
                hash == nodehash
            } else {
                false
            },
            "No error for bad node"
        );
    }
    #[test]
    fn union_three_nodes() {
        let repo = Arc::new(linear::getrepo());
        let repo_generation = RepoGenCache::new(10);

        // Note that these are *not* in generation order deliberately.
        let inputs: Vec<Box<NodeStream>> = vec![
            Box::new(SingleNodeHash::new(
                string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157"),
                &repo,
            )),
            Box::new(SingleNodeHash::new(
                string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                &repo,
            )),
            Box::new(SingleNodeHash::new(
                string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                &repo,
            )),
        ];
        let nodestream = Box::new(UnionNodeStream::new(
            &repo,
            repo_generation,
            inputs.into_iter(),
        ));

        // But, once I hit the asserts, I expect them in generation order.
        assert_node_sequence(
            vec![
                string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157"),
                string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
            ],
            nodestream,
        );
    }
    #[test]
    fn union_nothing() {
        let repo = Arc::new(linear::getrepo());
        let repo_generation = RepoGenCache::new(10);

        let inputs: Vec<Box<NodeStream>> = vec![];
        let nodestream = Box::new(UnionNodeStream::new(
            &repo,
            repo_generation,
            inputs.into_iter(),
        ));
        assert_node_sequence(vec![], nodestream);
    }
    #[test]
    fn union_nesting() {
        let repo = Arc::new(linear::getrepo());
        let repo_generation = RepoGenCache::new(10);

        // Note that these are *not* in generation order deliberately.
        let inputs: Vec<Box<NodeStream>> = vec![
            Box::new(SingleNodeHash::new(
                string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                &repo,
            )),
            Box::new(SingleNodeHash::new(
                string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                &repo,
            )),
        ];

        let nodestream = Box::new(UnionNodeStream::new(
            &repo,
            repo_generation.clone(),
            inputs.into_iter(),
        ));

        let inputs: Vec<Box<NodeStream>> = vec![
            nodestream,
            Box::new(SingleNodeHash::new(
                string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157"),
                &repo,
            )),
        ];
        let nodestream = Box::new(UnionNodeStream::new(
            &repo,
            repo_generation,
            inputs.into_iter(),
        ));

        assert_node_sequence(
            vec![
                string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157"),
                string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
            ],
            nodestream,
        );
    }
    #[test]
    fn slow_ready_union_nothing() {
        // Tests that we handle an input staying at NotReady for a while without panicing
        let repeats = 10;
        let repo = Arc::new(linear::getrepo());
        let repo_generation = RepoGenCache::new(10);
        let inputs: Vec<Box<NodeStream>> = vec![
            Box::new(NotReadyEmptyStream {
                poll_count: repeats,
            }),
        ];
        let mut nodestream = Box::new(UnionNodeStream::new(
            &repo,
            repo_generation,
            inputs.into_iter(),
        ));

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
    }
}
