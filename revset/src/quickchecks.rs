// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::tests::*;

#[cfg(test)]
mod test {
    use super::*;
    use crate::ancestors::AncestorsNodeStream;
    use crate::ancestorscombinators::DifferenceOfUnionsOfAncestorsNodeStream;
    use crate::async_unit;
    use crate::fixtures::branch_even;
    use crate::fixtures::branch_uneven;
    use crate::fixtures::branch_wide;
    use crate::fixtures::linear;
    use crate::fixtures::merge_even;
    use crate::fixtures::merge_uneven;
    use crate::fixtures::unshared_merge_even;
    use crate::fixtures::unshared_merge_uneven;
    use crate::intersectnodestream::IntersectNodeStream;
    use crate::quickcheck::rand::{
        distributions::{range::Range, Sample},
        thread_rng, Rng,
    };
    use crate::quickcheck::{quickcheck, Arbitrary, Gen};
    use crate::setdifferencenodestream::SetDifferenceNodeStream;
    use crate::unionnodestream::UnionNodeStream;
    use crate::validation::ValidateNodeStream;
    use crate::BonsaiNodeStream;
    use blobrepo::BlobRepo;
    use changeset_fetcher::ChangesetFetcher;
    use cloned::cloned;
    use context::CoreContext;
    use failure::Error;
    use futures::executor::spawn;
    use futures::{
        future::{join_all, ok},
        Stream,
    };
    use futures_ext::{BoxFuture, BoxStream, StreamExt};
    use mononoke_types::ChangesetId;
    use revset_test_helper::single_changeset_id;
    use skiplist::SkiplistIndex;
    use std::collections::HashSet;
    use std::iter::Iterator;
    use std::sync::Arc;

    #[derive(Clone, Copy, Debug)]
    enum RevsetEntry {
        SingleNode(Option<ChangesetId>),
        SetDifference,
        Intersect(usize),
        Union(usize),
    }

    #[derive(Clone, Debug)]
    pub struct RevsetSpec {
        rp_entries: Vec<RevsetEntry>,
    }

    fn get_changesets_from_repo(ctx: CoreContext, repo: &BlobRepo) -> Vec<ChangesetId> {
        let changeset_fetcher = repo.get_changeset_fetcher();
        let mut all_changesets_executor = spawn(
            repo.get_bonsai_heads_maybe_stale(ctx.clone())
                .map({
                    cloned!(ctx);
                    move |head| AncestorsNodeStream::new(ctx.clone(), &changeset_fetcher, head)
                })
                .flatten(),
        );

        let mut all_changesets: Vec<ChangesetId> = Vec::new();
        loop {
            all_changesets.push(match all_changesets_executor.wait_stream() {
                None => break,
                Some(changeset) => changeset.expect("Failed to get changesets from repo"),
            });
        }

        assert!(!all_changesets.is_empty(), "Repo has no changesets");
        all_changesets
    }

    impl RevsetSpec {
        pub fn add_hashes<G>(&mut self, ctx: CoreContext, repo: &BlobRepo, random: &mut G)
        where
            G: Rng,
        {
            let all_changesets = get_changesets_from_repo(ctx, repo);
            for elem in self.rp_entries.iter_mut() {
                if let &mut RevsetEntry::SingleNode(None) = elem {
                    *elem =
                        RevsetEntry::SingleNode(random.choose(all_changesets.as_slice()).cloned());
                }
            }
        }

        pub fn as_hashes(&self) -> HashSet<ChangesetId> {
            let mut output: Vec<HashSet<ChangesetId>> = Vec::new();
            for entry in self.rp_entries.iter() {
                match entry {
                    &RevsetEntry::SingleNode(None) => panic!("You need to add_hashes first!"),
                    &RevsetEntry::SingleNode(Some(hash)) => {
                        let mut item = HashSet::new();
                        item.insert(hash);
                        output.push(item)
                    }
                    &RevsetEntry::SetDifference => {
                        let keep = output.pop().expect("No keep for setdifference");
                        let remove = output.pop().expect("No remove for setdifference");
                        output.push(keep.difference(&remove).map(|x| *x).collect())
                    }
                    &RevsetEntry::Union(size) => {
                        let idx = output.len() - size;
                        let mut inputs = output.split_off(idx).into_iter();
                        let first = inputs.next().expect("No first element");
                        output.push(inputs.fold(first, |a, b| a.union(&b).map(|x| *x).collect()))
                    }
                    &RevsetEntry::Intersect(size) => {
                        let idx = output.len() - size;
                        let mut inputs = output.split_off(idx).into_iter();
                        let first = inputs.next().expect("No first element");
                        output.push(
                            inputs.fold(first, |a, b| a.intersection(&b).map(|x| *x).collect()),
                        )
                    }
                }
            }
            assert!(
                output.len() == 1,
                "output should have been length 1, was {}",
                output.len()
            );
            output.pop().expect("No revisions").into_iter().collect()
        }

