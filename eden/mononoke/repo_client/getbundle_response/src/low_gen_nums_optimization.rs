/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::call_difference_of_union_of_ancestors_revset;
use crate::Params;
use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use changeset_fetcher::ArcChangesetFetcher;
use context::CoreContext;
use context::PerfCounterType;
use futures::future::try_join_all;
use futures::future::TryFutureExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;
use reachabilityindex::LeastCommonAncestorsHint;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use tunables::tunables;
use uniqueheap::UniqueHeap;

pub const DEFAULT_TRAVERSAL_LIMIT: u64 = 20;
pub const LOW_GEN_HEADS_LIMIT: u64 = 20;

pub(crate) struct LowGenNumChecker {
    low_gen_num_threshold: Option<u64>,
}

impl LowGenNumChecker {
    pub(crate) fn new_from_tunables(highest_gen_num: u64) -> Self {
        let difference = tunables().get_getbundle_high_low_gen_num_difference_threshold();
        let low_gen_num_threshold = if difference > 0 {
            let difference = difference as u64;
            Some(highest_gen_num.saturating_sub(difference))
        } else {
            None
        };

        Self {
            low_gen_num_threshold,
        }
    }

    #[cfg(test)]
    fn new(low_gen_num_threshold: Option<u64>) -> Self {
        Self {
            low_gen_num_threshold,
        }
    }

    pub(crate) fn is_low_gen_num(&self, gen_num: u64) -> bool {
        match self.low_gen_num_threshold {
            Some(threshold) => gen_num <= threshold,
            None => false,
        }
    }

    fn get_threshold(&self) -> Option<u64> {
        self.low_gen_num_threshold
    }
}

#[derive(Debug)]
pub(crate) struct PartialGetBundle {
    pub(crate) partial: Vec<ChangesetId>,
    pub(crate) new_heads: Vec<(ChangesetId, Generation)>,
    pub(crate) new_excludes: Vec<(ChangesetId, Generation)>,
}

impl PartialGetBundle {
    fn new_no_partial_result(
        new_heads: Vec<(ChangesetId, Generation)>,
        new_excludes: Vec<(ChangesetId, Generation)>,
    ) -> Self {
        Self {
            partial: vec![],
            new_heads,
            new_excludes,
        }
    }
}

