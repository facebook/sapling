// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use futures::stream::Stream;
use futures::Async;
use futures::Poll;
use mononoke_types::{ChangesetId, Generation};
use std::boxed::Box;
use std::collections::hash_set::IntoIter;
use std::collections::HashSet;
use std::iter::IntoIterator;
use std::mem::replace;
use std::sync::Arc;

use crate::failure::Error;

use crate::setcommon::*;
use crate::BonsaiNodeStream;

pub struct UnionNodeStream {
    inputs: Vec<(
        BonsaiInputStream,
        Poll<Option<(ChangesetId, Generation)>, Error>,
    )>,
    current_generation: Option<Generation>,
    accumulator: HashSet<ChangesetId>,
    drain: Option<IntoIter<ChangesetId>>,
}

impl UnionNodeStream {
    pub fn new<I>(
        ctx: CoreContext,
        changeset_fetcher: &Arc<dyn ChangesetFetcher>,
        inputs: I,
    ) -> Self
    where
        I: IntoIterator<Item = Box<BonsaiNodeStream>>,
    {
        let csid_and_gen = inputs.into_iter().map(move |i| {
            (
                add_generations_by_bonsai(ctx.clone(), i, changeset_fetcher.clone()),
                Ok(Async::NotReady),
            )
        });
        Self {
            inputs: csid_and_gen.collect(),
            current_generation: None,
            accumulator: HashSet::new(),
            drain: None,
        }
    }

    pub fn boxed(self) -> Box<BonsaiNodeStream> {
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
            self.current_generation = self
                .inputs
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
        let mut found_csids = false;
        for &mut (_, ref mut state) in self.inputs.iter_mut() {
            if let Ok(Async::Ready(Some((csid, gen_id)))) = *state {
                if Some(gen_id) == self.current_generation {
                    found_csids = true;
                    self.accumulator.insert(csid);
                    *state = Ok(Async::NotReady);
                }
            }
        }
        if !found_csids {
            self.current_generation = None;
        }
    }
}

impl Stream for UnionNodeStream {
    type Item = ChangesetId;
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
                None => {
                    if self.accumulator.is_empty() {
                        self.update_current_generation();
                    } else {
                        let full_accumulator = replace(&mut self.accumulator, HashSet::new());
                        self.drain = Some(full_accumulator.into_iter());
                    }
                }
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
    use crate::async_unit;
    use crate::errors::ErrorKind;
    use crate::fixtures::{branch_even, branch_uneven, branch_wide, linear};
    use crate::setcommon::{NotReadyEmptyStream, RepoErrorStream};
    use crate::tests::get_single_bonsai_streams;
    use crate::tests::TestChangesetFetcher;
    use crate::BonsaiNodeStream;
    use context::CoreContext;
    use futures::executor::spawn;
    use futures_ext::StreamExt;
    use revset_test_helper::assert_changesets_sequence;
    use revset_test_helper::{single_changeset_id, string_to_bonsai};
    use std::sync::Arc;