        pub fn as_revset(&self, ctx: CoreContext, repo: Arc<BlobRepo>) -> BonsaiNodeStream {
            let mut output: Vec<BonsaiNodeStream> = Vec::with_capacity(self.rp_entries.len());
            let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                Arc::new(TestChangesetFetcher::new(repo.clone()));
            for entry in self.rp_entries.iter() {
                let next_node = ValidateNodeStream::new(
                    ctx.clone(),
                    match entry {
                        &RevsetEntry::SingleNode(None) => panic!("You need to add_hashes first!"),
                        &RevsetEntry::SingleNode(Some(hash)) => single_changeset_id(
                            ctx.clone(),
                            Some(hash).expect(&format!("unknown {}", hash)),
                            &*repo.clone(),
                        )
                        .boxify(),
                        &RevsetEntry::SetDifference => {
                            let keep = output.pop().expect("No keep for setdifference");
                            let remove = output.pop().expect("No remove for setdifference");
                            SetDifferenceNodeStream::new(
                                ctx.clone(),
                                &changeset_fetcher,
                                keep,
                                remove,
                            )
                            .boxify()
                        }
                        &RevsetEntry::Union(size) => {
                            let idx = output.len() - size;
                            let inputs = output.split_off(idx);
                            let nodestream =
                                UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs)
                                    .boxify();
                            nodestream
                        }
                        &RevsetEntry::Intersect(size) => {
                            let idx = output.len() - size;
                            let inputs = output.split_off(idx);
                            let nodestream = IntersectNodeStream::new(
                                ctx.clone(),
                                &repo.get_changeset_fetcher(),
                                inputs,
                            )
                            .boxify();
                            nodestream
                        }
                    },
                    &repo.get_changeset_fetcher().clone(),
                )
                .boxify();
                output.push(next_node);
            }
            assert!(
                output.len() == 1,
                "output should have been length 1, was {}",
                output.len()
            );
            output.pop().expect("No revset entries")
        }
    }

    impl Arbitrary for RevsetSpec {
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            let mut revset: Vec<RevsetEntry> = Vec::with_capacity(g.size());
            let mut revspecs_in_set: usize = 0;
            let mut range = Range::new(0, 4);

            for _ in 0..g.size() {
                if revspecs_in_set == 0 {
                    // Can't add a set operator if we have don't have at least one node
                    revset.push(RevsetEntry::SingleNode(None));
                } else {
                    let input_count = g.gen_range(0, revspecs_in_set) + 1;
                    revset.push(
                        // Bias towards SingleNode if we only have 1 rev
                        match range.sample(g) {
                            0 => RevsetEntry::SingleNode(None),
                            1 => {
                                if revspecs_in_set >= 2 {
                                    revspecs_in_set -= 2;
                                    RevsetEntry::SetDifference
                                } else {
                                    RevsetEntry::SingleNode(None)
                                }
                            }
                            2 => {
                                revspecs_in_set -= input_count;
                                RevsetEntry::Intersect(input_count)
                            }
                            3 => {
                                revspecs_in_set -= input_count;
                                RevsetEntry::Union(input_count)
                            }
                            _ => panic!("Range returned too wide a variation"),
                        },
                    );
                }
                revspecs_in_set += 1;
            }
            assert!(revspecs_in_set > 0, "Did not produce enough revs");

            if revspecs_in_set > 1 {
                let mut range = Range::new(0, 2);
                revset.push(match range.sample(g) {
                    0 => RevsetEntry::Intersect(revspecs_in_set),
                    1 => RevsetEntry::Union(revspecs_in_set),
                    _ => panic!("Range returned too wide a variation"),
                });
            }

            RevsetSpec { rp_entries: revset }
        }

        // TODO(simonfar) We should implement shrink(), but we face the issue of ensuring that the
        // resulting revset only contains one final item.
        // Rough sketch: Take the last element of the Vec, so that we're using the same final reduction
        // type. Vector shrink the rest of the Vec using the standard shrinker. Re-add the final
        // reduction type. Note that we then need to handle the case where the final reduction type
        // is a SetDifference by pure chance.
    }

    fn match_streams(
        expected: BoxStream<ChangesetId, Error>,
        actual: BoxStream<ChangesetId, Error>,
    ) -> bool {
        let mut expected = {
            let mut nodestream = spawn(expected);

            let mut expected = HashSet::new();
            loop {
                let hash = nodestream.wait_stream();
                match hash {
                    Some(hash) => {
                        let hash = hash.expect("unexpected error");
                        expected.insert(hash);
                    }
                    None => {
                        break;
                    }
                }
            }
            expected
        };

        let mut nodestream = spawn(actual);

        while !expected.is_empty() {
            match nodestream.wait_stream() {
                Some(hash) => {
                    let hash = hash.expect("unexpected error");
                    if !expected.remove(&hash) {
                        return false;
                    }
                }
                None => {
                    return false;
                }
            }
        }
        nodestream.wait_stream().is_none() && expected.is_empty()
    }

    fn match_hashset_to_revset(ctx: CoreContext, repo: Arc<BlobRepo>, mut set: RevsetSpec) -> bool {
        set.add_hashes(ctx.clone(), &*repo, &mut thread_rng());
        let mut hashes = set.as_hashes();
        let mut nodestream = spawn(set.as_revset(ctx, repo));

        while !hashes.is_empty() {
            let hash = nodestream
                .wait_stream()
                .expect("Unexpected end of stream")
                .expect("Unexpected error");
            if !hashes.remove(&hash) {
                return false;
            }
        }
        nodestream.wait_stream().is_none() && hashes.is_empty()
    }

    // This is slightly icky. I would like to construct $test_name as setops_$repo, but concat_idents!
    // does not work the way I'd like it to. For now, make the user of this macro pass in both idents
    macro_rules! quickcheck_setops {
        ($test_name:ident, $repo:ident) => {
            #[test]
            fn $test_name() {
                fn prop(set: RevsetSpec) -> bool {
                    async_unit::tokio_unit_test(|| {
                        let ctx = CoreContext::test_mock();
                        let repo = Arc::new($repo::getrepo());
                        match_hashset_to_revset(ctx, repo, set)
                    })
                }

                quickcheck(prop as fn(RevsetSpec) -> bool)
            }
        };
    }

    quickcheck_setops!(setops_branch_even, branch_even);
    quickcheck_setops!(setops_branch_uneven, branch_uneven);
    quickcheck_setops!(setops_branch_wide, branch_wide);
    quickcheck_setops!(setops_linear, linear);
    quickcheck_setops!(setops_merge_even, merge_even);
    quickcheck_setops!(setops_merge_uneven, merge_uneven);
    quickcheck_setops!(setops_unshared_merge_even, unshared_merge_even);
    quickcheck_setops!(setops_unshared_merge_uneven, unshared_merge_uneven);

    // Given a list of hashes, generates all possible combinations where each hash can be included,
    // excluded or discarded. So for [h1] outputs are:
    // ([h1], [])
    // ([], [h1])
    // ([], [])
    struct IncludeExcludeDiscardCombinationsIterator {
        hashes: Vec<ChangesetId>,
        index: u64,
    }

    impl IncludeExcludeDiscardCombinationsIterator {
        fn new(hashes: Vec<ChangesetId>) -> Self {
            Self { hashes, index: 0 }
        }

        fn generate_include_exclude(&self) -> (Vec<ChangesetId>, Vec<ChangesetId>) {
            let mut val = self.index;
            let mut include = vec![];
            let mut exclude = vec![];
            for i in (0..self.hashes.len()).rev() {
                let i_commit_state = val / 3_u64.pow(i as u32);
                val %= 3_u64.pow(i as u32);
                match i_commit_state {
                    0 => {
                        // Do nothing
                    }
                    1 => {
                        include.push(self.hashes[i].clone());
                    }
                    2 => {
                        exclude.push(self.hashes[i].clone());
                    }
                    _ => panic!(""),
                }
            }
            (include, exclude)
        }
    }

    impl Iterator for IncludeExcludeDiscardCombinationsIterator {
        type Item = (Vec<ChangesetId>, Vec<ChangesetId>);

        fn next(&mut self) -> Option<Self::Item> {
            let res = if self.index >= 3_u64.pow(self.hashes.len() as u32) {
                None
            } else {
                Some(self.generate_include_exclude())
            };
            self.index += 1;
            res
        }
    }

    macro_rules! ancestors_check {
        ($test_name:ident, $repo:ident) => {
            #[test]
            fn $test_name() {
                async_unit::tokio_unit_test(|| {
                    let ctx = CoreContext::test_mock();

                    let repo = Arc::new($repo::getrepo());
                    let changeset_fetcher: Arc<dyn ChangesetFetcher> =
                        Arc::new(TestChangesetFetcher::new(repo.clone()));

                    let all_changesets = get_changesets_from_repo(ctx.clone(), &*repo);

                    // Limit the number of changesets, otherwise tests take too much time
                    let max_changesets = 7;
                    let all_changesets: Vec<_> =
                        all_changesets.into_iter().take(max_changesets).collect();
                    let iter = IncludeExcludeDiscardCombinationsIterator::new(all_changesets);
                    for (include, exclude) in iter {
                        let difference_stream = create_skiplist(ctx.clone(), &repo)
                            .map({
                                cloned!(ctx, changeset_fetcher, exclude, include);
                                move |skiplist| {
                                    DifferenceOfUnionsOfAncestorsNodeStream::new_with_excludes(
                                        ctx.clone(),
                                        &changeset_fetcher,
                                        skiplist,
                                        include.clone(),
                                        exclude.clone(),
                                    )
                                }
                            })
                            .flatten_stream()
                            .boxify();

                        let actual = ValidateNodeStream::new(
                            ctx.clone(),
                            difference_stream,
                            &changeset_fetcher,
                        );

                        let mut includes = vec![];
                        for i in include.clone() {
                            includes.push(
                                AncestorsNodeStream::new(ctx.clone(), &changeset_fetcher, i)
                                    .boxify(),
                            );
                        }

                        let mut excludes = vec![];
                        for i in exclude.clone() {
                            excludes.push(
                                AncestorsNodeStream::new(ctx.clone(), &changeset_fetcher, i)
                                    .boxify(),
                            );
                        }
                        let includes =
                            UnionNodeStream::new(ctx.clone(), &changeset_fetcher, includes)
                                .boxify();
                        let excludes =
                            UnionNodeStream::new(ctx.clone(), &changeset_fetcher, excludes)
                                .boxify();
                        let expected = SetDifferenceNodeStream::new(
                            ctx.clone(),
                            &changeset_fetcher,
                            includes,
                            excludes,
                        )
                        .boxify();

                        assert!(
                            match_streams(expected, actual.boxify()),
                            "streams do not match for {:?} {:?}",
                            include,
                            exclude
                        );
                    }
                    ()
                })
            }
        };
    }
    mod empty_skiplist_tests {
        use super::*;
        use futures::Future;
        use futures_ext::FutureExt;

        fn create_skiplist(
            _ctxt: CoreContext,
            _repo: &Arc<BlobRepo>,
        ) -> BoxFuture<Arc<SkiplistIndex>, Error> {
            ok(Arc::new(SkiplistIndex::new())).boxify()
        }

        ancestors_check!(ancestors_check_branch_even, branch_even);
        ancestors_check!(ancestors_check_branch_uneven, branch_uneven);
        ancestors_check!(ancestors_check_branch_wide, branch_wide);
        ancestors_check!(ancestors_check_linear, linear);
        ancestors_check!(ancestors_check_merge_even, merge_even);
        ancestors_check!(ancestors_check_merge_uneven, merge_uneven);
        ancestors_check!(ancestors_check_unshared_merge_even, unshared_merge_even);
        ancestors_check!(ancestors_check_unshared_merge_uneven, unshared_merge_uneven);
    }

    mod full_skiplist_tests {
        use super::*;
        use futures::Future;
        use futures_ext::FutureExt;

        fn create_skiplist(
            ctx: CoreContext,
            repo: &Arc<BlobRepo>,
        ) -> BoxFuture<Arc<SkiplistIndex>, Error> {
            let changeset_fetcher = repo.get_changeset_fetcher();
            let skiplist_index = SkiplistIndex::new();
            let max_index_depth = 100;

            repo.get_bonsai_heads_maybe_stale(ctx.clone())
                .collect()
                .and_then({
                    cloned!(skiplist_index);
                    move |heads| {
                        join_all(heads.into_iter().map({
                            cloned!(skiplist_index);
                            move |head| {
                                skiplist_index.add_node(
                                    ctx.clone(),
                                    changeset_fetcher.clone(),
                                    head,
                                    max_index_depth,
                                )
                            }
                        }))
                    }
                })
                .map(move |_| Arc::new(skiplist_index))
                .boxify()
        }

        ancestors_check!(ancestors_check_branch_even, branch_even);
        ancestors_check!(ancestors_check_branch_uneven, branch_uneven);
        ancestors_check!(ancestors_check_branch_wide, branch_wide);
        ancestors_check!(ancestors_check_linear, linear);
        ancestors_check!(ancestors_check_merge_even, merge_even);
        ancestors_check!(ancestors_check_merge_uneven, merge_uneven);
        ancestors_check!(ancestors_check_unshared_merge_even, unshared_merge_even);
        ancestors_check!(ancestors_check_unshared_merge_uneven, unshared_merge_uneven);
    }
}
