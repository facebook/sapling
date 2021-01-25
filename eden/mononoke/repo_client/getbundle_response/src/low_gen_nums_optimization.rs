/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{call_difference_of_union_of_ancestors_revset, Params};
use anyhow::{anyhow, Error, Result};
use blobrepo::ChangesetFetcher;
use context::{CoreContext, PerfCounterType};
use futures::{
    future::{try_join_all, TryFutureExt},
    stream::{self, StreamExt, TryStreamExt},
};
use mononoke_types::{ChangesetId, Generation};
use reachabilityindex::LeastCommonAncestorsHint;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    iter::FromIterator,
    sync::Arc,
};
use tunables::tunables;

pub const DEFAULT_TRAVERSAL_LIMIT: u64 = 20;

pub(crate) struct LowGenNumChecker {
    low_gen_num_threshold: Option<u64>,
}

impl LowGenNumChecker {
    pub(crate) fn new_from_tunables(highest_gen_num: u64) -> Self {
        let difference = tunables().get_getbundle_high_low_gen_num_difference_threshold();
        if difference > 0 {
            let difference = difference as u64;
            let low_gen_num_threshold = Some(highest_gen_num.saturating_sub(difference));
            return Self {
                low_gen_num_threshold,
            };
        }

        let threshold = tunables().get_getbundle_low_gen_num_threshold();
        let low_gen_num_threshold = if threshold > 0 {
            Some(threshold as u64)
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
pub(crate) async fn compute_partial_getbundle(
    ctx: &CoreContext,
    changeset_fetcher: &Arc<dyn ChangesetFetcher>,
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

    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();
    let mut new_heads: HashMap<_, _> = HashMap::from_iter(heads.clone());
    let new_excludes: HashMap<_, _> = HashMap::from_iter(excludes);

    if let Some(max_head) = maybe_max_head {
        if !new_excludes.contains_key(&max_head.0) && max_head.1.value() > gen_num_threshold {
            queue.push_back(max_head.0);
            visited.insert(max_head.0);
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
    while let Some(cs_id) = queue.pop_front() {
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
            // This parent was already visited or excluded - just ignore it
            if !visited.insert(p) || new_excludes.contains_key(&p) {
                continue;
            }

            new_heads.insert(p, gen_num);
            if gen_num.value() > gen_num_threshold {
                // We don't visit a parent that has a very low generation number -
                // it will be processed separately
                queue.push_back(p);
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
///
pub(crate) async fn low_gen_num_optimization(
    ctx: &CoreContext,
    changeset_fetcher: &Arc<dyn ChangesetFetcher>,
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
    let split_params = match split_heads_excludes(params, threshold) {
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

    let maybe_nodes_to_send = call_difference_of_union_of_ancestors_revset(
        &ctx,
        &changeset_fetcher,
        low_gens_params,
        &lca_hint,
        Some(limit),
    )
    .await?;

    let nodes_to_send = match maybe_nodes_to_send {
        Some(nodes_to_send) => nodes_to_send,
        None => {
            ctx.scuba().clone().log_with_msg(
                "Low generation getbundle optimization traversed too many nodes, disabling",
                Some(format!("{}", limit)),
            );

            return Ok(None);
        }
    };

    let nodes_to_send_second_part = call_difference_of_union_of_ancestors_revset(
        &ctx,
        &changeset_fetcher,
        high_gens_params,
        &lca_hint,
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

struct SplitParams {
    low_gens_params: Params,
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
/// O <- head 1
/// |
/// O <- exclude 1
/// |
/// ...   O <- head 2 <- low generation number
/// |     |
/// O     O <- exclude 2
///
/// we'd get {head1}, {exclude1, exclude2} and {head2}, {exclude2}
fn split_heads_excludes(params: Params, threshold: u64) -> Option<SplitParams> {
    let Params { heads, excludes } = params;
    let (high_gen_heads, low_gen_heads): (Vec<_>, Vec<_>) = heads
        .into_iter()
        .partition(|head| head.1.value() > threshold);

    let max_low_gen_num = match low_gen_heads.iter().max_by_key(|entry| entry.1) {
        Some(max_low_gen_num) => max_low_gen_num.1,
        None => {
            return None;
        }
    };

    let low_gen_excludes = excludes
        .clone()
        .into_iter()
        .filter(|entry| entry.1 <= max_low_gen_num)
        .collect();

    let high_gens_params = Params {
        heads: high_gen_heads,
        excludes,
    };
    let low_gens_params = Params {
        heads: low_gen_heads,
        excludes: low_gen_excludes,
    };

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
    use futures::{compat::Stream01CompatExt, FutureExt};
    use futures_ext::StreamExt as OldStreamExt;
    use futures_old::stream as old_stream;
    use maplit::hashmap;
    use mononoke_types_mocks::changesetid::{FOURS_CSID, ONES_CSID, THREES_CSID, TWOS_CSID};
    use revset::add_generations_by_bonsai;
    use skiplist::SkiplistIndex;
    use std::collections::BTreeMap;
    use tests_utils::drawdag::create_from_dag;
    use tunables::{with_tunables_async, MononokeTunables};

    #[test]
    fn test_split_heads_excludes() -> Result<(), Error> {
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
        } = split_heads_excludes(params.clone(), 2).unwrap();

        assert_eq!(low_gens_params.heads, vec![(ONES_CSID, gen_0)]);
        assert_eq!(low_gens_params.excludes, vec![]);

        assert_eq!(high_gens_params.heads, vec![(FOURS_CSID, gen_7)]);
        assert_eq!(
            high_gens_params.excludes,
            vec![(TWOS_CSID, gen_1), (THREES_CSID, gen_5)]
        );

        let SplitParams {
            low_gens_params,
            high_gens_params,
        } = split_heads_excludes(params.clone(), 7).unwrap();
        assert_eq!(low_gens_params.heads, params.heads);
        assert_eq!(low_gens_params.excludes, params.excludes);
        assert_eq!(high_gens_params.heads, vec![]);
        assert_eq!(high_gens_params.excludes, params.excludes);

        Ok(())
    }

    #[fbinit::compat_test]
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

    #[fbinit::compat_test]
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

    #[fbinit::compat_test]
    async fn test_compute_partial_getbundle(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = blobrepo_factory::new_memblob_empty(None)?;

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

    async fn test_compute_partial_bundle(
        ctx: &CoreContext,
        repo: &BlobRepo,
        commit_map: &BTreeMap<String, ChangesetId>,
        values: HashMap<String, i64>,
        heads: &[String],
        excludes: &[String],
        low_gen_num_checker: &LowGenNumChecker,
    ) -> Result<(PartialGetBundle, Params), Error> {
        let params = generate_params(&ctx, &repo, &commit_map, &heads, &excludes).await?;

        let tunables = MononokeTunables::default();
        tunables.update_ints(&values);
        let bundle = with_tunables_async(
            tunables,
            async {
                let res = compute_partial_getbundle(
                    &ctx,
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
        let repo = blobrepo_factory::new_memblob_empty(None)?;

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
        let repo = blobrepo_factory::new_memblob_empty(None)?;

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
