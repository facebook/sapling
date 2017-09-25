// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use futures::{Async, Poll};
use futures::stream::Stream;
use mercurial_types::{NodeHash, Repo};
use repoinfo::{Generation, RepoGenCache};
use std::boxed::Box;
use std::collections::HashSet;
use std::collections::hash_set::IntoIter;
use std::mem::replace;
use std::sync::Arc;

use NodeStream;
use errors::*;
use setcommon::*;

pub struct SetDifferenceNodeStream {
    keep_input: (InputStream, Poll<Option<(NodeHash, Generation)>, Error>),
    remove_input: (InputStream, Poll<Option<(NodeHash, Generation)>, Error>),
    current_generation: Option<Generation>,
    keep_nodes: HashSet<NodeHash>,
    remove_nodes: HashSet<NodeHash>,
    drain: Option<IntoIter<NodeHash>>,
}

impl SetDifferenceNodeStream {
    pub fn new<R>(
        repo: &Arc<R>,
        repo_generation: RepoGenCache<R>,
        keep_input: Box<NodeStream>,
        remove_input: Box<NodeStream>,
    ) -> SetDifferenceNodeStream
    where
        R: Repo,
    {
        let keep_input = (
            add_generations(keep_input, repo_generation.clone(), repo.clone()),
            Ok(Async::NotReady),
        );
        let remove_input = (
            add_generations(remove_input, repo_generation.clone(), repo.clone()),
            Ok(Async::NotReady),
        );
        SetDifferenceNodeStream {
            keep_input,
            remove_input,
            current_generation: None,
            keep_nodes: HashSet::new(),
            remove_nodes: HashSet::new(),
            drain: None,
        }
    }

    fn poll_both_inputs(&mut self) {
        if let Ok(Async::NotReady) = self.keep_input.1 {
            self.keep_input.1 = self.keep_input.0.poll();
        }
        if let Ok(Async::NotReady) = self.remove_input.1 {
            self.remove_input.1 = self.remove_input.0.poll();
        }
    }

    fn both_inputs_ready(&self) -> bool {
        let keep_ready = match self.keep_input.1 {
            Ok(Async::Ready(_)) => true,
            _ => false,
        };
        let remove_ready = match self.remove_input.1 {
            Ok(Async::Ready(_)) => true,
            _ => false,
        };
        remove_ready && keep_ready
    }

    fn keep_input_finished(&self) -> bool {
        match self.keep_input.1 {
            Ok(Async::Ready(None)) => true,
            _ => false,
        }
    }

    fn accumulate_nodes(&mut self) {
        if let Ok(Async::Ready(Some((hash, gen_id)))) = self.keep_input.1 {
            if Some(gen_id) == self.current_generation {
                self.keep_nodes.insert(hash);
                self.keep_input.1 = Ok(Async::NotReady);
            } else {
                self.current_generation = None;
            }
        } else {
            self.current_generation = None;
        }

        if let Ok(Async::Ready(Some((hash, gen_id)))) = self.remove_input.1 {
            if Some(gen_id) == self.current_generation {
                self.remove_nodes.insert(hash);
            }
            if Some(gen_id) >= self.current_generation {
                self.remove_input.1 = Ok(Async::NotReady);
            }
        }
    }

    fn update_current_generation(&mut self) {
        if let Ok(Async::Ready(Some((_, gen_id)))) = self.keep_input.1 {
            self.current_generation = Some(gen_id);
        }
    }
}