/// This is the preprocessing of getbundle parameters - `heads` and `excludes`.
/// It tries to preprocess getbundle parameters in a way that make low_gen_num_optimization
/// kick in later. This helps in a case where a small repo merged in a large repository.
/// In particular, it tries to do a short walk starting from the head with the largest
/// generation number and return:
/// 1) partial answer to the getbundle query - i.e. a set of nodes that are ancestors of head
///    with the largest gen number that will be returned to the client
/// 2) Modified `heads` and `common` parameters.
///
/// The idea of this optimization is that returned `heads` parameters might contain
/// a commit with a low generation number, and this will later be processed by a second
/// `low_gen_num_optimization` and make the overall getbundle call much faster.
///
/// ```text
///   A <- head with largest generation
///   |
///   B
///   | \
///   |  C <- a node with a low generation number
/// ...
///   O <- common with largest generation
///
/// Returns:
/// partial result - [A, B]
/// new_heads[ .., C, ...]
/// new_common: common
/// ```
pub(crate) async fn compute_partial_getbundle(
    ctx: &CoreContext,
    changeset_fetcher: &ArcChangesetFetcher,
    heads: Vec<(ChangesetId, Generation)>,
    excludes: Vec<(ChangesetId, Generation)>,
    low_gen_num_checker: &LowGenNumChecker,
) -> Result<PartialGetBundle, Error> {
    let traversal_limit = tunables().get_getbundle_partial_getbundle_traversal_limit();
    if traversal_limit == 0 {
        // This optimimization is disabled, just exit quickly
        return Ok(PartialGetBundle::new_no_partial_result(heads, excludes));
    }

    let gen_num_threshold = match low_gen_num_checker.get_threshold() {
        Some(threshold) => threshold,
        None => {
            return Ok(PartialGetBundle::new_no_partial_result(heads, excludes));
        }
    };

    ctx.scuba()
        .clone()
        .log_with_msg("Computing partial getbundle", None);
    let maybe_max_head = heads.iter().max_by_key(|node| node.1);

    let mut queue = UniqueHeap::new();
    let mut new_heads: HashMap<_, _> = HashMap::from_iter(heads.clone());
    let new_excludes: HashMap<_, _> = HashMap::from_iter(excludes);

    if let Some((cs_id, gen_num)) = maybe_max_head {
        if !new_excludes.contains_key(cs_id) && gen_num.value() > gen_num_threshold {
            queue.push((*gen_num, *cs_id));
        }
    }

    let mut partial = vec![];
    let mut traversed = 0;

    // Do a BFS traversal starting from a commit with the highest generation number, and return all the
    // visited nodes. We don't visit a node if:
    // 1) It has a very low generation number
    // 2) It is excluded (i.e. it's in new_excludes parameter)
    // 3) We've traversed more or equal than traversal limit
    //
    // All the parents that weren't traversed but also weren't excluded will be added to
    // new_heads
    while let Some((_, cs_id)) = queue.pop() {
        partial.push(cs_id);
        new_heads.remove(&cs_id);

        let parents = changeset_fetcher.get_parents(ctx.clone(), cs_id).await?;
        let parents = try_join_all(parents.into_iter().map(|p| {
            changeset_fetcher
                .get_generation_number(ctx.clone(), p)
                .map_ok(move |gen_num| (p, gen_num))
        }))
        .await?;
        for (p, gen_num) in parents {
            // This parent is excluded - just ignore it
            if new_excludes.contains_key(&p) {
                continue;
            }

            new_heads.insert(p, gen_num);
            if gen_num.value() > gen_num_threshold {
                // We don't visit a parent that has a very low generation number -
                // it will be processed separately
                queue.push((gen_num, p));
            }
        }

        traversed += 1;
        if traversed >= traversal_limit {
            break;
        }
    }

    ctx.perf_counters()
        .add_to_counter(PerfCounterType::GetbundlePartialTraversal, traversed);

    Ok(PartialGetBundle {
        partial,
        new_heads: new_heads.into_iter().collect(),
        new_excludes: new_excludes.into_iter().collect(),
    })
}

/// Optimization for the case when params.heads has values with very low generation number.
///
/// ```text
/// O <- head 1
/// |
/// O <- exclude 1
/// |
/// ...   O <- head 2
/// |     |
/// O     O <- exclude 2
///
/// if exclude 1 has a much larger generation number than head2 then DifferenceOfUnionsOfAncestorsNodeStream
/// has to traverse commit graph starting from exclude 1 downto head 2. Even though skiplist are used to make this
/// traversal faster, it's still quite expensive.
///
/// low_gen_num_optimization instead makes two separate DifferenceOfUnionsOfAncestorsNodeStream calls - one for heads
/// with high generation number, and another for heads and excludes with low generation number.
/// ```
pub(crate) async fn low_gen_num_optimization(
    ctx: &CoreContext,
    changeset_fetcher: &ArcChangesetFetcher,
    params: Params,
    lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
    low_gen_num_checker: &LowGenNumChecker,
) -> Result<Option<Vec<ChangesetId>>> {
    let threshold = match low_gen_num_checker.get_threshold() {
        Some(threshold) => threshold,
        None => {
            return Ok(None);
        }
    };
    let split_params = match split_heads_excludes(ctx, params, threshold) {
        Some(split_params) => split_params,
        None => return Ok(None),
    };

    let SplitParams {
        low_gens_params,
        high_gens_params,
    } = split_params;

    let limit = tunables().get_getbundle_low_gen_optimization_max_traversal_limit();
    let limit = if limit <= 0 {
        DEFAULT_TRAVERSAL_LIMIT
    } else {
        limit as u64
    };

    let maybe_nodes_to_send =
        process_low_gen_params(ctx, changeset_fetcher, low_gens_params, lca_hint, limit).await?;
    let nodes_to_send = match maybe_nodes_to_send {
        Some(nodes_to_send) => nodes_to_send,
        None => {
            return Ok(None);
        }
    };

    let nodes_to_send_second_part = call_difference_of_union_of_ancestors_revset(
        ctx,
        changeset_fetcher,
        high_gens_params,
        lca_hint,
        None,
    )
    .await?
    .ok_or_else(|| anyhow!(crate::UNEXPECTED_NONE_ERR_MSG))?;

    let mut nodes_to_send = nodes_to_send.into_iter().collect::<HashSet<_>>();
    nodes_to_send.extend(nodes_to_send_second_part);

    let mut nodes_to_send: Vec<_> = stream::iter(nodes_to_send)
        .map({
            move |bcs_id| async move {
                let gen_num = changeset_fetcher
                    .get_generation_number(ctx.clone(), bcs_id)
                    .await?;
                Result::<_, Error>::Ok((bcs_id, gen_num))
            }
        })
        .buffered(100)
        .try_collect()
        .await?;

    nodes_to_send.sort_by_key(|(_bcs_id, gen_num)| gen_num.value());
    Ok(Some(
        nodes_to_send
            .into_iter()
            .rev()
            .map(|(bcs_id, _gen_num)| bcs_id)
            .collect(),
    ))
}

