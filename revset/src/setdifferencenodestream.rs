// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use futures::stream::Stream;
use futures::{Async, Poll};
use mononoke_types::{ChangesetId, Generation};
use std::collections::HashSet;
use std::sync::Arc;

use crate::errors::*;
use crate::setcommon::*;
use crate::BonsaiNodeStream;

pub struct SetDifferenceNodeStream {
    keep_input: BonsaiInputStream,
    next_keep: Async<Option<(ChangesetId, Generation)>>,

    remove_input: BonsaiInputStream,
    next_remove: Async<Option<(ChangesetId, Generation)>>,

    remove_nodes: HashSet<ChangesetId>,
    remove_generation: Option<Generation>,
}

impl SetDifferenceNodeStream {
    pub fn new(
        ctx: CoreContext,
        changeset_fetcher: &Arc<dyn ChangesetFetcher>,
        keep_input: BonsaiNodeStream,
        remove_input: BonsaiNodeStream,
    ) -> SetDifferenceNodeStream {
        SetDifferenceNodeStream {
            keep_input: add_generations_by_bonsai(
                ctx.clone(),
                keep_input,
                changeset_fetcher.clone(),
            ),
            next_keep: Async::NotReady,
            remove_input: add_generations_by_bonsai(
                ctx.clone(),
                remove_input,
                changeset_fetcher.clone(),
            ),
            next_remove: Async::NotReady,
            remove_nodes: HashSet::new(),
            remove_generation: None,
        }
    }

    fn next_keep(&mut self) -> Result<&Async<Option<(ChangesetId, Generation)>>> {
        if self.next_keep.is_not_ready() {
            self.next_keep = self.keep_input.poll()?;
        }
        Ok(&self.next_keep)
    }

    fn next_remove(&mut self) -> Result<&Async<Option<(ChangesetId, Generation)>>> {
        if self.next_remove.is_not_ready() {
            self.next_remove = self.remove_input.poll()?;
        }
        Ok(&self.next_remove)
    }
}

impl Stream for SetDifferenceNodeStream {
    type Item = ChangesetId;
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
    use crate::async_unit;
    use crate::fixtures::linear;
    use crate::fixtures::merge_even;
    use crate::fixtures::merge_uneven;
    use crate::setcommon::NotReadyEmptyStream;
    use crate::tests::get_single_bonsai_streams;
    use crate::tests::TestChangesetFetcher;
    use crate::UnionNodeStream;
    use changeset_fetcher::ChangesetFetcher;
    use context::CoreContext;
    use failure_ext::err_downcast;
    use futures::executor::spawn;
    use futures_ext::StreamExt;
    use revset_test_helper::assert_changesets_sequence;
    use revset_test_helper::{single_changeset_id, string_to_bonsai};
    use std::sync::Arc;