impl Stream for SetDifferenceNodeStream {
    type Item = NodeHash;
    type Error = Error;
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // This feels wrong, but in practice it's fine - it should be quick to hit a return, and
        // the standard futures::executor expects you to only return NotReady if blocked on I/O.
        loop {
            self.poll_both_inputs();

            // Empty the drain if any - return all items for this generation
            let next_in_drain = self.drain.as_mut().and_then(|drain| drain.next());
            if next_in_drain.is_some() {
                return Ok(Async::Ready(next_in_drain));
            } else {
                self.drain = None;
            }

            // Return any errors
            if self.keep_input.1.is_err() {
                let err = replace(&mut self.keep_input.1, Ok(Async::NotReady));
                return Err(err.unwrap_err());
            }
            if self.remove_input.1.is_err() {
                let err = replace(&mut self.remove_input.1, Ok(Async::NotReady));
                return Err(err.unwrap_err());
            }

            if !self.both_inputs_ready() {
                return Ok(Async::NotReady);
            }

            match self.current_generation {
                None => if self.keep_nodes.is_empty() {
                    self.update_current_generation();
                } else {
                    let remove_nodes = replace(&mut self.remove_nodes, HashSet::new());
                    let mut keep_nodes = replace(&mut self.keep_nodes, HashSet::new());
                    keep_nodes.retain(|hash| !remove_nodes.contains(hash));
                    self.drain = Some(keep_nodes.into_iter());
                },
                Some(_) => self.accumulate_nodes(),
            }
            // If we cannot ever output another node, we're done.
            if self.keep_input_finished() && self.drain.is_none() && self.keep_nodes.is_empty() {
                return Ok(Async::Ready(None));
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use SingleNodeHash;
    use UnionNodeStream;
    use assert_node_sequence;
    use futures::executor::spawn;
    use linear;
    use repoinfo::RepoGenCache;
    use setcommon::NotReadyEmptyStream;
    use std::sync::Arc;
    use string_to_nodehash;

    #[test]
    fn difference_identical_node() {
        let repo = Arc::new(linear::getrepo());
        let repo_generation = RepoGenCache::new(10);

        let head_hash = string_to_nodehash("a5ffa77602a066db7d5cfb9fb5823a0895717c5a");
        let nodestream = Box::new(SetDifferenceNodeStream::new(
            &repo,
            repo_generation,
            Box::new(SingleNodeHash::new(head_hash.clone(), &repo)),
            Box::new(SingleNodeHash::new(head_hash.clone(), &repo)),
        ));

        assert_node_sequence(vec![], nodestream);
    }

    #[test]
    fn difference_node_and_empty() {
        let repo = Arc::new(linear::getrepo());
        let repo_generation = RepoGenCache::new(10);

        let head_hash = string_to_nodehash("a5ffa77602a066db7d5cfb9fb5823a0895717c5a");
        let nodestream = Box::new(SetDifferenceNodeStream::new(
            &repo,
            repo_generation,
            Box::new(SingleNodeHash::new(head_hash.clone(), &repo)),
            Box::new(NotReadyEmptyStream { poll_count: 0 }),
        ));

        assert_node_sequence(vec![head_hash], nodestream);
    }

    #[test]
    fn difference_empty_and_node() {
        let repo = Arc::new(linear::getrepo());
        let repo_generation = RepoGenCache::new(10);

        let head_hash = string_to_nodehash("a5ffa77602a066db7d5cfb9fb5823a0895717c5a");
        let nodestream = Box::new(SetDifferenceNodeStream::new(
            &repo,
            repo_generation,
            Box::new(NotReadyEmptyStream { poll_count: 0 }),
            Box::new(SingleNodeHash::new(head_hash.clone(), &repo)),
        ));

        assert_node_sequence(vec![], nodestream);
    }

    #[test]
    fn difference_two_nodes() {
        let repo = Arc::new(linear::getrepo());
        let repo_generation = RepoGenCache::new(10);

        let nodestream = Box::new(SetDifferenceNodeStream::new(
            &repo,
            repo_generation,
            Box::new(SingleNodeHash::new(
                string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                &repo,
            )),
            Box::new(SingleNodeHash::new(
                string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                &repo,
            )),
        ));

        assert_node_sequence(
            vec![
                string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
            ],
            nodestream,
        );
    }

    #[test]
    fn difference_error_node() {
        let repo = Arc::new(linear::getrepo());
        let repo_generation = RepoGenCache::new(10);

        let nodehash = string_to_nodehash("0000000000000000000000000000000000000000");
        let mut nodestream = spawn(Box::new(SetDifferenceNodeStream::new(
            &repo,
            repo_generation,
            Box::new(SingleNodeHash::new(nodehash.clone(), &repo)),
            Box::new(SingleNodeHash::new(nodehash.clone(), &repo)),
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
    fn slow_ready_difference_nothing() {
        // Tests that we handle an input staying at NotReady for a while without panicing
        let repeats = 10;
        let repo = Arc::new(linear::getrepo());
        let repo_generation = RepoGenCache::new(10);
        let mut nodestream = Box::new(SetDifferenceNodeStream::new(
            &repo,
            repo_generation,
            Box::new(NotReadyEmptyStream {
                poll_count: repeats,
            }),
            Box::new(NotReadyEmptyStream {
                poll_count: repeats,
            }),
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
            "Set difference of something that's not ready {} times failed to complete",
            repeats
        );
    }

    #[test]
    fn difference_union_with_single_node() {
        let repo = Arc::new(linear::getrepo());
        let repo_generation = RepoGenCache::new(10);

        let inputs: Vec<Box<NodeStream>> = vec![
            Box::new(SingleNodeHash::new(
                string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                &repo,
            )),
            Box::new(SingleNodeHash::new(
                string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157"),
                &repo,
            )),
            Box::new(SingleNodeHash::new(
                string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                &repo,
            )),
        ];
        let nodestream = Box::new(UnionNodeStream::new(
            &repo,
            repo_generation.clone(),
            inputs.into_iter(),
        ));

        let nodestream = Box::new(SetDifferenceNodeStream::new(
            &repo,
            repo_generation,
            nodestream,
            Box::new(SingleNodeHash::new(
                string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                &repo,
            )),
        ));

        assert_node_sequence(
            vec![
                string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157"),
                string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
            ],
            nodestream,
        );
    }

    #[test]
    fn difference_single_node_with_union() {
        let repo = Arc::new(linear::getrepo());
        let repo_generation = RepoGenCache::new(10);

        let inputs: Vec<Box<NodeStream>> = vec![
            Box::new(SingleNodeHash::new(
                string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                &repo,
            )),
            Box::new(SingleNodeHash::new(
                string_to_nodehash("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157"),
                &repo,
            )),
            Box::new(SingleNodeHash::new(
                string_to_nodehash("d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                &repo,
            )),
        ];
        let nodestream = Box::new(UnionNodeStream::new(
            &repo,
            repo_generation.clone(),
            inputs.into_iter(),
        ));

        let nodestream = Box::new(SetDifferenceNodeStream::new(
            &repo,
            repo_generation,
            Box::new(SingleNodeHash::new(
                string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                &repo,
            )),
            nodestream,
        ));

        assert_node_sequence(vec![], nodestream);
    }
}
