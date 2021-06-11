/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{BonsaiDerivable, BonsaiDerivedMapping, DeriveError};
use anyhow::{Error, Result};
use blobrepo::BlobRepo;
use blobstore::Loadable;
use cacheblob::LeaseOps;
use cloned::cloned;
use context::CoreContext;
use futures::{
    channel::oneshot,
    future::{abortable, try_join, try_join_all, Aborted, FutureExt, TryFutureExt},
    stream::FuturesUnordered,
    TryStreamExt,
};
use futures_stats::{futures03::TimedFutureExt, FutureStats};
use metaconfig_types::{DerivedDataConfig, DerivedDataTypesConfig};
use mononoke_types::ChangesetId;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::debug;
use slog::warn;
use stats::prelude::*;
use std::convert::TryInto;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use time_ext::DurationExt;
use topo_sort::{sort_topological, TopoSortedDagTraversal};

define_stats! {
    prefix = "mononoke.derived_data";
    derived_data_latency:
        dynamic_timeseries("{}.deriving.latency_ms", (derived_data_type: &'static str); Average),
    derived_data_disabled:
        dynamic_timeseries("{}.{}.derived_data_disabled", (repo_id: i32, derived_data_type: &'static str); Count),
}

const LEASE_WARNING_THRESHOLD: Duration = Duration::from_secs(60);

/// Checks that the named derived data type is enabled, and returns the
/// enabled derived data types config if it is.  Returns an error if the
/// derived data type is not enabled.
pub fn enabled_type_config<'repo>(
    repo: &'repo BlobRepo,
    name: &'static str,
) -> Result<&'repo DerivedDataTypesConfig, DeriveError> {
    let config = repo.get_derived_data_config();
    let mut enabled = true;
    if !config.enabled.types.contains(name) {
        enabled = false;
    }

    if emergency_disabled(repo, name) {
        enabled = false;
    }

    if enabled {
        let config = repo.get_derived_data_config();
        Ok(&config.enabled)
    } else {
        STATS::derived_data_disabled.add_value(1, (repo.get_repoid().id(), name));
        Err(DeriveError::Disabled(
            name,
            repo.get_repoid(),
            repo.name().clone(),
        ))
    }
}

fn emergency_disabled(repo: &BlobRepo, name: &str) -> bool {
    let disabled_for_repo = tunables::tunables()
        .get_by_repo_all_derived_data_disabled(repo.name())
        .unwrap_or(false);

    if disabled_for_repo {
        return true;
    }

    let disabled_for_type = tunables::tunables()
        .get_by_repo_derived_data_types_disabled(repo.name())
        .unwrap_or(vec![]);

    if disabled_for_type
        .iter()
        .find(|ty| ty.as_str() == name)
        .is_some()
    {
        return true;
    }

    // Not disabled
    false
}

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
pub async fn derive_impl<
    Derivable: BonsaiDerivable,
    Mapping: BonsaiDerivedMapping<Value = Derivable> + Send + Sync + Clone + 'static,
>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    derived_mapping: &Mapping,
    start_csid: ChangesetId,
) -> Result<Derivable, DeriveError> {
    let derivation = async {
        let derive_and_log = |csid: ChangesetId| {
            cloned!(ctx, derived_mapping, repo);
            async move {
                ctx.scuba().clone().log_with_msg(
                    "Waiting for derived data to be generated",
                    Some(format!("{} {}", Derivable::NAME, csid)),
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
                    .log_with_msg(tag, Some(format!("{} {}", Derivable::NAME, csid)));
                res?;
                Result::<_, Error>::Ok(csid)
            }
        };

        // T92169040 - remove get_derived_data_disable_parallel_derivation
        if !tunables::tunables().get_derived_data_disable_parallel_derivation() {
            let commits_not_derived_to_parents =
                find_underived(ctx, repo, derived_mapping, Some(start_csid), None).await?;
            let mut dag_traversal = TopoSortedDagTraversal::new(commits_not_derived_to_parents);
            let mut inflight_futs = FuturesUnordered::new();

            let buffer_size = tunables::tunables().get_derived_data_parallel_derivation_buffer();
            let buffer_size: usize = if buffer_size > 0 {
                buffer_size.try_into().unwrap()
            } else {
                10
            };
            let mut count = 0;
            while !dag_traversal.is_empty() || !inflight_futs.is_empty() {
                let free = buffer_size.saturating_sub(inflight_futs.len());
                inflight_futs.extend(
                    dag_traversal
                        .drain(free)
                        .map(|csid| tokio::spawn(derive_and_log(csid))),
                );
                if let Some(derived) = inflight_futs.try_next().await? {
                    count += 1;
                    dag_traversal.visited(derived?);
                }
            }

            Result::<_, Error>::Ok(count)
        } else {
            let all_csids =
                find_topo_sorted_underived(ctx, repo, derived_mapping, Some(start_csid), None)
                    .await?;

            for cs_id in &all_csids {
                derive_and_log(*cs_id).await?;
            }

            Result::<_, Error>::Ok(all_csids.len())
        }
    };

    let (derivation, derivation_abort_handle) = abortable(derivation);

    // Watcher checks if derived data was disabled for a given type,
    // and if yes it cancells derivation.
    let watcher = {
        let repo = repo.clone();
        async move {
            let mut delay_secs =
                tunables::tunables().get_derived_data_disabled_watcher_delay_secs();
            if delay_secs <= 0 {
                delay_secs = 10;
            }
            let delay_secs: u64 = delay_secs.try_into().unwrap();
            loop {
                if emergency_disabled(&repo, Derivable::NAME) {
                    derivation_abort_handle.abort();
                    break;
                }
                tokio::time::sleep(Duration::from_secs(delay_secs)).await;
            }
        }
    };

    let (watcher, watcher_abort_handle) = abortable(watcher);
    tokio::spawn(watcher);


    let (stats, res) = derivation.timed().await;
    let res = match res {
        Ok(res) => res.map_err(DeriveError::Error),
        Err(Aborted) => Err(DeriveError::Disabled(
            Derivable::NAME,
            repo.get_repoid(),
            repo.name().clone(),
        )),
    };
    watcher_abort_handle.abort();

    let count = match res {
        Ok(ref count) => *count,
        Err(_) => 0,
    };

    if should_log_slow_derivation(stats.completion_time) {
        warn!(
            ctx.logger(),
            "slow derivation of {} {} for {}, took {:.2?}",
            count,
            Derivable::NAME,
            start_csid,
            stats.completion_time,
        );
        ctx.scuba().clone().add_future_stats(&stats).log_with_msg(
            "Slow derivation",
            Some(format!(
                "type={},count={},csid={}",
                Derivable::NAME,
                count,
                start_csid.to_string()
            )),
        );
    }

    res?;

    let derived = fetch_derived_may_panic(&ctx, start_csid, derived_mapping).await?;
    Ok(derived)
}

fn should_log_slow_derivation(duration: Duration) -> bool {
    let threshold = tunables::tunables().get_derived_data_slow_derivation_threshold_secs();
    let threshold = match threshold.try_into() {
        Ok(t) if t > 0 => t,
        _ => return false,
    };
    duration > Duration::from_secs(threshold)
}

pub async fn find_underived<
    Derivable: BonsaiDerivable,
    Mapping: BonsaiDerivedMapping<Value = Derivable> + Send + Sync + Clone + 'static,
    Changesets: IntoIterator<Item = ChangesetId>,
>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    derived_mapping: &Mapping,
    start_csids: Changesets,
    limit: Option<u64>,
) -> Result<HashMap<ChangesetId, Vec<ChangesetId>>, Error> {
    let changeset_fetcher = repo.get_changeset_fetcher();
    // This is necessary to avoid visiting the same commit a lot of times in mergy repos
    let visited: Arc<Mutex<HashSet<ChangesetId>>> = Arc::new(Mutex::new(HashSet::new()));

    let changeset_fetcher = &changeset_fetcher;
    let visited = &visited;
    let commits_not_derived_to_parents: HashMap<_, _> =
        bounded_traversal::bounded_traversal_stream(100, start_csids, {
            move |cs_id| {
                async move {
                    if let Some(limit) = limit {
                        let visited = visited.lock().unwrap();
                        if visited.len() as u64 > limit {
                            return Result::<_, Error>::Ok((None, vec![]));
                        }
                    }

                    let derive_node = DeriveNode::from_bonsai(ctx, derived_mapping, &cs_id).await?;

                    match derive_node {
                        DeriveNode::Derived(_) => Ok((None, vec![])),
                        DeriveNode::Bonsai(bcs_id) => {
                            let parents =
                                changeset_fetcher.get_parents(ctx.clone(), bcs_id).await?;

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
                .boxed()
            }
        })
        .try_filter_map(|x| async { Ok(x) })
        .try_collect()
        .await?;

    let sz = commits_not_derived_to_parents.len();
    if sz > 100 {
        warn!(
            ctx.logger(),
            "derive_impl is called on a graph of size {}", sz
        );
    }

    // Remove parents that were already derived - no need to derive them again
    let commits_not_derived_to_parents = commits_not_derived_to_parents
        .iter()
        .map(|(cs_id, parents)| {
            let parents = parents
                .iter()
                .cloned()
                .filter(|p| commits_not_derived_to_parents.contains_key(p))
                .collect::<Vec<_>>();
            (cs_id.clone(), parents)
        })
        .collect::<HashMap<_, _>>();

    Ok(commits_not_derived_to_parents)
}

pub async fn find_topo_sorted_underived<
    Derivable: BonsaiDerivable,
    Mapping: BonsaiDerivedMapping<Value = Derivable> + Send + Sync + Clone + 'static,
    Changesets: IntoIterator<Item = ChangesetId>,
>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    derived_mapping: &Mapping,
    start_csids: Changesets,
    limit: Option<u64>,
) -> Result<Vec<ChangesetId>, Error> {
    let commits_not_derived_to_parents =
        find_underived(ctx, repo, derived_mapping, start_csids, limit).await?;

    let topo_sorted_commit_graph =
        sort_topological(&commits_not_derived_to_parents).expect("commit graph has cycles!");

    Ok(topo_sorted_commit_graph)
}

// Panics if any of the parents is not derived yet
async fn derive_may_panic<Derivable, Mapping>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    mapping: &Mapping,
    bcs_id: &ChangesetId,
) -> Result<(), Error>
where
    Derivable: BonsaiDerivable,
    Mapping: BonsaiDerivedMapping<Value = Derivable> + Send + Sync + Clone + 'static,
{
    debug!(
        ctx.logger(),
        "derive {} for {}",
        Derivable::NAME,
        bcs_id.to_hex()
    );

    let lease = repo.get_derived_data_lease_ops();
    let lease_key = Arc::new(format!(
        "repo{}.{}.{}",
        repo.get_repoid().id(),
        Derivable::NAME,
        bcs_id
    ));

    let res = derive_in_loop(ctx, repo, mapping, *bcs_id, &lease, &lease_key).await;

    res
}

async fn derive_in_loop<Derivable, Mapping>(
    ctx: &CoreContext,
    repo: &BlobRepo,
    mapping: &Mapping,
    bcs_id: ChangesetId,
    lease: &Arc<dyn LeaseOps>,
    lease_key: &Arc<String>,
) -> Result<(), Error>
where
    Derivable: BonsaiDerivable,
    Mapping: BonsaiDerivedMapping<Value = Derivable> + Send + Sync + Clone + 'static,
{
    let changeset_fetcher = repo.get_changeset_fetcher();
    let parents = async {
        let parents = changeset_fetcher.get_parents(ctx.clone(), bcs_id).await?;

        try_join_all(
            parents
                .into_iter()
                .map(|p| fetch_derived_may_panic(ctx, p, mapping)),
        )
        .await
    };

    let bcs_fut = bcs_id.load(ctx, repo.blobstore()).map_err(Error::from);
    let (parents, bcs) = try_join(parents, bcs_fut).await?;

    let mut lease_start = Instant::now();
    let mut lease_total = Duration::from_secs(0);
    let mut backoff_ms = 200;

    loop {
        let result = lease.try_add_put_lease(&lease_key).await;
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

        let mut vs = mapping.get(ctx.clone(), vec![bcs_id]).await?;
        let derived = vs.remove(&bcs_id);

        match derived {
            Some(_) => {
                break;
            }
            None => {
                if leased || ignored {
                    // Get a new context for derivation. This means derivation won't count against
                    // the original context's perf counters, but there will still be logs to Scuba
                    // there to indicate that derivation occcurs. It lets us capture exact perf
                    // counters for derivation and log those to the derived data table in Scuba.
                    let ctx = ctx.clone_and_reset();

                    let deriver = async {
                        let options = mapping.options();
                        let derived = Derivable::derive_from_parents(
                            ctx.clone(),
                            repo.clone(),
                            bcs,
                            parents,
                            &options,
                        )
                        .await?;
                        mapping.put(ctx.clone(), bcs_id, derived).await?;
                        let res: Result<_, Error> = Ok(());
                        res
                    };

                    let (sender, receiver) = oneshot::channel();
                    lease.renew_lease_until(ctx.clone(), &lease_key, receiver.map(|_| ()).boxed());

                    let derived_data_config = repo.get_derived_data_config();
                    let mut derived_data_scuba = init_derived_data_scuba::<Derivable>(
                        &ctx,
                        repo.name(),
                        &derived_data_config,
                        &bcs_id,
                    );

                    log_derivation_start::<Derivable>(&ctx, &mut derived_data_scuba, &bcs_id);
                    let (stats, res) = deriver.timed().await;
                    log_derivation_end::<Derivable>(
                        &ctx,
                        &mut derived_data_scuba,
                        &bcs_id,
                        &stats,
                        &res,
                    );
                    let _ = sender.send(());
                    res?;
                    break;
                } else {
                    let sleep = rand::random::<u64>() % backoff_ms;
                    tokio::time::sleep(Duration::from_millis(sleep)).await;

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

/// This function returns None if this item is not yet derived,  Some(Self) otherwise.
/// It does not derive if not already derived.
pub(crate) async fn fetch_derived<Derivable, Mapping>(
    ctx: &CoreContext,
    bcs_id: &ChangesetId,
    derived_mapping: &Mapping,
) -> Result<Option<Derivable>, Error>
where
    Derivable: BonsaiDerivable,
    Mapping: BonsaiDerivedMapping<Value = Derivable>,
{
    let derive_node = DeriveNode::from_bonsai(ctx, derived_mapping, bcs_id).await?;
    match derive_node {
        DeriveNode::Derived(derived) => Ok(Some(derived)),
        DeriveNode::Bonsai(_) => Ok(None),
    }
}

// Like fetch_derived but panics if not found
async fn fetch_derived_may_panic<Derivable, Mapping>(
    ctx: &CoreContext,
    bcs_id: ChangesetId,
    derived_mapping: &Mapping,
) -> Result<Derivable, Error>
where
    Derivable: BonsaiDerivable,
    Mapping: BonsaiDerivedMapping<Value = Derivable>,
{
    if let Some(derived) = fetch_derived(ctx, &bcs_id, derived_mapping).await? {
        Ok(derived)
    } else {
        panic!("{} should be derived already", bcs_id)
    }
}

fn init_derived_data_scuba<Derivable: BonsaiDerivable>(
    ctx: &CoreContext,
    name: &str,
    derived_data_config: &DerivedDataConfig,
    bcs_id: &ChangesetId,
) -> MononokeScubaSampleBuilder {
    match &derived_data_config.scuba_table {
        Some(scuba_table) => {
            let mut builder = MononokeScubaSampleBuilder::new(ctx.fb, scuba_table);
            builder.add_common_server_data();
            builder.add("derived_data", Derivable::NAME);
            builder.add("reponame", name);
            builder.add("changeset", format!("{}", bcs_id));
            builder
        }
        None => MononokeScubaSampleBuilder::with_discard(),
    }
}

fn log_derivation_start<Derivable>(
    ctx: &CoreContext,
    derived_data_scuba: &mut MononokeScubaSampleBuilder,
    bcs_id: &ChangesetId,
) where
    Derivable: BonsaiDerivable,
{
    let tag = "Generating derived data";
    ctx.scuba()
        .clone()
        .log_with_msg(tag, Some(format!("{} {}", Derivable::NAME, bcs_id)));
    // derived data name and bcs_id already logged as separate fields
    derived_data_scuba.log_with_msg(tag, None);
}

fn log_derivation_end<Derivable>(
    ctx: &CoreContext,
    derived_data_scuba: &mut MononokeScubaSampleBuilder,
    bcs_id: &ChangesetId,
    stats: &FutureStats,
    res: &Result<(), Error>,
) where
    Derivable: BonsaiDerivable,
{
    let tag = if res.is_ok() {
        "Generated derived data"
    } else {
        "Failed to generate derived data"
    };

    let msg = Some(format!("{} {}", Derivable::NAME, bcs_id));
    let mut scuba_sample = ctx.scuba().clone();
    scuba_sample.add_future_stats(&stats);
    if let Err(err) = res {
        scuba_sample.add("Derive error", format!("{:#}", err));
    };
    scuba_sample.log_with_msg(tag, msg.clone());

    ctx.perf_counters().insert_perf_counters(derived_data_scuba);

    let msg = match res {
        Ok(_) => None,
        Err(err) => Some(format!("{:#}", err)),
    };

    derived_data_scuba
        .add_future_stats(&stats)
        // derived data name and bcs_id already logged as separate fields
        .log_with_msg(tag, msg);

    STATS::derived_data_latency.add_value(
        stats.completion_time.as_millis_unchecked() as i64,
        (Derivable::NAME,),
    );
}

#[derive(Clone, Copy)]
enum DeriveNode<Derivable> {
    /// Already derived value fetched from mapping
    Derived(Derivable),
    /// Bonsai changeset which requires derivation
    Bonsai(ChangesetId),
}

impl<Derivable: BonsaiDerivable> DeriveNode<Derivable> {
    async fn from_bonsai<Mapping>(
        ctx: &CoreContext,
        derived_mapping: &Mapping,
        csid: &ChangesetId,
    ) -> Result<Self, Error>
    where
        Mapping: BonsaiDerivedMapping<Value = Derivable>,
    {
        // TODO: do not create intermediate hashmap, since this methods is going to be called
        //       most often, to get derived value
        let csids_to_id = derived_mapping.get(ctx.clone(), vec![*csid]).await?;
        match csids_to_id.get(csid) {
            Some(id) => Ok(DeriveNode::Derived(id.clone())),
            None => Ok(DeriveNode::Bonsai(*csid)),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use anyhow::{anyhow, Error};
    use async_trait::async_trait;
    use blobrepo_hg::BlobRepoHg;
    use bookmarks::BookmarkName;
    use cacheblob::LeaseOps;
    use cloned::cloned;
    use context::SessionId;
    use fbinit::FacebookInit;
    use fixtures::{
        branch_even, branch_uneven, branch_wide, linear, many_diamonds, many_files_dirs,
        merge_even, merge_uneven, unshared_merge_even, unshared_merge_uneven,
    };
    use futures::{compat::Stream01CompatExt, future::BoxFuture};
    use futures_stats::TimedTryFutureExt;
    use lazy_static::lazy_static;
    use lock_ext::LockExt;
    use maplit::hashmap;
    use mercurial_types::HgChangesetId;
    use mononoke_types::{BonsaiChangeset, MPath, RepositoryId};
    use revset::AncestorsNodeStream;
    use std::{
        collections::HashMap,
        str::FromStr,
        sync::{Arc, Mutex},
        time::Duration,
    };
    use test_repo_factory::TestRepoFactory;
    use tests_utils::{resolve_cs_id, CreateCommitContext};
    use tunables::{override_tunables, with_tunables, MononokeTunables};

    use crate::BonsaiDerived;

    lazy_static! {
        static ref MAPPINGS: Mutex<HashMap<SessionId, TestMapping>> = Mutex::new(HashMap::new());
        static ref DELAY: Mutex<HashMap<SessionId, Duration>> = Mutex::new(HashMap::new());
    }

    #[derive(Clone, Hash, Eq, Ord, PartialEq, PartialOrd, Debug)]
    struct TestGenNum(u64, ChangesetId, Vec<ChangesetId>);

    #[async_trait]
    impl BonsaiDerivable for TestGenNum {
        const NAME: &'static str = "test_gen_num";

        type Options = ();

        async fn derive_from_parents_impl(
            ctx: CoreContext,
            _repo: BlobRepo,
            bonsai: BonsaiChangeset,
            parents: Vec<Self>,
            _options: &Self::Options,
        ) -> Result<Self, Error> {
            let parent_commits = parents.iter().map(|x| x.1).collect();

            let session = ctx.metadata().session_id().clone();
            let duration = DELAY.with(|m| m.get(&session).cloned());
            if let Some(duration) = duration {
                tokio::time::sleep(duration).await;
            }

            Ok(Self(
                parents.into_iter().max().map(|x| x.0).unwrap_or(0) + 1,
                bonsai.get_changeset_id(),
                parent_commits,
            ))
        }
    }

    #[async_trait]
    impl BonsaiDerived for TestGenNum {
        type DefaultMapping = TestMapping;

        fn default_mapping(
            ctx: &CoreContext,
            _repo: &BlobRepo,
        ) -> Result<Self::DefaultMapping, DeriveError> {
            let session = ctx.metadata().session_id().clone();
            Ok(MAPPINGS.with(|m| m.entry(session).or_insert_with(TestMapping::new).clone()))
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

    #[async_trait]
    impl BonsaiDerivedMapping for TestMapping {
        type Value = TestGenNum;

        async fn get(
            &self,
            _ctx: CoreContext,
            csids: Vec<ChangesetId>,
        ) -> Result<HashMap<ChangesetId, Self::Value>, Error> {
            let mut res = hashmap! {};
            {
                let mapping = self.mapping.lock().unwrap();
                for id in csids {
                    if let Some(gen_num) = mapping.get(&id) {
                        res.insert(id, gen_num.clone());
                    }
                }
            }

            Ok(res)
        }

        async fn put(
            &self,
            _ctx: CoreContext,
            csid: ChangesetId,
            id: Self::Value,
        ) -> Result<(), Error> {
            {
                let mut mapping = self.mapping.lock().unwrap();
                mapping.insert(csid, id);
            }
            Ok(())
        }

        fn options(&self) {}
    }

    async fn derive_for_master(ctx: CoreContext, repo: BlobRepo) {
        let master_book = BookmarkName::new("master").unwrap();
        let bcs_id = repo
            .get_bonsai_bookmark(ctx.clone(), &master_book)
            .await
            .unwrap()
            .unwrap();
        let expected = repo
            .get_changeset_fetcher()
            .get_generation_number(ctx.clone(), bcs_id.clone())
            .await
            .unwrap();

        let mapping = &TestGenNum::default_mapping(&ctx, &repo).unwrap();
        let actual = TestGenNum::derive(&ctx, &repo, bcs_id).await.unwrap();
        assert_eq!(expected.value(), actual.0);

        let changeset_fetcher = repo.get_changeset_fetcher();
        AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), bcs_id.clone())
            .compat()
            .and_then(move |new_bcs_id| {
                cloned!(ctx, changeset_fetcher);
                async move {
                    let parents = changeset_fetcher.get_parents(ctx.clone(), new_bcs_id.clone());
                    let mapping = mapping.get(ctx, vec![new_bcs_id]);
                    let (parents, mapping) = try_join(parents, mapping).await?;
                    let gen_num = mapping.get(&new_bcs_id).unwrap();
                    assert_eq!(parents, gen_num.2);
                    Ok(())
                }
            })
            .try_collect::<Vec<_>>()
            .await
            .unwrap();
    }

    async fn init_linear(fb: FacebookInit) -> BlobRepo {
        let repo = TestRepoFactory::new()
            .unwrap()
            .with_config_override(|repo_config| {
                repo_config
                    .derived_data_config
                    .enabled
                    .types
                    .insert(TestGenNum::NAME.to_string());
            })
            .build()
            .unwrap();
        linear::initrepo(fb, &repo).await;
        repo
    }

    #[fbinit::test]
    async fn test_incomplete_maping(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

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

        TestGenNum::derive(&ctx, &repo, after_root_cs_id).await?;

        // Delete root entry, and derive descendant of after_root changeset, make sure
        // it doesn't fail
        TestGenNum::default_mapping(&ctx, &repo)?.remove(&root_cs_id);
        TestGenNum::derive(&ctx, &repo, after_root_cs_id).await?;

        let third_cs_id =
            resolve_cs_id(&ctx, &repo, "607314ef579bd2407752361ba1b0c1729d08b281").await?;
        TestGenNum::derive(&ctx, &repo, third_cs_id).await?;

        Ok(())
    }

    #[fbinit::test]
    async fn test_count_underived(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);

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

        let underived = TestGenNum::count_underived(&ctx, &repo, &after_root_cs_id, 100).await?;
        assert_eq!(underived, 2);

        let underived = TestGenNum::count_underived(&ctx, &repo, &root_cs_id, 100).await?;
        assert_eq!(underived, 1);

        let underived = TestGenNum::count_underived(&ctx, &repo, &after_root_cs_id, 1).await?;
        assert_eq!(underived, 2);

        let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;
        let underived = TestGenNum::count_underived(&ctx, &repo, &master_cs_id, 100).await?;
        assert_eq!(underived, 11);

        Ok(())
    }

    #[fbinit::test]
    async fn test_derive_for_fixture_repos(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut factory = TestRepoFactory::new()?;
        factory.with_config_override(|repo_config| {
            repo_config
                .derived_data_config
                .enabled
                .types
                .insert(TestGenNum::NAME.to_string());
        });

        let repo = factory.with_id(RepositoryId::new(1)).build()?;
        branch_even::initrepo(fb, &repo).await;
        derive_for_master(ctx.clone(), repo).await;

        let repo = factory.with_id(RepositoryId::new(2)).build()?;
        branch_uneven::initrepo(fb, &repo).await;
        derive_for_master(ctx.clone(), repo).await;

        let repo = factory.with_id(RepositoryId::new(3)).build()?;
        branch_wide::initrepo(fb, &repo).await;
        derive_for_master(ctx.clone(), repo).await;

        let repo = factory.with_id(RepositoryId::new(4)).build()?;
        linear::initrepo(fb, &repo).await;
        derive_for_master(ctx.clone(), repo).await;

        let repo = factory.with_id(RepositoryId::new(5)).build()?;
        many_files_dirs::initrepo(fb, &repo).await;
        derive_for_master(ctx.clone(), repo).await;

        let repo = factory.with_id(RepositoryId::new(6)).build()?;
        merge_even::initrepo(fb, &repo).await;
        derive_for_master(ctx.clone(), repo).await;

        let repo = factory.with_id(RepositoryId::new(7)).build()?;
        merge_uneven::initrepo(fb, &repo).await;
        derive_for_master(ctx.clone(), repo).await;

        let repo = factory.with_id(RepositoryId::new(8)).build()?;
        unshared_merge_even::initrepo(fb, &repo).await;
        derive_for_master(ctx.clone(), repo).await;

        let repo = factory.with_id(RepositoryId::new(9)).build()?;
        unshared_merge_uneven::initrepo(fb, &repo).await;
        derive_for_master(ctx.clone(), repo).await;

        let repo = factory.with_id(RepositoryId::new(10)).build()?;
        many_diamonds::initrepo(fb, &repo).await;
        derive_for_master(ctx.clone(), repo).await;

        Ok(())
    }

    #[fbinit::test]
    async fn test_batch_derive(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let from_batch = {
            let repo = init_linear(fb).await;
            let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;

            let cs_ids =
                AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), master_cs_id)
                    .compat()
                    .try_collect::<Vec<_>>()
                    .await?;
            // Reverse them to derive parents before children
            let cs_ids = cs_ids.clone().into_iter().rev().collect::<Vec<_>>();
            let mapping = TestGenNum::default_mapping(&ctx, &repo)?;
            let derived_batch =
                TestGenNum::batch_derive(&ctx, &repo, cs_ids, &mapping, None).await?;
            derived_batch
                .get(&master_cs_id)
                .unwrap_or_else(|| panic!("{} has not been derived", master_cs_id))
                .clone()
        };

        let sequential = {
            let repo = init_linear(fb).await;
            let master_cs_id = resolve_cs_id(&ctx, &repo, "master").await?;
            TestGenNum::derive(&ctx, &repo, master_cs_id).await?
        };

        assert_eq!(from_batch, sequential);
        Ok(())
    }

    #[fbinit::test]
    async fn test_leases(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_linear(fb).await;
        let mapping = TestGenNum::default_mapping(&ctx, &repo)?;

        let hg_csid = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
        let csid = repo
            .get_bonsai_from_hg(ctx.clone(), hg_csid)
            .await?
            .ok_or(Error::msg("known hg does not have bonsai csid"))?;

        let lease = repo.get_derived_data_lease_ops();
        let lease_key = Arc::new(format!(
            "repo{}.{}.{}",
            repo.get_repoid().id(),
            TestGenNum::NAME,
            csid
        ));

        // take lease
        assert_eq!(lease.try_add_put_lease(&lease_key).await?, true);
        assert_eq!(lease.try_add_put_lease(&lease_key).await?, false);

        let output = Arc::new(Mutex::new(Vec::new()));
        tokio::spawn({
            cloned!(ctx, repo, output);
            async move {
                let result = TestGenNum::derive(&ctx, &repo, csid).await;
                output.with(move |output| output.push(result));
            }
        });

        // schedule derivation
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(mapping.get(ctx.clone(), vec![csid]).await?, HashMap::new());

        // release lease
        lease.release_lease(&lease_key).await;

        tokio::time::sleep(Duration::from_millis(3000)).await;
        let result = match output.with(|output| output.pop()) {
            Some(result) => result?,
            None => panic!("scheduled derivation should have been completed"),
        };
        assert_eq!(
            mapping.get(ctx.clone(), vec![csid]).await?,
            hashmap! { csid => result.clone() }
        );

        // take lease
        assert_eq!(lease.try_add_put_lease(&lease_key).await?, true);
        // should succed as lease should not be request
        assert_eq!(TestGenNum::derive(&ctx, &repo, csid).await?, result);
        lease.release_lease(&lease_key).await;

        Ok(())
    }

    #[derive(Debug)]
    struct FailingLease;

    impl std::fmt::Display for FailingLease {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "FailingLease")
        }
    }

    #[async_trait]
    impl LeaseOps for FailingLease {
        async fn try_add_put_lease(&self, _key: &str) -> Result<bool> {
            Err(anyhow!("error"))
        }

        fn renew_lease_until(&self, _ctx: CoreContext, _key: &str, _done: BoxFuture<'static, ()>) {}

        async fn wait_for_other_leases(&self, _key: &str) {}

        async fn release_lease(&self, _key: &str) {}
    }

    #[fbinit::test]
    async fn test_always_failing_lease(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = TestRepoFactory::new()?
            .with_config_override(|repo_config| {
                repo_config
                    .derived_data_config
                    .enabled
                    .types
                    .insert(TestGenNum::NAME.to_string());
            })
            .with_derived_data_lease(|| Arc::new(FailingLease))
            .build()
            .unwrap();
        linear::initrepo(fb, &repo).await;
        let mapping = TestGenNum::default_mapping(&ctx, &repo)?;

        let hg_csid = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
        let csid = repo
            .get_bonsai_from_hg(ctx.clone(), hg_csid)
            .await?
            .ok_or(Error::msg("known hg does not have bonsai csid"))?;

        let lease = repo.get_derived_data_lease_ops();
        let lease_key = Arc::new(format!(
            "repo{}.{}.{}",
            repo.get_repoid().id(),
            TestGenNum::NAME,
            csid
        ));

        // takig lease should fail
        assert!(lease.try_add_put_lease(&lease_key).await.is_err());

        // should succeed even though lease always fails
        let result = TestGenNum::derive(&ctx, &repo, csid).await?;
        assert_eq!(
            mapping.get(ctx, vec![csid]).await?,
            hashmap! { csid => result },
        );

        Ok(())
    }

    #[test]
    fn test_should_log_slow_derivation() {
        let d10 = Duration::from_secs(10);
        let d20 = Duration::from_secs(20);

        let tunables = MononokeTunables::default();
        with_tunables(tunables, || {
            assert!(!should_log_slow_derivation(d10));
            assert!(!should_log_slow_derivation(d20));
        });

        let tunables = MononokeTunables::default();
        tunables
            .update_ints(&hashmap! {"derived_data_slow_derivation_threshold_secs".into() => -1});
        with_tunables(tunables, || {
            assert!(!should_log_slow_derivation(d10));
            assert!(!should_log_slow_derivation(d20));
        });

        let tunables = MononokeTunables::default();
        tunables.update_ints(&hashmap! {"derived_data_slow_derivation_threshold_secs".into() => 0});
        with_tunables(tunables, || {
            assert!(!should_log_slow_derivation(d10));
            assert!(!should_log_slow_derivation(d20));
        });

        let tunables = MononokeTunables::default();
        tunables
            .update_ints(&hashmap! {"derived_data_slow_derivation_threshold_secs".into() => 15});
        with_tunables(tunables, || {
            assert!(!should_log_slow_derivation(d10));
            assert!(should_log_slow_derivation(d20));
        });
    }

    #[fbinit::test]
    async fn test_parallel_derivation(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = TestRepoFactory::new()?
            .with_config_override(|repo_config| {
                repo_config
                    .derived_data_config
                    .enabled
                    .types
                    .insert(TestGenNum::NAME.to_string());
            })
            .build()?;

        // Create a commit with lots of parents, and make each derivation took 2 seconds.
        // Check that derivations happen in parallel
        let mut parents = vec![];
        for i in 0..8 {
            let p = CreateCommitContext::new_root(&ctx, &repo)
                .add_file(MPath::new(format!("file_{}", i))?, format!("{}", i))
                .commit()
                .await?;
            parents.push(p);
        }

        let merge = CreateCommitContext::new(&ctx, &repo, parents)
            .commit()
            .await?;

        let session = ctx.metadata().session_id().clone();
        DELAY.with(|m| m.insert(session, Duration::from_secs(2)));

        let (stats, _res) = TestGenNum::derive(&ctx, &repo, merge).try_timed().await?;

        println!("duration {:?}", stats.completion_time);
        assert!(stats.completion_time < Duration::from_secs(10));

        Ok(())
    }

    #[fbinit::test]
    async fn test_cancelling_slow_derivation(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = TestRepoFactory::new()?
            .with_config_override(|repo_config| {
                repo_config
                    .derived_data_config
                    .enabled
                    .types
                    .insert(TestGenNum::NAME.to_string());
            })
            .build()?;

        let session = ctx.metadata().session_id().clone();
        DELAY.with(|m| m.insert(session, Duration::from_secs(20)));

        let create_tunables = || {
            let tunables = MononokeTunables::default();
            // Make delay smaller so that the test runs faster
            tunables.update_ints(&hashmap! {
                "derived_data_disabled_watcher_delay_secs".to_string() => 1,
            });
            tunables
        };

        let commit = CreateCommitContext::new_root(&ctx, &repo)
            .add_file(MPath::new("file")?, "content")
            .commit()
            .await?;

        let tunables = create_tunables();
        override_tunables(Some(Arc::new(tunables)));

        let tunables = create_tunables();
        tunables.update_by_repo_bools(&hashmap! {
            repo.name().to_string() => hashmap! {
                "all_derived_data_disabled".to_string() => true,
            },
        });
        ensure_disabled(&ctx, &repo, commit, tunables).await?;

        // Remove previous override
        let tunables = create_tunables();
        override_tunables(Some(Arc::new(tunables)));

        dbg!("Disable derive data for a single type");
        let tunables = create_tunables();
        tunables.update_by_repo_vec_of_strings(&hashmap! {
            repo.name().to_string() => hashmap! {
                "derived_data_types_disabled".to_string() => vec![TestGenNum::NAME.to_string()],
            },
        });
        ensure_disabled(&ctx, &repo, commit, tunables).await?;

        Ok(())
    }

    async fn ensure_disabled(
        ctx: &CoreContext,
        repo: &BlobRepo,
        commit: ChangesetId,
        tunables: MononokeTunables,
    ) -> Result<(), Error> {
        let spawned_derivation = tokio::spawn({
            let ctx = ctx.clone();
            let repo = repo.clone();
            async move { TestGenNum::derive(&ctx, &repo, commit).timed().await }
        });
        tokio::time::sleep(Duration::from_millis(1000)).await;

        override_tunables(Some(Arc::new(tunables)));

        let (stats, res) = spawned_derivation.await?;

        assert!(matches!(res, Err(DeriveError::Disabled(..))));

        println!("duration {:?}", stats.completion_time);
        assert!(stats.completion_time < Duration::from_secs(15));

        Ok(())
    }
}
