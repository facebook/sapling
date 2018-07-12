// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashSet;
use std::sync::Arc;

use blobrepo::BlobRepo;
use failure::Error;
use futures::{Async, Poll};
use futures::stream::Stream;
use mercurial_types::HgNodeHash;
use mononoke_types::Generation;

use NodeStream;
use setcommon::{add_generations, InputStream};

/// A wrapper around a NodeStream that asserts that the two revset invariants hold:
/// 1. The generation number never increases
/// 2. No hash is seen twice
/// This uses memory proportional to the number of hashes in the revset.
pub struct ValidateNodeStream {
    wrapped: InputStream,
    last_generation: Option<Generation>,
    seen_hashes: HashSet<HgNodeHash>,
}

impl ValidateNodeStream {
    pub fn new(wrapped: Box<NodeStream>, repo: &Arc<BlobRepo>) -> ValidateNodeStream {
        ValidateNodeStream {
            wrapped: add_generations(wrapped, repo.clone()),
            last_generation: None,
            seen_hashes: HashSet::new(),
        }
    }

    pub fn boxed(self) -> Box<NodeStream> {
        Box::new(self)
    }
}

impl Stream for ValidateNodeStream {
    type Item = HgNodeHash;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let next = self.wrapped.poll()?;

        let (hash, gen) = match next {
            Async::NotReady => return Ok(Async::NotReady),
            Async::Ready(None) => return Ok(Async::Ready(None)),
            Async::Ready(Some((hash, gen))) => (hash, gen),
        };

        assert!(
            self.seen_hashes.insert(hash),
            format!("Hash {} seen twice", hash)
        );

        assert!(
            self.last_generation.is_none() || self.last_generation >= Some(gen),
            "Generation number increased unexpectedly"
        );

        self.last_generation = Some(gen);

        Ok(Async::Ready(Some(hash)))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use SingleNodeHash;
    use async_unit;
    use linear;

    use repoinfo::RepoGenCache;
    use setcommon::NotReadyEmptyStream;
    use std::sync::Arc;
    use tests::assert_node_sequence;
    use tests::string_to_nodehash;

    #[test]
    fn validate_accepts_single_node() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));
            let repo_generation = RepoGenCache::new(10);

            let head_hash = string_to_nodehash("a5ffa77602a066db7d5cfb9fb5823a0895717c5a");

            let nodestream = SingleNodeHash::new(head_hash, &repo);

            let nodestream = ValidateNodeStream::new(Box::new(nodestream), &repo).boxed();
            assert_node_sequence(repo_generation, &repo, vec![head_hash], nodestream);
        });
    }

    #[test]
    fn slow_ready_validates() {
        async_unit::tokio_unit_test(|| {
            // Tests that we handle an input staying at NotReady for a while without panicing
            let repeats = 10;
            let repo = Arc::new(linear::getrepo(None));
            let mut nodestream = ValidateNodeStream::new(
                Box::new(NotReadyEmptyStream {
                    poll_count: repeats,
                }),
                &repo,
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
    #[should_panic]
    fn repeat_hash_panics() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));

            let head_hash = string_to_nodehash("a5ffa77602a066db7d5cfb9fb5823a0895717c5a");
            let nodestream =
                SingleNodeHash::new(head_hash, &repo).chain(SingleNodeHash::new(head_hash, &repo));

            let mut nodestream = ValidateNodeStream::new(Box::new(nodestream), &repo).boxed();

            loop {
                match nodestream.poll() {
                    Ok(Async::Ready(None)) => return,
                    _ => (),
                }
            }
        });
    }

    #[test]
    #[should_panic]
    fn wrong_order_panics() {
        async_unit::tokio_unit_test(|| {
            let repo = Arc::new(linear::getrepo(None));

            let nodestream = SingleNodeHash::new(
                string_to_nodehash("cb15ca4a43a59acff5388cea9648c162afde8372"),
                &repo,
            ).chain(SingleNodeHash::new(
                string_to_nodehash("3c15267ebf11807f3d772eb891272b911ec68759"),
                &repo,
            ));

            let mut nodestream = ValidateNodeStream::new(Box::new(nodestream), &repo).boxed();

            loop {
                match nodestream.poll() {
                    Ok(Async::Ready(None)) => return,
                    _ => (),
                }
            }
        });
    }
}
