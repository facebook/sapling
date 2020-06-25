/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{BonsaiDerived, BonsaiDerivedMapping, DeriveError, Mode};
use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use cacheblob::LeaseOps;
use context::CoreContext;
use futures::{
    compat::Future01CompatExt,
    future::{try_join, try_join_all, FutureExt as NewFutureExt, TryFutureExt},
    TryStreamExt,
};
use futures_ext::FutureExt;
use futures_old::sync::oneshot;
use futures_old::Future;
use futures_stats::{futures03::TimedFutureExt, FutureStats};
use metaconfig_types::DerivedDataConfig;
use mononoke_types::ChangesetId;
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use slog::debug;
use slog::warn;
use stats::prelude::*;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use time_ext::DurationExt;
use topo_sort::sort_topological;
use tracing::{trace_args, EventId, Traced};

define_stats! {
    prefix = "mononoke.derived_data";
    derived_data_latency:
        dynamic_timeseries("{}.deriving.latency_ms", (derived_data_type: &'static str); Average),
    derived_data_disabled:
        dynamic_timeseries("{}.{}.derived_data_disabled", (repo_id: i32, derived_data_type: &'static str); Count),
}

const DERIVE_TRACE_THRESHOLD: Duration = Duration::from_secs(3);
const LEASE_WARNING_THRESHOLD: Duration = Duration::from_secs(60);

/// Actual implementation of `BonsaiDerived::derive`, which recursively generates derivations.
/// If the data was already generated (i.e. the data is already in `derived_mapping`) then
/// nothing will be generated. Otherwise this function will try to find set of commits that's
/// bounded by commits which have derived_mapping entry i.e. in the case below
///
/// A <- no mapping
/// |
/// B <- no mapping
/// |
/// C <- mapping exists
/// ...
///
/// the data will be first generated for commit B, then for commit A.
///
/// NOTE: One important caveat about derived_mapping - it's NOT guaranteed that ancestor
/// changeset has a mapping entry if descendant changeset has a mapping entry.
/// For example, case like
///
/// A <- no mapping
/// |
/// B <- mapping exists
/// |
/// C <- no mapping
///
/// is possible and valid (but only if the data for commit C is derived).
pub fn derive_impl<
    Derived: BonsaiDerived,
    Mapping: BonsaiDerivedMapping<Value = Derived> + Send + Sync + Clone + 'static,
>(
    ctx: CoreContext,
    repo: BlobRepo,
    derived_mapping: Mapping,
    start_csid: ChangesetId,
    mode: Mode,
) -> impl Future<Item = Derived, Error = DeriveError> {
    async move {
        let derivation = async {
            let all_csids =
                find_topo_sorted_underived(&ctx, &repo, &derived_mapping, &start_csid, None, mode)
                    .await?;

            for csid in &all_csids {
                ctx.scuba().clone().log_with_msg(
                    "Waiting for derived data to be generated",
                    Some(format!("{} {}", Derived::NAME, csid)),
                );

                let (stats, res) = derive_may_panic(&ctx, &repo, &derived_mapping, &csid)
                    .timed()
                    .await;

                let tag = if res.is_ok() {
                    "Got derived data"
                } else {
                    "Failed to get derived data"
                };
                ctx.scuba()
                    .clone()
                    .add_future_stats(&stats)
                    .log_with_msg(tag, Some(format!("{} {}", Derived::NAME, csid)));
                res?;
            }

            let res: Result<_, DeriveError> = Ok(all_csids.len());
            res
        };

        let (stats, res) = derivation.timed().await;

        let count = match res {
            Ok(ref count) => *count,
            Err(_) => 0,
        };
        if stats.completion_time > DERIVE_TRACE_THRESHOLD {
            warn!(
                ctx.logger(),
                "slow derivation of {} {} for {}, took {:?}: mononoke_prod/flat/{}.trace",
                count,
                Derived::NAME,
                start_csid,
                stats.completion_time,
                ctx.trace().id(),
            );
            ctx.scuba().clone().add_future_stats(&stats).log_with_msg(
                "Slow derivation",
                Some(format!(
                    "type={},count={},csid={}",
                    Derived::NAME,
                    count,
                    start_csid.to_string()
                )),
            );
        }

        res?;

        let derived = fetch_derived_may_panic(&ctx, start_csid, &derived_mapping).await?;
        Ok(derived)
    }
    .boxed()
    .compat()
}

fn fail_if_disabled<Derived: BonsaiDerived>(repo: &BlobRepo) -> Result<(), DeriveError> {
    if !repo
        .get_derived_data_config()
        .derived_data_types
        .contains(Derived::NAME)
    {
        STATS::derived_data_disabled.add_value(1, (repo.get_repoid().id(), Derived::NAME));
        return Err(DeriveError::Disabled(Derived::NAME, repo.get_repoid()));
    }
    Ok(())
}

pub(crate) async fn find_topo_sorted_underived<
    Derived: BonsaiDerived,
    Mapping: BonsaiDerivedMapping<Value = Derived> + Send + Sync + Clone + 'static,
>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    derived_mapping: &Mapping,
    start_csid: &ChangesetId,
    limit: Option<u64>,
    mode: Mode,
) -> Result<Vec<ChangesetId>, Error> {
    if mode == Mode::OnlyIfEnabled {
        fail_if_disabled::<Derived>(repo)?;
    }

    let changeset_fetcher = repo.get_changeset_fetcher();
    // This is necessary to avoid visiting the same commit a lot of times in mergy repos
    let visited: Arc<Mutex<HashSet<ChangesetId>>> = Arc::new(Mutex::new(HashSet::new()));

    let changeset_fetcher = &changeset_fetcher;
    let visited = &visited;
    let commits_not_derived_to_parents =
        bounded_traversal::bounded_traversal_stream(100, Some(*start_csid), {
            move |cs_id| {
                async move {
                    if let Some(limit) = limit {
                        let visited = visited.lock().unwrap();
                        if visited.len() as u64 > limit {
                            return Result::<_, Error>::Ok((None, vec![]));
                        }
                    }

                    let derive_node =
                        DeriveNode::from_bonsai(&ctx, derived_mapping, &cs_id).await?;

                    match derive_node {
                        DeriveNode::Derived(_) => Ok((None, vec![])),
                        DeriveNode::Bonsai(bcs_id) => {
                            let parents = changeset_fetcher
                                .get_parents(ctx.clone(), bcs_id)
                                .compat()
                                .await?;

                            let parents_to_visit: Vec<_> = {
                                let mut visited = visited.lock().unwrap();
                                parents
                                    .iter()
                                    .cloned()
                                    .filter(|p| visited.insert(*p))
                                    .collect()
                            };
                            // Topological sort needs parents, so return them here
                            Ok((Some((bcs_id, parents)), parents_to_visit))
                        }
                    }
                }
            }
        })
        .try_filter_map(|x| async { Ok(x) })
        .try_collect()
        .await?;

    let topo_sorted_commit_graph =
        sort_topological(&commits_not_derived_to_parents).expect("commit graph has cycles!");
    let sz = topo_sorted_commit_graph.len();
    if sz > 100 {
        warn!(
            ctx.logger(),
            "derive_impl is called on a graph of size {}", sz
        );
    }
    let all_csids: Vec<_> = topo_sorted_commit_graph
        .into_iter()
        // Note - sort_topological returns all nodes including commits which were already
        // derived i.e. sort_topological({"a" -> ["b"]}) return ("a", "b").
        // The '.filter()' below removes ["b"]
        .filter(move |cs_id| commits_not_derived_to_parents.contains_key(cs_id))
        .collect();

    Ok(all_csids)
}

// Panics if any of the parents is not derived yet
async fn derive_may_panic<Derived, Mapping>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    mapping: &Mapping,
    bcs_id: &ChangesetId,
) -> Result<(), Error>
where
    Derived: BonsaiDerived,
    Mapping: BonsaiDerivedMapping<Value = Derived> + Send + Sync + Clone + 'static,
{
    debug!(
        ctx.logger(),
        "derive {} for {}",
        Derived::NAME,
        bcs_id.to_hex()
    );

    let lease = repo.get_derived_data_lease_ops();
    let lease_key = Arc::new(format!(
        "repo{}.{}.{}",
        repo.get_repoid().id(),
        Derived::NAME,
        bcs_id
    ));

    let res = derive_in_loop(ctx, repo, mapping, *bcs_id, &lease, &lease_key).await;

    res
}

async fn derive_in_loop<Derived, Mapping>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    mapping: &Mapping,
    bcs_id: ChangesetId,
    lease: &Arc<dyn LeaseOps>,
    lease_key: &Arc<String>,
) -> Result<(), Error>
where
    Derived: BonsaiDerived,
    Mapping: BonsaiDerivedMapping<Value = Derived> + Send + Sync + Clone + 'static,
{
    let event_id = EventId::new();
    let changeset_fetcher = repo.get_changeset_fetcher();
    let parents = async {
        let parents = changeset_fetcher
            .get_parents(ctx.clone(), bcs_id)
            .compat()
            .await?;

        try_join_all(
            parents
                .into_iter()
                .map(|p| fetch_derived_may_panic(ctx, p, mapping)),
        )
        .await
    };

    let bcs_fut = bcs_id
        .load(ctx.clone(), repo.blobstore())
        .map_err(Error::from);
    let (parents, bcs) = try_join(parents, bcs_fut).await?;

    let mut lease_start = Instant::now();
    let mut lease_total = Duration::from_secs(0);
    let mut backoff_ms = 200;

    loop {
        let result = lease.try_add_put_lease(&lease_key).compat().await;
        // In case of lease unavailability we do not want to stall
        // generation of all derived data, since lease is a soft lock
        // it is safe to assume that we successfuly acquired it
        let (leased, ignored) = match result {
            Ok(leased) => {
                let elapsed = lease_start.elapsed();
                if elapsed > LEASE_WARNING_THRESHOLD {
                    lease_total += elapsed;
                    lease_start = Instant::now();
                    warn!(
                        ctx.logger(),
                        "Can not acquire lease {} for more than {:?}", lease_key, lease_total
                    );
                }
                (leased, false)
            }
            Err(_) => (false, true),
        };

        let mut vs = mapping.get(ctx.clone(), vec![bcs_id]).compat().await?;
        let derived = vs.remove(&bcs_id);

        match derived {
            Some(_) => {
                break;
            }
            None => {
                if leased || ignored {
                    let deriver = async {
                        let derived =
                            Derived::derive_from_parents(ctx.clone(), repo.clone(), bcs, parents)
                                .traced_with_id(
                                    &ctx.trace(),
                                    "derive::derive_from_parents",
                                    trace_args! {
                                        "csid" => bcs_id.to_hex().to_string(),
                                        "type" => Derived::NAME
                                    },
                                    event_id,
                                )
                                .compat()
                                .await?;
                        mapping.put(ctx.clone(), bcs_id, derived).compat().await?;
                        let res: Result<_, Error> = Ok(());
                        res
                    };

                    let (sender, receiver) = oneshot::channel();
                    lease.renew_lease_until(
                        ctx.clone(),
                        &lease_key,
                        receiver.map_err(|_| ()).boxify(),
                    );

                    let derived_data_config = repo.get_derived_data_config();
                    let mut derived_data_scuba = init_derived_data_scuba::<Derived>(
                        &ctx,
                        repo.name(),
                        &derived_data_config,
                        &bcs_id,
                    );

                    log_derivation_start::<Derived>(&ctx, &mut derived_data_scuba, &bcs_id);
                    let (stats, res) = deriver.timed().await;
                    log_derivation_end::<Derived>(
                        &ctx,
                        &mut derived_data_scuba,
                        &bcs_id,
                        &stats,
                        res.is_ok(),
                    );
                    let _ = sender.send(());
                    res?;
                    break;
                } else {
                    let sleep = rand::random::<u64>() % backoff_ms;
                    let _ = tokio::time::delay_for(Duration::from_millis(sleep)).await;

                    backoff_ms *= 2;
                    if backoff_ms >= 1000 {
                        backoff_ms = 1000;
                    }
                    continue;
                }
            }
        }
    }
    Ok(())
}

async fn fetch_derived_may_panic<Derived, Mapping>(
    ctx: &CoreContext,
    bcs_id: ChangesetId,
    derived_mapping: &Mapping,
) -> Result<Derived, Error>
where
    Derived: BonsaiDerived,
    Mapping: BonsaiDerivedMapping<Value = Derived> + Send + Sync + Clone,
{
    let derive_node = DeriveNode::from_bonsai(ctx, derived_mapping, &bcs_id).await?;
    match derive_node {
        DeriveNode::Derived(derived) => Ok(derived),
        DeriveNode::Bonsai(_) => {
            panic!("{} should be derived already", bcs_id);
        }
    }
}

fn init_derived_data_scuba<Derived: BonsaiDerived>(
    ctx: &CoreContext,
    name: &str,
    derived_data_config: &DerivedDataConfig,
    bcs_id: &ChangesetId,
) -> ScubaSampleBuilder {
    match &derived_data_config.scuba_table {
        Some(scuba_table) => {
            let mut builder = ScubaSampleBuilder::new(ctx.fb, scuba_table);
            builder.add_common_server_data();
            builder.add("derived_data", Derived::NAME);
            builder.add("reponame", name);
            builder.add("changeset", format!("{}", bcs_id));
            builder
        }
        None => ScubaSampleBuilder::with_discard(),
    }
}

fn log_derivation_start<Derived>(
    ctx: &CoreContext,
    derived_data_scuba: &mut ScubaSampleBuilder,
    bcs_id: &ChangesetId,
) where
    Derived: BonsaiDerived,
{
    let tag = "Generating derived data";
    ctx.scuba()
        .clone()
        .log_with_msg(tag, Some(format!("{} {}", Derived::NAME, bcs_id)));
    // derived data name and bcs_id already logged as separate fields
    derived_data_scuba.log_with_msg(tag, None);
}

fn log_derivation_end<Derived>(
    ctx: &CoreContext,
    derived_data_scuba: &mut ScubaSampleBuilder,
    bcs_id: &ChangesetId,
    stats: &FutureStats,
    success: bool,
) where
    Derived: BonsaiDerived,
{
    let tag = if success {
        "Generated derived data"
    } else {
        "Failed to generate derived data"
    };

    let msg = Some(format!("{} {}", Derived::NAME, bcs_id));
    ctx.scuba()
        .clone()
        .add_future_stats(&stats)
        .log_with_msg(tag, msg.clone());

    derived_data_scuba
        .add_future_stats(&stats)
        // derived data name and bcs_id already logged as separate fields
        .log_with_msg(tag, None);

    STATS::derived_data_latency.add_value(
        stats.completion_time.as_millis_unchecked() as i64,
        (Derived::NAME,),
    );
}

#[derive(Clone, Copy)]
enum DeriveNode<Derived> {
    /// Already derived value fetched from mapping
    Derived(Derived),
    /// Bonsai changeset which requires derivation
    Bonsai(ChangesetId),
}

impl<Derived: BonsaiDerived> DeriveNode<Derived> {
    async fn from_bonsai<Mapping>(
        ctx: &CoreContext,
        derived_mapping: &Mapping,
        csid: &ChangesetId,
    ) -> Result<Self, Error>
    where
        Mapping: BonsaiDerivedMapping<Value = Derived> + Clone,
    {
        // TODO: do not create intermediate hashmap, since this methods is going to be called
        //       most often, to get derived value
        let csids_to_id = derived_mapping
            .get(ctx.clone(), vec![*csid])
            .compat()
            .await?;
        match csids_to_id.get(csid) {
            Some(id) => Ok(DeriveNode::Derived(id.clone())),
            None => Ok(DeriveNode::Bonsai(*csid)),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use anyhow::Error;
    use blobrepo_hg::BlobRepoHg;
    use blobrepo_override::DangerousOverride;
    use bookmarks::BookmarkName;
    use cacheblob::LeaseOps;
    use cloned::cloned;
    use context::SessionId;
    use fbinit::FacebookInit;
    use fixtures::{
        branch_even, branch_uneven, branch_wide, linear, many_diamonds, many_files_dirs,
        merge_even, merge_uneven, unshared_merge_even, unshared_merge_uneven,
    };
    use futures::compat::Future01CompatExt;
    use futures_ext::BoxFuture;
    use futures_old::{future, Stream};
    use lazy_static::lazy_static;
    use lock_ext::LockExt;
    use maplit::hashmap;
    use mercurial_types::HgChangesetId;
    use metaconfig_types::DerivedDataConfig;
    use mononoke_types::BonsaiChangeset;
    use revset::AncestorsNodeStream;
    use std::{
        collections::HashMap,
        str::FromStr,
        sync::{Arc, Mutex},
        time::Duration,
    };
    use tests_utils::resolve_cs_id;
    use tokio_compat::runtime::Runtime;

    lazy_static! {
        static ref MAPPINGS: Mutex<HashMap<SessionId, TestMapping>> = Mutex::new(HashMap::new());
    }

    #[derive(Clone, Hash, Eq, Ord, PartialEq, PartialOrd, Debug)]
    struct TestGenNum(u64, ChangesetId, Vec<ChangesetId>);

    impl BonsaiDerived for TestGenNum {
        const NAME: &'static str = "test_gen_num";
        type Mapping = TestMapping;

        fn mapping(ctx: &CoreContext, _repo: &BlobRepo) -> Self::Mapping {
            let session = ctx.session_id().clone();
            MAPPINGS.with(|m| m.entry(session).or_insert_with(TestMapping::new).clone())
        }

        fn derive_from_parents(
            _ctx: CoreContext,
            _repo: BlobRepo,
            bonsai: BonsaiChangeset,
            parents: Vec<Self>,
        ) -> BoxFuture<Self, Error> {
            let parent_commits = parents.iter().map(|x| x.1).collect();

            future::ok(Self(
                parents.into_iter().max().map(|x| x.0).unwrap_or(0) + 1,
                bonsai.get_changeset_id(),
                parent_commits,
            ))
            .boxify()
        }
    }

    #[derive(Debug, Clone)]
    struct TestMapping {
        mapping: Arc<Mutex<HashMap<ChangesetId, TestGenNum>>>,
    }

    impl TestMapping {
        fn new() -> Self {
            Self {
                mapping: Arc::new(Mutex::new(hashmap! {})),
            }
        }

        fn remove(&self, cs_id: &ChangesetId) {
            self.mapping.with(|m| m.remove(cs_id));
        }
    }

    impl BonsaiDerivedMapping for TestMapping {
        type Value = TestGenNum;

        fn get(
            &self,
            _ctx: CoreContext,
            csids: Vec<ChangesetId>,
        ) -> BoxFuture<HashMap<ChangesetId, Self::Value>, Error> {
            let mut res = hashmap! {};
            {
                let mapping = self.mapping.lock().unwrap();
                for id in csids {
                    if let Some(gen_num) = mapping.get(&id) {
                        res.insert(id, gen_num.clone());
                    }
                }
            }

            future::ok(res).boxify()
        }

        fn put(
            &self,
            _ctx: CoreContext,
            csid: ChangesetId,
            id: Self::Value,
        ) -> BoxFuture<(), Error> {
            {
                let mut mapping = self.mapping.lock().unwrap();
                mapping.insert(csid, id);
            }
            future::ok(()).boxify()
        }
    }

    fn derive_for_master(runtime: &mut Runtime, ctx: CoreContext, repo: BlobRepo) {
        let repo = repo.dangerous_override(|mut derived_data_config: DerivedDataConfig| {
            derived_data_config
                .derived_data_types
                .insert(TestGenNum::NAME.to_string());
            derived_data_config
        });

        let master_book = BookmarkName::new("master").unwrap();
        let bcs_id = runtime
            .block_on(repo.get_bonsai_bookmark(ctx.clone(), &master_book))
            .unwrap()
            .unwrap();
        let expected = runtime
            .block_on(
                repo.get_changeset_fetcher()
                    .get_generation_number(ctx.clone(), bcs_id.clone()),
            )
            .unwrap();

        let mapping = TestGenNum::mapping(&ctx, &repo);
        let actual = runtime
            .block_on(TestGenNum::derive(ctx.clone(), repo.clone(), bcs_id))
            .unwrap();
        assert_eq!(expected.value(), actual.0);

        let changeset_fetcher = repo.get_changeset_fetcher();
        runtime
            .block_on(
                AncestorsNodeStream::new(
                    ctx.clone(),
                    &repo.get_changeset_fetcher(),
                    bcs_id.clone(),
                )
                .and_then(move |new_bcs_id| {
                    let parents = changeset_fetcher.get_parents(ctx.clone(), new_bcs_id.clone());
                    let mapping = mapping.get(ctx.clone(), vec![new_bcs_id]);

                    parents.join(mapping).map(move |(parents, mapping)| {
                        let gen_num = mapping.get(&new_bcs_id).unwrap();
                        assert_eq!(parents, gen_num.2);
                    })
                })
                .collect(),
            )
            .unwrap();
    }

    async fn init_linear(fb: FacebookInit) -> BlobRepo {
        linear::getrepo(fb).await.dangerous_override(
            |mut derived_data_config: DerivedDataConfig| {
                derived_data_config
                    .derived_data_types
                    .insert(TestGenNum::NAME.to_string());
                derived_data_config
            },
        )
    }

    #[fbinit::test]
    fn test_incomplete_maping(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut runtime = Runtime::new()?;

        runtime.block_on_std(async move {
            let repo = init_linear(fb).await;

            // This is the parent of the root commit
            // ...
            //  O <- 3e0e761030db6e479a7fb58b12881883f9f8c63f
            //  |
            //  O <- 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536
            let after_root_cs_id =
                resolve_cs_id(&ctx, &repo, "3e0e761030db6e479a7fb58b12881883f9f8c63f").await?;
            let root_cs_id =
                resolve_cs_id(&ctx, &repo, "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").await?;

            TestGenNum::derive(ctx.clone(), repo.clone(), after_root_cs_id)
                .compat()
                .await?;

            // Delete root entry, and derive descendant of after_root changeset, make sure
            // it doesn't fail
            TestGenNum::mapping(&ctx, &repo).remove(&root_cs_id);
            TestGenNum::derive(ctx.clone(), repo.clone(), after_root_cs_id)
                .compat()
                .await?;

            let third_cs_id =
                resolve_cs_id(&ctx, &repo, "607314ef579bd2407752361ba1b0c1729d08b281").await?;
            TestGenNum::derive(ctx.clone(), repo.clone(), third_cs_id)
                .compat()
                .await?;

            Ok(())
        })
    }

    #[fbinit::test]
    fn test_count_underived(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut runtime = Runtime::new()?;

        runtime.block_on_std(async move {
            let repo = init_linear(fb).await;

            // This is the parent of the root commit
            // ...
            //  O <- 3e0e761030db6e479a7fb58b12881883f9f8c63f
            //  |
            //  O <- 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536
            let after_root_cs_id =
                resolve_cs_id(&ctx, &repo, "3e0e761030db6e479a7fb58b12881883f9f8c63f").await?;
            let root_cs_id =
                resolve_cs_id(&ctx, &repo, "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").await?;

            let underived =
                TestGenNum::count_underived(&ctx, &repo, &after_root_cs_id, 100).await?;
            assert_eq!(underived, 2);

            let underived = TestGenNum::count_underived(&ctx, &repo, &root_cs_id, 100).await?;
            assert_eq!(underived, 1);

            let underived = TestGenNum::count_underived(&ctx, &repo, &after_root_cs_id, 1).await?;
            assert_eq!(underived, 2);

            let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;
            let underived = TestGenNum::count_underived(&ctx, &repo, &master_cs_id, 100).await?;
            assert_eq!(underived, 11);

            Ok(())
        })
    }

    #[fbinit::test]
    fn test_derive_for_fixture_repos(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut runtime = Runtime::new()?;

        let repo = runtime.block_on_std(branch_even::getrepo(fb));
        derive_for_master(&mut runtime, ctx.clone(), repo.clone());

        let repo = runtime.block_on_std(branch_uneven::getrepo(fb));
        derive_for_master(&mut runtime, ctx.clone(), repo.clone());

        let repo = runtime.block_on_std(branch_wide::getrepo(fb));
        derive_for_master(&mut runtime, ctx.clone(), repo.clone());

        let repo = runtime.block_on_std(linear::getrepo(fb));
        derive_for_master(&mut runtime, ctx.clone(), repo.clone());

        let repo = runtime.block_on_std(many_files_dirs::getrepo(fb));
        derive_for_master(&mut runtime, ctx.clone(), repo.clone());

        let repo = runtime.block_on_std(merge_even::getrepo(fb));
        derive_for_master(&mut runtime, ctx.clone(), repo.clone());

        let repo = runtime.block_on_std(merge_uneven::getrepo(fb));
        derive_for_master(&mut runtime, ctx.clone(), repo.clone());

        let repo = runtime.block_on_std(unshared_merge_even::getrepo(fb));
        derive_for_master(&mut runtime, ctx.clone(), repo.clone());

        let repo = runtime.block_on_std(unshared_merge_uneven::getrepo(fb));
        derive_for_master(&mut runtime, ctx.clone(), repo.clone());

        let repo = runtime.block_on_std(many_diamonds::getrepo(fb));
        derive_for_master(&mut runtime, ctx.clone(), repo.clone());

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_batch_derive(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let from_batch = {
            let repo = linear::getrepo(fb).await;
            let repo = repo.dangerous_override(|mut derived_data_config: DerivedDataConfig| {
                derived_data_config
                    .derived_data_types
                    .insert(TestGenNum::NAME.to_string());
                derived_data_config
            });
            let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;

            let cs_ids =
                AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), master_cs_id)
                    .collect()
                    .compat()
                    .await?;
            // Reverse them to derive parents before children
            let cs_ids = cs_ids.clone().into_iter().rev().collect::<Vec<_>>();
            let derived_batch = TestGenNum::batch_derive(&ctx, &repo, cs_ids).await?;
            derived_batch
                .get(&master_cs_id)
                .unwrap_or_else(|| panic!("{} has not been derived", master_cs_id))
                .clone()
        };

        let sequential = {
            let repo = linear::getrepo(fb).await;
            let repo = repo.dangerous_override(|mut derived_data_config: DerivedDataConfig| {
                derived_data_config
                    .derived_data_types
                    .insert(TestGenNum::NAME.to_string());
                derived_data_config
            });
            let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;
            TestGenNum::derive(ctx.clone(), repo.clone(), master_cs_id)
                .compat()
                .await?
        };

        assert_eq!(from_batch, sequential);
        Ok(())
    }

    #[fbinit::test]
    fn test_leases(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut runtime = Runtime::new()?;
        let repo = runtime.block_on_std(init_linear(fb));
        let mapping = TestGenNum::mapping(&ctx, &repo);

        let hg_csid = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
        let csid = runtime
            .block_on(repo.get_bonsai_from_hg(ctx.clone(), hg_csid))?
            .ok_or(Error::msg("known hg does not have bonsai csid"))?;

        let lease = repo.get_derived_data_lease_ops();
        let lease_key = Arc::new(format!(
            "repo{}.{}.{}",
            repo.get_repoid().id(),
            TestGenNum::NAME,
            csid
        ));

        // take lease
        assert_eq!(
            runtime.block_on(lease.try_add_put_lease(&lease_key)),
            Ok(true)
        );
        assert_eq!(
            runtime.block_on(lease.try_add_put_lease(&lease_key)),
            Ok(false)
        );

        let output = Arc::new(Mutex::new(Vec::new()));
        runtime.spawn(TestGenNum::derive(ctx.clone(), repo.clone(), csid).then({
            cloned!(output);
            move |result| {
                output.with(move |output| output.push(result));
                Ok::<_, ()>(())
            }
        }));

        // schedule derivation
        runtime.block_on(tokio_timer::sleep(Duration::from_millis(300)))?;
        assert_eq!(
            runtime.block_on(mapping.get(ctx.clone(), vec![csid]))?,
            HashMap::new()
        );

        // release lease
        runtime
            .block_on(lease.release_lease(&lease_key))
            .map_err(|_| Error::msg("failed to release a lease"))?;

        runtime.block_on(tokio_timer::sleep(Duration::from_millis(3000)))?;
        let result = match output.with(|output| output.pop()) {
            Some(result) => result?,
            None => panic!("scheduled derivation should have been completed"),
        };
        assert_eq!(
            runtime.block_on(mapping.get(ctx.clone(), vec![csid]))?,
            hashmap! { csid => result.clone() }
        );

        // take lease
        assert_eq!(
            runtime.block_on(lease.try_add_put_lease(&lease_key)),
            Ok(true),
        );
        // should succed as lease should not be request
        assert_eq!(
            runtime.block_on(TestGenNum::derive(ctx.clone(), repo.clone(), csid))?,
            result
        );
        runtime
            .block_on(lease.release_lease(&lease_key))
            .map_err(|_| Error::msg("failed to release a lease"))?;

        Ok(())
    }

    #[derive(Debug)]
    struct FailingLease;

    impl LeaseOps for FailingLease {
        fn try_add_put_lease(&self, _key: &str) -> BoxFuture<bool, ()> {
            future::err(()).boxify()
        }

        fn renew_lease_until(&self, _ctx: CoreContext, _key: &str, _done: BoxFuture<(), ()>) {}

        fn wait_for_other_leases(&self, _key: &str) -> BoxFuture<(), ()> {
            future::err(()).boxify()
        }

        fn release_lease(&self, _key: &str) -> BoxFuture<(), ()> {
            future::err(()).boxify()
        }
    }

    #[fbinit::test]
    fn test_always_failing_lease(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = Runtime::new()?;

        let ctx = CoreContext::test_mock(fb);
        let repo = runtime
            .block_on_std(init_linear(fb))
            .dangerous_override(|_| Arc::new(FailingLease) as Arc<dyn LeaseOps>);
        let mapping = TestGenNum::mapping(&ctx, &repo);

        let hg_csid = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
        let csid = runtime
            .block_on(repo.get_bonsai_from_hg(ctx.clone(), hg_csid))?
            .ok_or(Error::msg("known hg does not have bonsai csid"))?;

        let lease = repo.get_derived_data_lease_ops();
        let lease_key = Arc::new(format!(
            "repo{}.{}.{}",
            repo.get_repoid().id(),
            TestGenNum::NAME,
            csid
        ));

        // takig lease should fail
        assert_eq!(
            runtime.block_on(lease.try_add_put_lease(&lease_key)),
            Err(())
        );

        // should succeed even though lease always fails
        let result = runtime.block_on(TestGenNum::derive(ctx.clone(), repo.clone(), csid))?;
        assert_eq!(
            runtime.block_on(mapping.get(ctx.clone(), vec![csid]))?,
            hashmap! { csid => result },
        );

        Ok(())
    }
}
