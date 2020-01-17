/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use futures::stream::Stream;
use futures::Async;
use futures::Poll;
use mononoke_types::{ChangesetId, Generation};
use std::collections::hash_map::IntoIter;
use std::collections::HashMap;
use std::iter::IntoIterator;
use std::mem::replace;
use std::sync::Arc;

use crate::setcommon::*;
use crate::BonsaiNodeStream;

pub struct IntersectNodeStream {
    inputs: Vec<(
        BonsaiInputStream,
        Poll<Option<(ChangesetId, Generation)>, Error>,
    )>,
    current_generation: Option<Generation>,
    accumulator: HashMap<ChangesetId, usize>,
    drain: Option<IntoIter<ChangesetId, usize>>,
}

impl IntersectNodeStream {
    pub fn new<I>(
        ctx: CoreContext,
        changeset_fetcher: &Arc<dyn ChangesetFetcher>,
        inputs: I,
    ) -> Self
    where
        I: IntoIterator<Item = BonsaiNodeStream>,
    {
        let csid_and_gen = inputs.into_iter().map({
            move |i| {
                (
                    add_generations_by_bonsai(ctx.clone(), i, changeset_fetcher.clone()),
                    Ok(Async::NotReady),
                )
            }
        });
        Self {
            inputs: csid_and_gen.collect(),
            current_generation: None,
            accumulator: HashMap::new(),
            drain: None,
        }
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
                .min();
        }
    }

    fn accumulate_nodes(&mut self) {
        let mut found_csids = false;
        for &mut (_, ref mut state) in self.inputs.iter_mut() {
            if let Ok(Async::Ready(Some((csid, gen_id)))) = *state {
                if Some(gen_id) == self.current_generation {
                    *self.accumulator.entry(csid).or_insert(0) += 1;
                }
                // Inputs of higher generation than the current one get consumed and dropped
                if Some(gen_id) >= self.current_generation {
                    found_csids = true;
                    *state = Ok(Async::NotReady);
                }
            }
        }
        if !found_csids {
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
    type Item = ChangesetId;
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
                    let (csid, count) = next_in_drain.expect("is_some() said this was safe");
                    if count == self.inputs.len() {
                        return Ok(Async::Ready(Some(csid)));
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
                None => {
                    if self.accumulator.is_empty() {
                        self.update_current_generation();
                    } else {
                        let full_accumulator = replace(&mut self.accumulator, HashMap::new());
                        self.drain = Some(full_accumulator.into_iter());
                    }
                }
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
    use crate::async_unit;
    use crate::errors::ErrorKind;
    use crate::fixtures::linear;
    use crate::fixtures::unshared_merge_even;
    use crate::fixtures::unshared_merge_uneven;
    use crate::setcommon::NotReadyEmptyStream;
    use crate::tests::get_single_bonsai_streams;
    use crate::tests::TestChangesetFetcher;
    use crate::BonsaiNodeStream;
    use crate::UnionNodeStream;
    use context::CoreContext;
    use failure_ext::err_downcast;
    use fbinit::FacebookInit;
    use futures::executor::spawn;
    use futures_ext::StreamExt;
    use revset_test_helper::assert_changesets_sequence;
    use revset_test_helper::{single_changeset_id, string_to_bonsai};
    use std::sync::Arc;

    #[fbinit::test]
    fn intersect_identical_node(fb: FacebookInit) {
        async_unit::tokio_unit_test(move || {
            let ctx = CoreContext::test_mock(fb);
            let repo = Arc::new(linear::getrepo(fb));
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            let hash = "a5ffa77602a066db7d5cfb9fb5823a0895717c5a";
            let head_csid = string_to_bonsai(fb, &repo, hash);

            let inputs: Vec<BonsaiNodeStream> = vec![
                single_changeset_id(ctx.clone(), head_csid.clone(), &repo).boxify(),
                single_changeset_id(ctx.clone(), head_csid.clone(), &repo).boxify(),
            ];

            let nodestream =
                IntersectNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter())
                    .boxify();

            assert_changesets_sequence(ctx, &repo, vec![head_csid], nodestream);
        });
    }

    #[fbinit::test]
    fn intersect_three_different_nodes(fb: FacebookInit) {
        async_unit::tokio_unit_test(move || {
            let ctx = CoreContext::test_mock(fb);
            let repo = Arc::new(linear::getrepo(fb));
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            let bcs_a947 = string_to_bonsai(fb, &repo, "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157");
            let bcs_3c15 = string_to_bonsai(fb, &repo, "3c15267ebf11807f3d772eb891272b911ec68759");
            let bcs_d0a = string_to_bonsai(fb, &repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0");
            // Note that these are *not* in generation order deliberately.
            let inputs: Vec<BonsaiNodeStream> = vec![
                single_changeset_id(ctx.clone(), bcs_a947, &repo).boxify(),
                single_changeset_id(ctx.clone(), bcs_3c15, &repo).boxify(),
                single_changeset_id(ctx.clone(), bcs_d0a, &repo).boxify(),
            ];

            let nodestream =
                IntersectNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter())
                    .boxify();

            assert_changesets_sequence(ctx, &repo, vec![], nodestream);
        });
    }

    #[fbinit::test]
    fn intersect_three_identical_nodes(fb: FacebookInit) {
        async_unit::tokio_unit_test(move || {
            let ctx = CoreContext::test_mock(fb);
            let repo = Arc::new(linear::getrepo(fb));
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            let bcs_d0a = string_to_bonsai(fb, &repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0");
            let inputs: Vec<BonsaiNodeStream> = vec![
                single_changeset_id(ctx.clone(), bcs_d0a, &repo).boxify(),
                single_changeset_id(ctx.clone(), bcs_d0a, &repo).boxify(),
                single_changeset_id(ctx.clone(), bcs_d0a, &repo).boxify(),
            ];
            let nodestream =
                IntersectNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter())
                    .boxify();

            assert_changesets_sequence(ctx.clone(), &repo, vec![bcs_d0a], nodestream);
        });
    }

    #[fbinit::test]
    fn intersect_nesting(fb: FacebookInit) {
        async_unit::tokio_unit_test(move || {
            let ctx = CoreContext::test_mock(fb);
            let repo = Arc::new(linear::getrepo(fb));
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            let bcs_3c15 = string_to_bonsai(fb, &repo, "3c15267ebf11807f3d772eb891272b911ec68759");
            let inputs: Vec<BonsaiNodeStream> = vec![
                single_changeset_id(ctx.clone(), bcs_3c15.clone(), &repo).boxify(),
                single_changeset_id(ctx.clone(), bcs_3c15.clone(), &repo).boxify(),
            ];

            let nodestream =
                IntersectNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter())
                    .boxify();

            let inputs: Vec<BonsaiNodeStream> = vec![
                nodestream,
                single_changeset_id(ctx.clone(), bcs_3c15.clone(), &repo).boxify(),
            ];
            let nodestream =
                IntersectNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter())
                    .boxify();

            assert_changesets_sequence(ctx.clone(), &repo, vec![bcs_3c15.clone()], nodestream);
        });
    }

    #[fbinit::test]
    fn intersection_of_unions(fb: FacebookInit) {
        async_unit::tokio_unit_test(move || {
            let ctx = CoreContext::test_mock(fb);
            let repo = Arc::new(linear::getrepo(fb));
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            let hash1 = "d0a361e9022d226ae52f689667bd7d212a19cfe0";
            let hash2 = "3c15267ebf11807f3d772eb891272b911ec68759";
            let hash3 = "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157";

            let inputs = get_single_bonsai_streams(ctx.clone(), &repo, &vec![hash1, hash2]);
            let nodestream =
                UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter()).boxify();

            // This set has a different node sequence, so that we can demonstrate that we skip nodes
            // when they're not going to contribute.
            let inputs = get_single_bonsai_streams(ctx.clone(), &repo, &[hash3, hash2, hash1]);
            let nodestream2 =
                UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter()).boxify();

            let inputs: Vec<BonsaiNodeStream> = vec![nodestream, nodestream2];
            let nodestream =
                IntersectNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter())
                    .boxify();

            assert_changesets_sequence(
                ctx.clone(),
                &repo,
                vec![
                    string_to_bonsai(fb, &repo, "3c15267ebf11807f3d772eb891272b911ec68759"),
                    string_to_bonsai(fb, &repo, "d0a361e9022d226ae52f689667bd7d212a19cfe0"),
                ],
                nodestream,
            );
        });
    }

    #[fbinit::test]
    fn intersect_error_node(fb: FacebookInit) {
        async_unit::tokio_unit_test(move || {
            let ctx = CoreContext::test_mock(fb);
            let repo = Arc::new(linear::getrepo(fb));
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            let hash = "a5ffa77602a066db7d5cfb9fb5823a0895717c5a";
            let changeset = string_to_bonsai(fb, &repo, hash);

            let inputs: Vec<BonsaiNodeStream> = vec![
                RepoErrorStream { item: changeset }.boxify(),
                single_changeset_id(ctx.clone(), changeset, &repo).boxify(),
            ];
            let mut nodestream = spawn(
                IntersectNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter())
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

    #[fbinit::test]
    fn intersect_nothing(fb: FacebookInit) {
        async_unit::tokio_unit_test(move || {
            let ctx = CoreContext::test_mock(fb);
            let repo = Arc::new(linear::getrepo(fb));
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            let inputs: Vec<BonsaiNodeStream> = vec![];
            let nodestream =
                IntersectNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter());
            assert_changesets_sequence(ctx, &repo, vec![], nodestream.boxify());
        });
    }

    #[fbinit::test]
    fn slow_ready_intersect_nothing(fb: FacebookInit) {
        async_unit::tokio_unit_test(move || {
            let ctx = CoreContext::test_mock(fb);
            // Tests that we handle an input staying at NotReady for a while without panicing
            let repeats = 10;
            let repo = Arc::new(linear::getrepo(fb));
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));
            let inputs: Vec<BonsaiNodeStream> = vec![NotReadyEmptyStream::new(repeats).boxify()];
            let mut nodestream =
                IntersectNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter())
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
                "Intersect of something that's not ready {} times failed to complete",
                repeats
            );
        });
    }

    #[fbinit::test]
    fn intersect_unshared_merge_even(fb: FacebookInit) {
        async_unit::tokio_unit_test(move || {
            let ctx = CoreContext::test_mock(fb);
            let repo = Arc::new(unshared_merge_even::getrepo(fb));
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            // Post-merge, merge, and both unshared branches
            let inputs = get_single_bonsai_streams(
                ctx.clone(),
                &repo,
                &[
                    "7fe9947f101acb4acf7d945e69f0d6ce76a81113",
                    "d592490c4386cdb3373dd93af04d563de199b2fb",
                    "33fb49d8a47b29290f5163e30b294339c89505a2",
                    "03b0589d9788870817d03ce7b87516648ed5b33a",
                ],
            );

            let left_nodestream =
                UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter()).boxify();

            // Four commits from one branch
            let inputs = get_single_bonsai_streams(
                ctx.clone(),
                &repo,
                &[
                    "03b0589d9788870817d03ce7b87516648ed5b33a",
                    "2fa8b4ee6803a18db4649a3843a723ef1dfe852b",
                    "0b94a2881dda90f0d64db5fae3ee5695a38e7c8f",
                    "f61fdc0ddafd63503dcd8eed8994ec685bfc8941",
                ],
            );
            let right_nodestream =
                UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter()).boxify();

            let inputs: Vec<BonsaiNodeStream> = vec![left_nodestream, right_nodestream];
            let nodestream =
                IntersectNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter());

            assert_changesets_sequence(
                ctx.clone(),
                &repo,
                vec![string_to_bonsai(
                    fb,
                    &repo,
                    "03b0589d9788870817d03ce7b87516648ed5b33a",
                )],
                nodestream.boxify(),
            );
        });
    }

    #[fbinit::test]
    fn intersect_unshared_merge_uneven(fb: FacebookInit) {
        async_unit::tokio_unit_test(move || {
            let ctx = CoreContext::test_mock(fb);
            let repo = Arc::new(unshared_merge_uneven::getrepo(fb));
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));

            // Post-merge, merge, and both unshared branches
            let inputs = get_single_bonsai_streams(
                ctx.clone(),
                &repo,
                &[
                    "dd993aab2bed7276e17c88470286ba8459ba6d94",
                    "9c6dd4e2c2f43c89613b094efb426cc42afdee2a",
                    "64011f64aaf9c2ad2e674f57c033987da4016f51",
                    "03b0589d9788870817d03ce7b87516648ed5b33a",
                ],
            );

            let left_nodestream =
                UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter()).boxify();

            // Four commits from one branch
            let inputs = get_single_bonsai_streams(
                ctx.clone(),
                &repo,
                &[
                    "03b0589d9788870817d03ce7b87516648ed5b33a",
                    "2fa8b4ee6803a18db4649a3843a723ef1dfe852b",
                    "0b94a2881dda90f0d64db5fae3ee5695a38e7c8f",
                    "f61fdc0ddafd63503dcd8eed8994ec685bfc8941",
                ],
            );
            let right_nodestream =
                UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter()).boxify();

            let inputs: Vec<BonsaiNodeStream> = vec![left_nodestream, right_nodestream];
            let nodestream =
                IntersectNodeStream::new(ctx.clone(), &changeset_fetcher, inputs.into_iter())
                    .boxify();

            assert_changesets_sequence(
                ctx.clone(),
                &repo,
                vec![string_to_bonsai(
                    fb,
                    &repo,
                    "03b0589d9788870817d03ce7b87516648ed5b33a",
                )],
                nodestream,
            );
        });
    }
}
