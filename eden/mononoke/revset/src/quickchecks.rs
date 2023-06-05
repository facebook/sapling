/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::tests::*;

#[cfg(test)]
mod test {
    use std::collections::HashSet;
    use std::sync::Arc;

    use blobrepo::BlobRepo;
    use bookmarks::BookmarksMaybeStaleExt;
    use bookmarks::BookmarksRef;
    use changeset_fetcher::ArcChangesetFetcher;
    use changeset_fetcher::ChangesetFetcherArc;
    use cloned::cloned;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use futures::compat::Stream01CompatExt;
    use futures::stream::StreamExt as _;
    use futures::TryStreamExt;
    use futures_ext::StreamExt;
    use futures_old::Stream;
    use mononoke_types::ChangesetId;
    use quickcheck::quickcheck;
    use quickcheck::Arbitrary;
    use quickcheck::Gen;
    use rand::seq::SliceRandom;
    use rand::thread_rng;
    use rand::Rng;
    use revset_test_helper::single_changeset_id;

    use super::*;
    use crate::ancestors::AncestorsNodeStream;
    use crate::fixtures::BranchEven;
    use crate::fixtures::BranchUneven;
    use crate::fixtures::BranchWide;
    use crate::fixtures::Linear;
    use crate::fixtures::MergeEven;
    use crate::fixtures::MergeUneven;
    use crate::fixtures::TestRepoFixture;
    use crate::fixtures::UnsharedMergeEven;
    use crate::fixtures::UnsharedMergeUneven;
    use crate::intersectnodestream::IntersectNodeStream;
    use crate::setdifferencenodestream::SetDifferenceNodeStream;
    use crate::unionnodestream::UnionNodeStream;
    use crate::validation::ValidateNodeStream;
    use crate::BonsaiNodeStream;

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

    async fn get_changesets_from_repo(ctx: CoreContext, repo: &BlobRepo) -> Vec<ChangesetId> {
        let mut all_changesets_stream = repo
            .bookmarks()
            .get_heads_maybe_stale(ctx.clone())
            .compat() // conversion is needed as AncestorsNodeStream is an OldStream
            .map({
                cloned!(ctx);
                move |head| {
                    AncestorsNodeStream::new(ctx.clone(), &repo.changeset_fetcher_arc(), head)
                }
            })
            .flatten()
            .compat();

        let mut all_changesets: Vec<ChangesetId> = Vec::new();
        loop {
            all_changesets.push(match all_changesets_stream.next().await {
                None => break,
                Some(changeset) => changeset.expect("Failed to get changesets from repo"),
            });
        }

        assert!(!all_changesets.is_empty(), "Repo has no changesets");
        all_changesets
    }

    impl RevsetSpec {
        pub async fn add_hashes<G>(&mut self, ctx: CoreContext, repo: &BlobRepo, random: &mut G)
        where
            G: Rng,
        {
            let all_changesets = get_changesets_from_repo(ctx, repo).await;
            for elem in self.rp_entries.iter_mut() {
                if let &mut RevsetEntry::SingleNode(None) = elem {
                    *elem =
                        RevsetEntry::SingleNode(all_changesets.as_slice().choose(random).cloned());
                }
            }
        }