async fn process_low_gen_params(
    ctx: &CoreContext,
    changeset_fetcher: &ArcChangesetFetcher,
    low_gens_params: Vec<Params>,
    lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
    limit: u64,
) -> Result<Option<Vec<ChangesetId>>, Error> {
    let mut s = stream::iter(low_gens_params)
        .map(Ok)
        .map_ok(|low_gens_params| {
            call_difference_of_union_of_ancestors_revset(
                ctx,
                changeset_fetcher,
                low_gens_params,
                lca_hint,
                Some(limit),
            )
        })
        .try_buffer_unordered(10);

    let mut nodes_to_send = vec![];
    while let Some(maybe_chunk) = s.try_next().await? {
        match maybe_chunk {
            Some(chunk) => {
                nodes_to_send.extend(chunk);
            }
            None => {
                ctx.scuba().clone().log_with_msg(
                    "Low generation getbundle optimization traversed too many nodes, disabling",
                    Some(format!("{}", limit)),
                );

                return Ok(None);
            }
        }
    }

    Ok(Some(nodes_to_send))
}

struct SplitParams {
    low_gens_params: Vec<Params>,
    high_gens_params: Params,
}

/// Split heads and excludes parameters into two Params so that calling
/// call_difference_of_union_of_ancestors_revset() for both of these parameters and combining the output
/// would yield semantically the same result from mercurial client perspective.
/// In particular:
/// 1) Heads are split in two two parts - the ones that have a generation value higher
///    than threshold (high_gens_params), and everything else (low_gens_params).
/// 2) high_gens_params gets all the excludes
/// 3) low_gens_params gets all the excludes that are lower than maximum from low gen nums.
///
/// In example below:
///
/// ```text
/// O <- head 1
/// |
/// O <- exclude 1
/// |
/// ...   O <- head 2 <- low generation number
/// |     |
/// O     O <- exclude 2
///
/// we'd get {head1}, {exclude1, exclude2} and {head2}, {exclude2}
/// ```
fn split_heads_excludes(ctx: &CoreContext, params: Params, threshold: u64) -> Option<SplitParams> {
    let Params { heads, excludes } = params;
    let (high_gen_heads, low_gen_heads): (Vec<_>, Vec<_>) = heads
        .into_iter()
        .partition(|head| head.1.value() > threshold);

    let high_gens_params = Params {
        heads: high_gen_heads,
        excludes: excludes.clone(),
    };

    let mut low_gens_params = vec![];
    // We shouldn't normally have too many heads with low generation number
    // If we do have a lot of them, then something weird is going on, and it's
    // better to exit quickly.
    let low_gen_heads_num: u64 = low_gen_heads.len().try_into().unwrap();
    if low_gen_heads_num > LOW_GEN_HEADS_LIMIT {
        ctx.scuba()
            .clone()
            .log_with_msg("Too many heads with low generating number", None);
        return None;
    }

    for head in low_gen_heads {
        let low_gen_excludes = excludes
            .clone()
            .into_iter()
            .filter(|entry| entry.1 <= head.1)
            .collect();
        low_gens_params.push(Params {
            heads: vec![head],
            excludes: low_gen_excludes,
        });
    }

    Some(SplitParams {
        low_gens_params,
        high_gens_params,
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use blobrepo::BlobRepo;
    use fbinit::FacebookInit;
    use futures::compat::Stream01CompatExt;
    use futures::FutureExt;
    use futures_01_ext::StreamExt as OldStreamExt;
    use futures_old::stream as old_stream;
    use maplit::btreemap;
    use maplit::hashmap;
    use mononoke_types_mocks::changesetid::FOURS_CSID;
    use mononoke_types_mocks::changesetid::ONES_CSID;
    use mononoke_types_mocks::changesetid::THREES_CSID;
    use mononoke_types_mocks::changesetid::TWOS_CSID;
    use revset::add_generations_by_bonsai;
    use skiplist::SkiplistIndex;
    use std::collections::BTreeMap;
    use tests_utils::drawdag::create_from_dag;
    use tests_utils::CreateCommitContext;
    use tunables::with_tunables_async;
    use tunables::MononokeTunables;

    #[fbinit::test]
    fn test_split_heads_excludes(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let gen_0 = Generation::new(0);
        let gen_1 = Generation::new(1);
        let gen_5 = Generation::new(5);
        let gen_7 = Generation::new(7);

        let heads = vec![(ONES_CSID, gen_0), (FOURS_CSID, gen_7)];
        let excludes = vec![(TWOS_CSID, gen_1), (THREES_CSID, gen_5)];

        let params = Params { heads, excludes };
        let SplitParams {
            low_gens_params,
            high_gens_params,
        } = split_heads_excludes(&ctx, params.clone(), 2).unwrap();

        assert_eq!(low_gens_params.len(), 1);
        assert_eq!(low_gens_params[0].heads, vec![(ONES_CSID, gen_0)]);
        assert_eq!(low_gens_params[0].excludes, vec![]);

        assert_eq!(high_gens_params.heads, vec![(FOURS_CSID, gen_7)]);
        assert_eq!(
            high_gens_params.excludes,
            vec![(TWOS_CSID, gen_1), (THREES_CSID, gen_5)]
        );

        let SplitParams {
            mut low_gens_params,
            high_gens_params,
        } = split_heads_excludes(&ctx, params.clone(), 7).unwrap();
        assert_eq!(low_gens_params.len(), 2);
        low_gens_params.sort_by_key(|params| params.heads[0].1);
        assert_eq!(low_gens_params[0].heads, vec![(ONES_CSID, gen_0)]);
        assert_eq!(low_gens_params[1].heads, vec![(FOURS_CSID, gen_7)]);
        assert_eq!(low_gens_params[0].excludes, vec![]);
        assert_eq!(
            low_gens_params[1].excludes,
            vec![(TWOS_CSID, gen_1), (THREES_CSID, gen_5)]
        );

        assert_eq!(high_gens_params.heads, vec![]);
        assert_eq!(high_gens_params.excludes, params.excludes);

        Ok(())
    }

    #[fbinit::test]
    async fn test_low_gen_num_optimization(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let (repo, commit_map) = create_repo(&ctx).await?;

        let skiplist: Arc<dyn LeastCommonAncestorsHint> = Arc::new(SkiplistIndex::new());

        let params = generate_params(
            &ctx,
            &repo,
            &commit_map,
            &["J".to_string(), "M".to_string()],
            &["L".to_string(), "I".to_string()],
        )
        .await?;

        let low_gen_num_checker = LowGenNumChecker::new(None);
        // Tunable is disabled, so optimization does not kick in
        let maybe_res = low_gen_num_optimization(
            &ctx,
            &repo.get_changeset_fetcher(),
            params.clone(),
            &skiplist,
            &low_gen_num_checker,
        )
        .await?;
        assert!(maybe_res.is_none());

        let low_gen_num_checker = LowGenNumChecker::new(Some(4));
        // Now it's enabled, make sure we got the response
        let tunables = MononokeTunables::default();
        tunables.update_bools(&hashmap! {"getbundle_use_low_gen_optimization".to_string() => true});
        tunables.update_ints(&hashmap! {
            "getbundle_low_gen_optimization_max_traversal_limit".to_string() => 3,
        });

        let expected_result = vec![
            commit_map.get("J").cloned().unwrap(),
            commit_map.get("M").cloned().unwrap(),
        ];
        with_tunables_async(
            tunables,
            async {
                let maybe_res = low_gen_num_optimization(
                    &ctx,
                    &repo.get_changeset_fetcher(),
                    params.clone(),
                    &skiplist,
                    &low_gen_num_checker,
                )
                .await?;
                assert_eq!(maybe_res, Some(expected_result));
                Result::<_, Error>::Ok(())
            }
            .boxed(),
        )
        .await?;

        // Now let's check that if low gen optimization overfetches a lot of commits then it
        // returns None
        let params = generate_params(
            &ctx,
            &repo,
            &commit_map,
            &["J".to_string(), "M".to_string()],
            &["K".to_string(), "I".to_string()],
        )
        .await?;

        let low_gen_num_checker = LowGenNumChecker::new(Some(4));
        let tunables = MononokeTunables::default();
        tunables.update_bools(&hashmap! {"getbundle_use_low_gen_optimization".to_string() => true});
        tunables.update_ints(&hashmap! {
            "getbundle_low_gen_optimization_max_traversal_limit".to_string() => 1,
        });

        with_tunables_async(
            tunables,
            async {
                let maybe_res = low_gen_num_optimization(
                    &ctx,
                    &repo.get_changeset_fetcher(),
                    params,
                    &skiplist,
                    &low_gen_num_checker,
                )
                .await?;
                assert_eq!(maybe_res, None);
                Result::<_, Error>::Ok(())
            }
            .boxed(),
        )
        .await?;

        Ok(())
    }

    #[fbinit::test]
    async fn test_low_gen_num_optimization_no_duplicates(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let (repo, commit_map) = create_mergy_repo(&ctx).await?;

        let skiplist: Arc<dyn LeastCommonAncestorsHint> = Arc::new(SkiplistIndex::new());

        let params = generate_params(
            &ctx,
            &repo,
            &commit_map,
            &["J".to_string(), "K".to_string()],
            &[],
        )
        .await?;

        let low_gen_num_checker = LowGenNumChecker::new(Some(5));
        // Let's it's enabled, make sure we got the response
        let tunables = MononokeTunables::default();
        tunables.update_bools(&hashmap! {"getbundle_use_low_gen_optimization".to_string() => true});
        tunables.update_ints(&hashmap! {
            "getbundle_low_gen_optimization_max_traversal_limit".to_string() => 10,
        });

        with_tunables_async(
            tunables,
            async {
                let maybe_res = low_gen_num_optimization(
                    &ctx,
                    &repo.get_changeset_fetcher(),
                    params.clone(),
                    &skiplist,
                    &low_gen_num_checker,
                )
                .await?;
                assert_eq!(maybe_res.map(|v| v.len()), Some(11));
                Result::<_, Error>::Ok(())
            }
            .boxed(),
        )
        .await?;

        Ok(())
    }

    #[fbinit::test]
    async fn test_compute_partial_getbundle(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;

        let commit_map = create_from_dag(
            &ctx,
            &repo,
            r##"
                A-B-C-D-E-F-G-H-I-J
                         /
                    K-L-M
            "##,
        )
        .await?;

        let low_gen_num_checker = LowGenNumChecker::new(None);
        // Partial getbundle optimization is disabled, so it should do nothing
        let (res, params) = test_compute_partial_bundle(
            &ctx,
            &repo,
            &commit_map,
            hashmap! {},
            &["J".to_string()],
            &["I".to_string()],
            &low_gen_num_checker,
        )
        .await?;

        assert!(res.partial.is_empty());
        assert_eq!(res.new_heads, params.heads);
        assert_eq!(res.new_excludes, params.excludes);

        let low_gen_num_checker = LowGenNumChecker::new(Some(4));
        // Now let's enable the optimization, but set very low traversal limit
        let (res, params) = test_compute_partial_bundle(
            &ctx,
            &repo,
            &commit_map,
            hashmap! {
                "getbundle_partial_getbundle_traversal_limit".to_string() => 1,
            },
            &["J".to_string()],
            &["G".to_string()],
            &low_gen_num_checker,
        )
        .await?;
        assert_eq!(res.partial, vec![commit_map.get("J").cloned().unwrap()]);
        assert_eq!(res.new_heads.len(), 1);
        assert_eq!(res.new_heads[0].0, commit_map["I"]);
        assert_eq!(res.new_excludes, params.excludes);

        let low_gen_num_checker = LowGenNumChecker::new(Some(4));
        // Simplest case - it should traverse a single changeset id and return it
        let (res, params) = test_compute_partial_bundle(
            &ctx,
            &repo,
            &commit_map,
            hashmap! {
                "getbundle_partial_getbundle_traversal_limit".to_string() => 10,
            },
            &["J".to_string()],
            &["I".to_string()],
            &low_gen_num_checker,
        )
        .await?;
        assert_eq!(res.partial, vec![commit_map.get("J").cloned().unwrap()]);
        assert!(res.new_heads.is_empty());
        assert_eq!(res.new_excludes, params.excludes);

        let low_gen_num_checker = LowGenNumChecker::new(Some(0));
        // Let it traverse the whole repo
        let (res, params) = test_compute_partial_bundle(
            &ctx,
            &repo,
            &commit_map,
            hashmap! {
                "getbundle_partial_getbundle_traversal_limit".to_string() => 20,
            },
            &["J".to_string(), "I".to_string()],
            &[],
            &low_gen_num_checker,
        )
        .await?;
        assert_eq!(res.partial.len(), 13);
        assert!(res.new_heads.is_empty());
        assert_eq!(res.new_excludes, params.excludes);

        let low_gen_num_checker = LowGenNumChecker::new(Some(4));
        // Now let's enable the optimization and make it traverse up until a merge commit
        let (res, params) = test_compute_partial_bundle(
            &ctx,
            &repo,
            &commit_map,
            hashmap! {
                "getbundle_partial_getbundle_traversal_limit".to_string() => 10,
            },
            &["J".to_string()],
            &["E".to_string()],
            &low_gen_num_checker,
        )
        .await?;
        assert_eq!(
            res.partial,
            vec![
                commit_map.get("J").cloned().unwrap(),
                commit_map.get("I").cloned().unwrap(),
                commit_map.get("H").cloned().unwrap(),
                commit_map.get("G").cloned().unwrap(),
                commit_map.get("F").cloned().unwrap(),
            ]
        );
        assert_eq!(res.new_heads.len(), 1);
        assert_eq!(
            res.new_heads.get(0).map(|x| x.0),
            commit_map.get("M").cloned()
        );
        assert_eq!(res.new_excludes, params.excludes);

        let low_gen_num_checker = LowGenNumChecker::new(Some(4));
        // Now let's add a few more heads that are ancestors of each other.
        // It shouldn't change the result
        let (res, params) = test_compute_partial_bundle(
            &ctx,
            &repo,
            &commit_map,
            hashmap! {
                "getbundle_partial_getbundle_traversal_limit".to_string() => 10,
            },
            &["J".to_string(), "I".to_string(), "H".to_string()],
            &["E".to_string()],
            &low_gen_num_checker,
        )
        .await?;
        assert_eq!(
            res.partial,
            vec![
                commit_map.get("J").cloned().unwrap(),
                commit_map.get("I").cloned().unwrap(),
                commit_map.get("H").cloned().unwrap(),
                commit_map.get("G").cloned().unwrap(),
                commit_map.get("F").cloned().unwrap(),
            ]
        );
        assert_eq!(res.new_heads.len(), 1);
        assert_eq!(
            res.new_heads.get(0).map(|x| x.0),
            commit_map.get("M").cloned()
        );
        assert_eq!(res.new_excludes, params.excludes);

        let low_gen_num_checker = LowGenNumChecker::new(Some(6));
        // Set higher gen num limit
        let (res, params) = test_compute_partial_bundle(
            &ctx,
            &repo,
            &commit_map,
            hashmap! {
                "getbundle_partial_getbundle_traversal_limit".to_string() => 10,
            },
            &["J".to_string()],
            &["E".to_string()],
            &low_gen_num_checker,
        )
        .await?;
        assert_eq!(
            res.partial,
            vec![
                commit_map.get("J").cloned().unwrap(),
                commit_map.get("I").cloned().unwrap(),
                commit_map.get("H").cloned().unwrap(),
                commit_map.get("G").cloned().unwrap(),
            ]
        );
        assert_eq!(res.new_heads.len(), 1);
        assert_eq!(
            res.new_heads.get(0).map(|x| x.0),
            commit_map.get("F").cloned()
        );
        assert_eq!(res.new_excludes, params.excludes);

        let low_gen_num_checker = LowGenNumChecker::new(Some(60));
        // Set very high gen num limit
        let (res, params) = test_compute_partial_bundle(
            &ctx,
            &repo,
            &commit_map,
            hashmap! {
                "getbundle_partial_getbundle_traversal_limit".to_string() => 10,
            },
            &["J".to_string()],
            &["E".to_string()],
            &low_gen_num_checker,
        )
        .await?;
        assert!(res.partial.is_empty());
        assert_eq!(res.new_heads, params.heads);
        assert_eq!(res.new_excludes, params.excludes);

        Ok(())
    }

    #[fbinit::test]
    async fn test_partial_getbundle_merge_parents_ancestors_of_each_other(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;

        let first = CreateCommitContext::new_root(&ctx, &repo).commit().await?;

        let second = CreateCommitContext::new(&ctx, &repo, vec![first])
            .commit()
            .await?;

        // Merge parents are ancestors of each other. This test makes sure
        // we return parents in correct order i.e. `second` before `first`.
        let merge_first_second = CreateCommitContext::new(&ctx, &repo, vec![first, second])
            .commit()
            .await?;

        let merge_second_first = CreateCommitContext::new(&ctx, &repo, vec![second, first])
            .commit()
            .await?;

        let commit_map = btreemap! {
            "A".to_string() => first,
            "B".to_string() => second,
            "C".to_string() => merge_first_second,
            "D".to_string() => merge_second_first,
        };

        let low_gen_num_checker = LowGenNumChecker::new(Some(0));
        let (res, _params) = test_compute_partial_bundle(
            &ctx,
            &repo,
            &commit_map,
            hashmap! {
                "getbundle_partial_getbundle_traversal_limit".to_string() => 10,
            },
            &["C".to_string()],
            &[],
            &low_gen_num_checker,
        )
        .await?;

        assert_eq!(res.partial, vec![merge_first_second, second, first]);

        let (res, _params) = test_compute_partial_bundle(
            &ctx,
            &repo,
            &commit_map,
            hashmap! {
                "getbundle_partial_getbundle_traversal_limit".to_string() => 10,
            },
            &["D".to_string()],
            &[],
            &low_gen_num_checker,
        )
        .await?;

        assert_eq!(res.partial, vec![merge_second_first, second, first]);
        Ok(())
    }

    #[fbinit::test]
    async fn test_low_gen_num_two_heads(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;

        let commit_map = create_from_dag(
            &ctx,
            &repo,
            r##"
                  H
                 /
                A-B-C-D-E-F-G
                   \
                     I
            "##,
        )
        .await?;
        let skiplist: Arc<dyn LeastCommonAncestorsHint> = Arc::new(SkiplistIndex::new());

        let params = generate_params(
            &ctx,
            &repo,
            &commit_map,
            &["H".to_string(), "I".to_string()],
            &["B".to_string()],
        )
        .await?;

        let tunables = MononokeTunables::default();
        tunables.update_bools(&hashmap! {"getbundle_use_low_gen_optimization".to_string() => true});
        tunables.update_ints(&hashmap! {
            "getbundle_low_gen_optimization_max_traversal_limit".to_string() => 10,
        });
        let low_gen_num_checker = LowGenNumChecker::new(Some(5));

        with_tunables_async(
            tunables,
            async {
                let maybe_res = low_gen_num_optimization(
                    &ctx,
                    &repo.get_changeset_fetcher(),
                    params.clone(),
                    &skiplist,
                    &low_gen_num_checker,
                )
                .await?;
                assert_eq!(maybe_res.map(|v| v.len()), Some(2));
                Result::<_, Error>::Ok(())
            }
            .boxed(),
        )
        .await?;

        Ok(())
    }

    async fn test_compute_partial_bundle(
        ctx: &CoreContext,
        repo: &BlobRepo,
        commit_map: &BTreeMap<String, ChangesetId>,
        values: HashMap<String, i64>,
        heads: &[String],
        excludes: &[String],
        low_gen_num_checker: &LowGenNumChecker,
    ) -> Result<(PartialGetBundle, Params), Error> {
        let params = generate_params(ctx, repo, commit_map, heads, excludes).await?;

        let tunables = MononokeTunables::default();
        tunables.update_ints(&values);
        let bundle = with_tunables_async(
            tunables,
            async {
                let res = compute_partial_getbundle(
                    ctx,
                    &repo.get_changeset_fetcher(),
                    params.heads.clone(),
                    params.excludes.clone(),
                    low_gen_num_checker,
                )
                .await?;
                Result::<_, Error>::Ok(res)
            }
            .boxed(),
        )
        .await?;
        Ok((bundle, params))
    }

    async fn generate_params(
        ctx: &CoreContext,
        repo: &BlobRepo,
        commit_map: &BTreeMap<String, ChangesetId>,
        heads: &[String],
        excludes: &[String],
    ) -> Result<Params, Error> {
        let heads: Vec<_> = heads
            .iter()
            .map(|x| commit_map.get(x).cloned().unwrap())
            .collect();

        let heads = add_generations_by_bonsai(
            ctx.clone(),
            old_stream::iter_ok(heads.into_iter()).boxify(),
            repo.get_changeset_fetcher(),
        )
        .compat()
        .try_collect::<Vec<_>>()
        .await?;

        let excludes: Vec<_> = excludes
            .iter()
            .map(|x| commit_map.get(x).cloned().unwrap())
            .collect();
        let excludes = add_generations_by_bonsai(
            ctx.clone(),
            old_stream::iter_ok(excludes.into_iter()).boxify(),
            repo.get_changeset_fetcher(),
        )
        .compat()
        .try_collect::<Vec<_>>()
        .await?;

        let mut params = Params::default();
        params.heads = heads;
        params.excludes = excludes;

        Ok(params)
    }

    async fn create_repo(
        ctx: &CoreContext,
    ) -> Result<(BlobRepo, BTreeMap<String, ChangesetId>), Error> {
        let repo: BlobRepo = test_repo_factory::build_empty(ctx.fb)?;

        let commit_map = create_from_dag(
            ctx,
            &repo,
            r##"
                A-B-C-D-E-F-G-H-I-J
                K-L-M
            "##,
        )
        .await?;

        Ok((repo, commit_map))
    }

    async fn create_mergy_repo(
        ctx: &CoreContext,
    ) -> Result<(BlobRepo, BTreeMap<String, ChangesetId>), Error> {
        let repo: BlobRepo = test_repo_factory::build_empty(ctx.fb)?;

        let commit_map = create_from_dag(
            ctx,
            &repo,
            r##"
                A-B-C-D-E-F-G-H-I-J
                     \
                      K
            "##,
        )
        .await?;

        Ok((repo, commit_map))
    }
}