    #[test]
    fn difference_identical_node() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo());
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            let hash = "a5ffa77602a066db7d5cfb9fb5823a0895717c5a";
            let changeset = string_to_bonsai(&repo, hash);
            let nodestream = SetDifferenceNodeStream::new(
                ctx.clone(),
                &changeset_fetcher,
                single_changeset_id(ctx.clone(), changeset.clone(), &repo).boxify(),
                single_changeset_id(ctx.clone(), changeset.clone(), &repo).boxify(),
            )
            .boxify();
            assert_changesets_sequence(ctx.clone(), &repo, vec![], nodestream);
        });
    }

    #[test]
    fn difference_node_and_empty() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo());
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            let hash = "a5ffa77602a066db7d5cfb9fb5823a0895717c5a";
            let changeset = string_to_bonsai(&repo, hash);
            let nodestream = SetDifferenceNodeStream::new(
                ctx.clone(),
                &changeset_fetcher,
                single_changeset_id(ctx.clone(), changeset.clone(), &repo).boxify(),
                NotReadyEmptyStream::new(0).boxify(),
            )
            .boxify();
            assert_changesets_sequence(ctx.clone(), &repo, vec![changeset], nodestream);
        });
    }

    #[test]
    fn difference_empty_and_node() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo());
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            let bcs_id = string_to_bonsai(&repo, "a5ffa77602a066db7d5cfb9fb5823a0895717c5a");

            let nodestream = SetDifferenceNodeStream::new(
                ctx.clone(),
                &changeset_fetcher,
                NotReadyEmptyStream::new(0).boxify(),
                single_changeset_id(ctx.clone(), bcs_id, &repo).boxify(),
            )
            .boxify();

            assert_changesets_sequence(ctx.clone(), &repo, vec![], nodestream);
        });
    }

    #[test]
    fn difference_two_nodes() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo());
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            let bcs_id_1 =
                string_to_bonsai(&repo.clone(), "d0a361e9022d226ae52f689667bd7d212a19cfe0");
            let bcs_id_2 =
                string_to_bonsai(&repo.clone(), "3c15267ebf11807f3d772eb891272b911ec68759");
            let nodestream = SetDifferenceNodeStream::new(
                ctx.clone(),
                &changeset_fetcher,
                single_changeset_id(ctx.clone(), bcs_id_1.clone(), &repo).boxify(),
                single_changeset_id(ctx.clone(), bcs_id_2, &repo).boxify(),
            )
            .boxify();

            assert_changesets_sequence(ctx.clone(), &repo, vec![bcs_id_1], nodestream);
        });
    }

    #[test]
    fn difference_error_node() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo());
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            let hash = "a5ffa77602a066db7d5cfb9fb5823a0895717c5a";
            let changeset = string_to_bonsai(&repo, hash);
            let mut nodestream = spawn(
                SetDifferenceNodeStream::new(
                    ctx.clone(),
                    &changeset_fetcher,
                    RepoErrorStream {
                        item: changeset.clone(),
                    }
                    .boxify(),
                    single_changeset_id(ctx.clone(), changeset, &repo).boxify(),
                )
                .boxify(),
            );

            match nodestream.wait_stream() {
                Some(Err(err)) => match err_downcast!(err, err: ErrorKind => err) {
                    Ok(ErrorKind::RepoChangesetError(cs)) => assert_eq!(cs, changeset),
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
            let ctx = CoreContext::test_mock();
            // Tests that we handle an input staying at NotReady for a while without panicing
            let repeats = 10;
            let repo = Arc::new(linear::getrepo());
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));
            let mut nodestream = SetDifferenceNodeStream::new(
                ctx.clone(),
                &changeset_fetcher,
                NotReadyEmptyStream::new(repeats).boxify(),
                NotReadyEmptyStream::new(repeats).boxify(),
            )
            .boxify();

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
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo());
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            let inputs = get_single_bonsai_streams(
                ctx.clone(),
                &repo,
                &[
                    "3c15267ebf11807f3d772eb891272b911ec68759",
                    "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157",
                    "d0a361e9022d226ae52f689667bd7d212a19cfe0",
                ],
            );

            let nodestream =
                UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter()).boxify();

            let bcs_id =
                string_to_bonsai(&repo.clone(), "3c15267ebf11807f3d772eb891272b911ec68759");
            let nodestream = SetDifferenceNodeStream::new(
                ctx.clone(),
                &changeset_fetcher,
                nodestream,
                single_changeset_id(ctx.clone(), bcs_id, &repo).boxify(),
            )
            .boxify();

            assert_changesets_sequence(
                ctx.clone(),
                &repo,
                vec![
                    string_to_bonsai(&repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157"),
                    string_to_bonsai(&repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                ],
                nodestream,
            );
        });
    }

    #[test]
    fn difference_single_node_with_union() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo());
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            let inputs = get_single_bonsai_streams(
                ctx.clone(),
                &repo,
                &[
                    "3c15267ebf11807f3d772eb891272b911ec68759",
                    "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157",
                    "d0a361e9022d226ae52f689667bd7d212a19cfe0",
                ],
            );
            let nodestream =
                UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter()).boxify();

            let bcs_id =
                string_to_bonsai(&repo.clone(), "3c15267ebf11807f3d772eb891272b911ec68759");
            let nodestream = SetDifferenceNodeStream::new(
                ctx.clone(),
                &changeset_fetcher,
                single_changeset_id(ctx.clone(), bcs_id, &repo).boxify(),
                nodestream,
            )
            .boxify();

            assert_changesets_sequence(ctx.clone(), &repo, vec![], nodestream);
        });
    }

    #[test]
    fn difference_merge_even() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(merge_even::getrepo());
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            // Top three commits in my hg log -G -r 'all()' output
            let inputs = get_single_bonsai_streams(
                ctx.clone(),
                &repo,
                &[
                    "4dcf230cd2f20577cb3e88ba52b73b376a2b3f69",
                    "4f7f3fd428bec1a48f9314414b063c706d9c1aed",
                    "16839021e338500b3cf7c9b871c8a07351697d68",
                ],
            );

            let left_nodestream =
                UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter()).boxify();

            // Everything from base to just before merge on one side
            let inputs = get_single_bonsai_streams(
                ctx.clone(),
                &repo,
                &[
                    "4f7f3fd428bec1a48f9314414b063c706d9c1aed",
                    "b65231269f651cfe784fd1d97ef02a049a37b8a0",
                    "d7542c9db7f4c77dab4b315edd328edf1514952f",
                    "15c40d0abc36d47fb51c8eaec51ac7aad31f669c",
                ],
            );
            let right_nodestream =
                UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter()).boxify();

            let nodestream = SetDifferenceNodeStream::new(
                ctx.clone(),
                &changeset_fetcher,
                left_nodestream,
                right_nodestream,
            )
            .boxify();

            assert_changesets_sequence(
                ctx.clone(),
                &repo,
                vec![
                    string_to_bonsai(&repo, "4dcf230cd2f20577cb3e88ba52b73b376a2b3f69"),
                    string_to_bonsai(&repo, "16839021e338500b3cf7c9b871c8a07351697d68"),
                ],
                nodestream,
            );
        });
    }

    #[test]
    fn difference_merge_uneven() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(merge_uneven::getrepo());
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            // Merge commit, and one from each branch
            let inputs = get_single_bonsai_streams(
                ctx.clone(),
                &repo,
                &[
                    "7221fa26c85f147db37c2b5f4dbcd5fe52e7645b",
                    "4f7f3fd428bec1a48f9314414b063c706d9c1aed",
                    "16839021e338500b3cf7c9b871c8a07351697d68",
                ],
            );
            let left_nodestream =
                UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter()).boxify();

            // Everything from base to just before merge on one side
            let inputs = get_single_bonsai_streams(
                ctx.clone(),
                &repo,
                &[
                    "16839021e338500b3cf7c9b871c8a07351697d68",
                    "1d8a907f7b4bf50c6a09c16361e2205047ecc5e5",
                    "3cda5c78aa35f0f5b09780d971197b51cad4613a",
                    "15c40d0abc36d47fb51c8eaec51ac7aad31f669c",
                ],
            );
            let right_nodestream =
                UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter()).boxify();

            let nodestream = SetDifferenceNodeStream::new(
                ctx.clone(),
                &changeset_fetcher,
                left_nodestream,
                right_nodestream,
            )
            .boxify();

            assert_changesets_sequence(
                ctx.clone(),
                &repo,
                vec![
                    string_to_bonsai(&repo, "7221fa26c85f147db37c2b5f4dbcd5fe52e7645b"),
                    string_to_bonsai(&repo, "4f7f3fd428bec1a48f9314414b063c706d9c1aed"),
                ],
                nodestream,
            );
        });
    }
}