        pub fn as_hashes(&self) -> HashSet<ChangesetId> {
            let mut output: Vec<HashSet<ChangesetId>> = Vec::new();
            for entry in self.rp_entries.iter() {
                match *entry {
                    RevsetEntry::SingleNode(None) => panic!("You need to add_hashes first!"),
                    RevsetEntry::SingleNode(Some(hash)) => {
                        let mut item = HashSet::new();
                        item.insert(hash);
                        output.push(item)
                    }
                    RevsetEntry::SetDifference => {
                        let keep = output.pop().expect("No keep for setdifference");
                        let remove = output.pop().expect("No remove for setdifference");
                        output.push(keep.difference(&remove).copied().collect())
                    }
                    RevsetEntry::Union(size) => {
                        let idx = output.len() - size;
                        let mut inputs = output.split_off(idx).into_iter();
                        let first = inputs.next().expect("No first element");
                        output.push(inputs.fold(first, |a, b| a.union(&b).copied().collect()))
                    }
                    RevsetEntry::Intersect(size) => {
                        let idx = output.len() - size;
                        let mut inputs = output.split_off(idx).into_iter();
                        let first = inputs.next().expect("No first element");
                        output
                            .push(inputs.fold(first, |a, b| a.intersection(&b).copied().collect()))
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

        pub fn as_revset(&self, ctx: CoreContext, repo: BlobRepo) -> BonsaiNodeStream {
            let mut output: Vec<BonsaiNodeStream> = Vec::with_capacity(self.rp_entries.len());
            let changeset_fetcher: ArcChangesetFetcher =
                Arc::new(TestChangesetFetcher::new(repo.clone()));
            for entry in self.rp_entries.iter() {
                let next_node = ValidateNodeStream::new(
                    ctx.clone(),
                    match *entry {
                        RevsetEntry::SingleNode(None) => panic!("You need to add_hashes first!"),
                        RevsetEntry::SingleNode(Some(hash)) => {
                            single_changeset_id(ctx.clone(), hash, &repo).boxify()
                        }
                        RevsetEntry::SetDifference => {
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
                        RevsetEntry::Union(size) => {
                            let idx = output.len() - size;
                            let inputs = output.split_off(idx);

                            UnionNodeStream::new(ctx.clone(), &changeset_fetcher, inputs).boxify()
                        }
                        RevsetEntry::Intersect(size) => {
                            let idx = output.len() - size;
                            let inputs = output.split_off(idx);
                            IntersectNodeStream::new(
                                ctx.clone(),
                                &repo.changeset_fetcher_arc(),
                                inputs,
                            )
                            .boxify()
                        }
                    },
                    &repo.changeset_fetcher_arc(),
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
        fn arbitrary(g: &mut Gen) -> Self {
            let mut revset: Vec<RevsetEntry> = Vec::with_capacity(g.size());
            let mut revspecs_in_set: usize = 0;

            for _ in 0..g.size() {
                if revspecs_in_set == 0 {
                    // Can't add a set operator if we have don't have at least one node
                    revset.push(RevsetEntry::SingleNode(None));
                } else {
                    let input_count = (usize::arbitrary(g) % revspecs_in_set) + 1;
                    revset.push(
                        // Bias towards SingleNode if we only have 1 rev
                        match g.choose(&[0, 1, 2, 3]).unwrap() {
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
                revset.push(match bool::arbitrary(g) {
                    true => RevsetEntry::Intersect(revspecs_in_set),
                    false => RevsetEntry::Union(revspecs_in_set),
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

    async fn match_hashset_to_revset(
        ctx: CoreContext,
        repo: BlobRepo,
        mut set: RevsetSpec,
    ) -> bool {
        set.add_hashes(ctx.clone(), &repo, &mut thread_rng()).await;
        let mut hashes = set.as_hashes();
        let mut nodestream = set.as_revset(ctx, repo).compat();

        while !hashes.is_empty() {
            let hash = nodestream
                .next()
                .await
                .expect("Unexpected end of stream")
                .expect("Unexpected error");
            if !hashes.remove(&hash) {
                return false;
            }
        }
        nodestream.next().await.is_none() && hashes.is_empty()
    }

    // This is slightly icky. I would like to construct $test_name as setops_$repo, but concat_idents!
    // does not work the way I'd like it to. For now, make the user of this macro pass in both idents
    macro_rules! quickcheck_setops {
        ($test_name:ident, $repo:ident) => {
            #[test]
            fn $test_name() {
                #[tokio::main(flavor = "current_thread")]
                async fn prop(fb: FacebookInit, set: RevsetSpec) -> bool {
                    let ctx = CoreContext::test_mock(fb);
                    let repo = $repo::getrepo(fb).await;
                    match_hashset_to_revset(ctx, repo, set).await
                }

                quickcheck(prop as fn(FacebookInit, RevsetSpec) -> bool)
            }
        };
    }

    quickcheck_setops!(setops_branch_even, BranchEven);
    quickcheck_setops!(setops_branch_uneven, BranchUneven);
    quickcheck_setops!(setops_branch_wide, BranchWide);
    quickcheck_setops!(setops_linear, Linear);
    quickcheck_setops!(setops_merge_even, MergeEven);
    quickcheck_setops!(setops_merge_uneven, MergeUneven);
    quickcheck_setops!(setops_unshared_merge_even, UnsharedMergeEven);
    quickcheck_setops!(setops_unshared_merge_uneven, UnsharedMergeUneven);
}