    #[test]
    fn union_identical_node() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo(None));
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            let hash = "a5ffa77602a066db7d5cfb9fb5823a0895717c5a";
            let head_csid = string_to_bonsai(&repo, hash);

            let inputs: Vec<Box<BonsaiNodeStream>> = vec![
                single_changeset_id(ctx.clone(), head_csid.clone(), &repo).boxify(),
                single_changeset_id(ctx.clone(), head_csid.clone(), &repo).boxify(),
            ];
            let nodestream =
                UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter()).boxify();

            assert_changesets_sequence(ctx.clone(), &repo, vec![head_csid.clone()], nodestream);
        });
    }

    #[test]
    fn union_error_node() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo(None));
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            let hash = "a5ffa77602a066db7d5cfb9fb5823a0895717c5a";
            let expected_csid = string_to_bonsai(&repo, hash);

            let inputs: Vec<Box<BonsaiNodeStream>> = vec![
                Box::new(RepoErrorStream {
                    item: expected_csid,
                }),
                single_changeset_id(ctx.clone(), expected_csid.clone(), &repo).boxify(),
            ];
            let mut nodestream = spawn(
                UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter()).boxify(),
            );

            match nodestream.wait_stream() {
                Some(Err(err)) => match err_downcast!(err, err: ErrorKind => err) {
                    Ok(ErrorKind::RepoChangesetError(cs)) => assert_eq!(cs, expected_csid),
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
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo(None));
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            let bcs_d0a = string_to_bonsai(&repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0");
            let bcs_3c1 = string_to_bonsai(&repo, "3c15267ebf11807f3d772eb891272b911ec68759");
            let bcs_a947 = string_to_bonsai(&repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157");
            // Note that these are *not* in generation order deliberately.
            let inputs: Vec<Box<BonsaiNodeStream>> = vec![
                single_changeset_id(ctx.clone(), bcs_a947, &repo).boxify(),
                single_changeset_id(ctx.clone(), bcs_3c1, &repo).boxify(),
                single_changeset_id(ctx.clone(), bcs_d0a, &repo).boxify(),
            ];
            let nodestream =
                UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter()).boxify();

            // But, once I hit the asserts, I expect them in generation order.
            assert_changesets_sequence(
                ctx.clone(),
                &repo,
                vec![bcs_3c1, bcs_a947, bcs_d0a],
                nodestream,
            );
        });
    }

    #[test]
    fn union_nothing() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo(None));
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            let inputs: Vec<Box<BonsaiNodeStream>> = vec![];
            let nodestream =
                UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter()).boxify();
            assert_changesets_sequence(ctx.clone(), &repo, vec![], nodestream);
        });
    }

    #[test]
    fn union_nesting() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(linear::getrepo(None));
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            let bcs_d0a = string_to_bonsai(&repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0");
            let bcs_3c1 = string_to_bonsai(&repo, "3c15267ebf11807f3d772eb891272b911ec68759");
            // Note that these are *not* in generation order deliberately.
            let inputs: Vec<Box<BonsaiNodeStream>> = vec![
                single_changeset_id(ctx.clone(), bcs_d0a, &repo).boxify(),
                single_changeset_id(ctx.clone(), bcs_3c1, &repo).boxify(),
            ];

            let nodestream =
                UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter()).boxify();

            let bcs_a947 = string_to_bonsai(&repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157");
            let inputs: Vec<Box<BonsaiNodeStream>> = vec![
                nodestream,
                single_changeset_id(ctx.clone(), bcs_a947, &repo).boxify(),
            ];
            let nodestream =
                UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter()).boxify();

            assert_changesets_sequence(
                ctx.clone(),
                &repo,
                vec![bcs_3c1, bcs_a947, bcs_d0a],
                nodestream,
            );
        });
    }

    #[test]
    fn slow_ready_union_nothing() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            // Tests that we handle an input staying at NotReady for a while without panicing
            let repeats = 10;
            let repo = Arc::new(linear::getrepo(None));
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            let inputs: Vec<Box<BonsaiNodeStream>> =
                vec![Box::new(NotReadyEmptyStream::new(repeats))];
            let mut nodestream =
                UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter()).boxify();

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
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(branch_even::getrepo(None));
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            let nodes = vec![
                string_to_bonsai(&repo, "4f7f3fd428bec1a48f9314414b063c706d9c1aed"),
                string_to_bonsai(&repo, "3cda5c78aa35f0f5b09780d971197b51cad4613a"),
                string_to_bonsai(&repo, "d7542c9db7f4c77dab4b315edd328edf1514952f"),
            ];

            // Two nodes should share the same generation number
            let inputs: Vec<Box<BonsaiNodeStream>> = nodes
                .clone()
                .into_iter()
                .map(|cs| single_changeset_id(ctx.clone(), cs, &repo).boxify())
                .collect();
            let nodestream =
                UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter()).boxify();
            assert_changesets_sequence(ctx.clone(), &repo, nodes, nodestream);
        });
    }

    #[test]
    fn union_branch_uneven_repo() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(branch_uneven::getrepo(None));
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            let cs_1 = string_to_bonsai(&repo, "3cda5c78aa35f0f5b09780d971197b51cad4613a");
            let cs_2 = string_to_bonsai(&repo, "d7542c9db7f4c77dab4b315edd328edf1514952f");
            let cs_3 = string_to_bonsai(&repo, "4f7f3fd428bec1a48f9314414b063c706d9c1aed");
            let cs_4 = string_to_bonsai(&repo, "bc7b4d0f858c19e2474b03e442b8495fd7aeef33");
            let cs_5 = string_to_bonsai(&repo, "264f01429683b3dd8042cb3979e8bf37007118bc");
            // Two nodes should share the same generation number
            let inputs: Vec<Box<BonsaiNodeStream>> = vec![
                single_changeset_id(ctx.clone(), cs_1.clone(), &repo).boxify(),
                single_changeset_id(ctx.clone(), cs_2.clone(), &repo).boxify(),
                single_changeset_id(ctx.clone(), cs_3.clone(), &repo).boxify(),
                single_changeset_id(ctx.clone(), cs_4.clone(), &repo).boxify(),
                single_changeset_id(ctx.clone(), cs_5.clone(), &repo).boxify(),
            ];
            let nodestream =
                UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter()).boxify();

            assert_changesets_sequence(
                ctx.clone(),
                &repo,
                vec![cs_5, cs_4, cs_3, cs_1, cs_2],
                nodestream,
            );
        });
    }

    #[test]
    fn union_branch_wide_repo() {
        async_unit::tokio_unit_test(|| {
            let ctx = CoreContext::test_mock();
            let repo = Arc::new(branch_wide::getrepo(None));
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            // Two nodes should share the same generation number
            let inputs = get_single_bonsai_streams(
                ctx.clone(),
                &repo,
                &[
                    "49f53ab171171b3180e125b918bd1cf0af7e5449",
                    "4685e9e62e4885d477ead6964a7600c750e39b03",
                    "c27ef5b7f15e9930e5b93b1f32cc2108a2aabe12",
                    "9e8521affb7f9d10e9551a99c526e69909042b20",
                ],
            );
            let nodestream =
                UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter()).boxify();

            assert_changesets_sequence(
                ctx.clone(),
                &repo,
                vec![
                    string_to_bonsai(&repo, "49f53ab171171b3180e125b918bd1cf0af7e5449"),
                    string_to_bonsai(&repo, "c27ef5b7f15e9930e5b93b1f32cc2108a2aabe12"),
                    string_to_bonsai(&repo, "4685e9e62e4885d477ead6964a7600c750e39b03"),
                    string_to_bonsai(&repo, "9e8521affb7f9d10e9551a99c526e69909042b20"),
                ],
                nodestream,
            );
        });
    }
}
