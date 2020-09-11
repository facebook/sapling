/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{call_difference_of_union_of_ancestors_revset, Params};
use anyhow::{anyhow, Error, Result};
use blobrepo::ChangesetFetcher;
use context::CoreContext;
use futures::{
    compat::Future01CompatExt,
    stream::{self, StreamExt, TryStreamExt},
};
use mononoke_types::{ChangesetId, Generation};
use reachabilityindex::LeastCommonAncestorsHint;
use scuba_ext::ScubaSampleBuilderExt;
use std::{collections::HashSet, sync::Arc};
use tunables::tunables;

pub const DEFAULT_TRAVERSAL_LIMIT: u64 = 20;

pub(crate) fn has_low_gen_num(heads: &[(ChangesetId, Generation)]) -> bool {
    let low_gen_num_threshold = tunables().get_getbundle_low_gen_num_threshold() as u64;
    for (_, gen) in heads {
        if gen.value() <= low_gen_num_threshold {
            return true;
        }
    }

    false
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
) -> Result<Option<Vec<ChangesetId>>> {
    let threshold = tunables().get_getbundle_low_gen_num_threshold() as u64;
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
                    .compat()
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

        // Tunable is disabled, so optimization does not kick in
        let maybe_res = low_gen_num_optimization(
            &ctx,
            &repo.get_changeset_fetcher(),
            params.clone(),
            &skiplist,
        )
        .await?;
        assert!(maybe_res.is_none());


        // Now it's enabled, make sure we got the response
        let tunables = MononokeTunables::default();
        tunables.update_bools(&hashmap! {"getbundle_use_low_gen_optimization".to_string() => true});
        tunables.update_ints(&hashmap! {
            "getbundle_low_gen_num_threshold".to_string() => 4,
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

        let tunables = MononokeTunables::default();
        tunables.update_bools(&hashmap! {"getbundle_use_low_gen_optimization".to_string() => true});
        tunables.update_ints(&hashmap! {
            "getbundle_low_gen_num_threshold".to_string() => 4,
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

        // Let's it's enabled, make sure we got the response
        let tunables = MononokeTunables::default();
        tunables.update_bools(&hashmap! {"getbundle_use_low_gen_optimization".to_string() => true});
        tunables.update_ints(&hashmap! {
            "getbundle_low_gen_num_threshold".to_string() => 5,
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
