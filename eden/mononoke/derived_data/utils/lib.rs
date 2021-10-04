/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{anyhow, format_err, Error};
use async_trait::async_trait;
use blame::{BlameRoot, RootBlameV2};
use blobrepo::BlobRepo;
use blobrepo_override::DangerousOverride;
use blobstore::Blobstore;
use bonsai_hg_mapping::BonsaiHgMappingArc;
use borrowed::borrowed;
use cacheblob::{dummy::DummyLease, LeaseOps, MemWritesBlobstore};
use changeset_info::ChangesetInfo;
use changesets::ChangesetsArc;
use cloned::cloned;
use context::CoreContext;
use deleted_files_manifest::RootDeletedManifestId;
use derived_data::{
    derive_impl, BonsaiDerivable, BonsaiDerivedMapping, BonsaiDerivedMappingContainer,
    DerivedDataTypesConfig, RegenerateMapping,
};
use derived_data_filenodes::{FilenodesOnlyPublic, FilenodesOnlyPublicMapping};
use derived_data_manager::{
    BatchDeriveOptions, BatchDeriveStats, BonsaiDerivable as NewBonsaiDerivable,
    DerivedDataManager, Rederivation,
};
use fastlog::RootFastlog;
use fbinit::FacebookInit;
use filenodes::FilenodesArc;
use fsnodes::RootFsnodeId;
use futures::{
    future::{self, ready, try_join_all, BoxFuture, FutureExt},
    stream::futures_unordered::FuturesUnordered,
    Future, Stream, TryFutureExt, TryStreamExt,
};
use futures_stats::TimedTryFutureExt;
use git_types::TreeHandle;
use lazy_static::lazy_static;
use lock_ext::LockExt;
use mercurial_derived_data::MappedHgChangesetId;
use metaconfig_types::BlameVersion;
use mononoke_types::ChangesetId;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use scuba_ext::MononokeScubaSampleBuilder;
use skeleton_manifest::RootSkeletonManifestId;
use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    io::Write,
    marker::PhantomData,
    sync::{Arc, Mutex},
};
use topo_sort::sort_topological;
use unodes::RootUnodeManifestId;

pub const POSSIBLE_DERIVED_TYPES: &[&str] = &[
    RootUnodeManifestId::NAME,
    RootFastlog::NAME,
    MappedHgChangesetId::NAME,
    RootFsnodeId::NAME,
    BlameRoot::NAME,
    ChangesetInfo::NAME,
    RootDeletedManifestId::NAME,
    FilenodesOnlyPublic::NAME,
    RootSkeletonManifestId::NAME,
    TreeHandle::NAME,
];

lazy_static! {
    // TODO: come up with a better way to maintain these dependencies T77090285
    pub static ref DERIVED_DATA_DEPS: HashMap<&'static str, Vec<&'static str>> = {
        let unodes = RootUnodeManifestId::NAME;
        let fastlog = RootFastlog::NAME;
        let hgchangeset = MappedHgChangesetId::NAME;
        let fsnodes = RootFsnodeId::NAME;
        let blame = BlameRoot::NAME;
        let changesets_info = ChangesetInfo::NAME;
        let deleted_mf = RootDeletedManifestId::NAME;
        let filenodes = FilenodesOnlyPublic::NAME;
        let skeleton_mf = RootSkeletonManifestId::NAME;

        let mut dag = HashMap::new();

        dag.insert(hgchangeset, vec![]);
        dag.insert(unodes, vec![]);
        dag.insert(blame, vec![unodes]);
        dag.insert(fastlog, vec![unodes]);
        dag.insert(changesets_info, vec![]);
        dag.insert(filenodes, vec![hgchangeset]);
        dag.insert(fsnodes, vec![]);
        dag.insert(deleted_mf, vec![unodes]);
        dag.insert(skeleton_mf, vec![]);

        dag
    };
    pub static ref DERIVED_DATA_ORDER: HashMap<&'static str, usize> = {
        let order = sort_topological(&*DERIVED_DATA_DEPS).expect("derived data can not form loop");
        order
            .into_iter()
            .enumerate()
            .map(|(index, name)| (name, index))
            .collect::<HashMap<_, _>>()
    };
}

pub fn derive_data_for_csids(
    ctx: &CoreContext,
    repo: &BlobRepo,
    csids: Vec<ChangesetId>,
    derived_data_types: &[String],
) -> Result<impl Future<Output = Result<(), Error>>, Error> {
    let derivations = FuturesUnordered::new();

    for data_type in derived_data_types {
        let derived_utils = derived_data_utils(ctx.fb, repo, data_type)?;

        let mut futs = vec![];
        for csid in &csids {
            let fut = derived_utils
                .derive(ctx.clone(), repo.clone(), *csid)
                .map_ok(|_| ());
            futs.push(fut);
        }

        derivations.push(async move {
            // Call functions sequentially because derived data is sequential
            // so there's no point in trying to derive it in parallel
            for f in futs {
                f.await?;
            }
            Result::<_, Error>::Ok(())
        });
    }

    Ok(async move {
        derivations.try_for_each(|_| ready(Ok(()))).await?;
        Ok(())
    })
}

#[async_trait]
pub trait DerivedUtils: Send + Sync + 'static {
    /// Derive data for changeset
    fn derive(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csid: ChangesetId,
    ) -> BoxFuture<'static, Result<String, Error>>;

    fn backfill_batch_dangerous(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csids: Vec<ChangesetId>,
        parallel: bool,
        gap_size: Option<usize>,
    ) -> BoxFuture<'static, Result<BackfillDeriveStats, Error>>;

    /// Find pending changeset (changesets for which data have not been derived)
    async fn pending(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csids: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetId>, Error>;

    /// Count how many ancestors are not derived
    async fn count_underived(
        &self,
        ctx: &CoreContext,
        repo: &BlobRepo,
        csid: ChangesetId,
    ) -> Result<u64, Error>;

    /// Regenerate derived data for specified set of commits
    fn regenerate(&self, csids: &Vec<ChangesetId>);

    /// Remove all previously set regenerations
    fn clear_regenerate(&self);

    /// Get a name for this type of derived data
    fn name(&self) -> &'static str;

    /// Find all underived ancestors of the target changeset id.
    ///
    /// Returns a map from underived commit to its underived
    /// parents, suitable for input to toposort.
    async fn find_underived<'a>(
        &'a self,
        ctx: &'a CoreContext,
        repo: &'a BlobRepo,
        csid: ChangesetId,
    ) -> Result<HashMap<ChangesetId, Vec<ChangesetId>>, Error>;
}

#[derive(Clone)]
struct DerivedUtilsFromMapping<Derivable, Mapping>
where
    Derivable: BonsaiDerivable,
    Mapping: BonsaiDerivedMapping<Value = Derivable>,
{
    orig_mapping: Arc<RegenerateMapping<Mapping>>,
    mapping: BonsaiDerivedMappingContainer<Derivable>,
}

impl<Derivable, Mapping> DerivedUtilsFromMapping<Derivable, Mapping>
where
    Derivable: BonsaiDerivable,
    Mapping: BonsaiDerivedMapping<Value = Derivable> + 'static,
{
    fn new(fb: FacebookInit, mapping: Mapping, repo: BlobRepo) -> Self {
        let orig_mapping = Arc::new(RegenerateMapping::new(mapping));
        let mapping = BonsaiDerivedMappingContainer::new(
            fb,
            repo.name(),
            repo.get_derived_data_config().scuba_table.as_deref(),
            orig_mapping.clone(),
        );
        Self {
            orig_mapping,
            mapping,
        }
    }
}

#[async_trait]
impl<Derivable, Mapping> DerivedUtils for DerivedUtilsFromMapping<Derivable, Mapping>
where
    Derivable: BonsaiDerivable + std::fmt::Debug,
    Mapping: BonsaiDerivedMapping<Value = Derivable> + 'static,
{
    fn derive(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csid: ChangesetId,
    ) -> BoxFuture<'static, Result<String, Error>> {
        // We call derive_impl directly so that we can pass
        // `self.mapping` there. This will allow us to
        // e.g. regenerate derived data for the commit
        // even if it was already generated (see RegenerateMapping call).
        cloned!(self.mapping);
        async move {
            let result = derive_impl::derive_impl::<Derivable>(&ctx, &repo, &mapping, csid).await?;
            Ok(format!("{:?}", result))
        }
        .boxed()
    }

    /// !!!!This function is dangerous and should be used with care!!!!
    /// In particular it might corrupt the data if it tries to derive data that
    /// depends on another derived data (e.g. blame depends on unodes) and both
    /// of them are not derived.
    /// For example, if unodes and blame are both underived and we are trying
    /// to derive blame then unodes mapping might be inserted in the blobstore
    /// before all unodes were derived.
    ///
    /// This function should be safe to use only if derived data doesn't depend
    /// on another derived data (e.g. unodes) or if this dependency is already derived
    /// (e.g. deriving blame when unodes are already derived).
    fn backfill_batch_dangerous(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csids: Vec<ChangesetId>,
        parallel: bool,
        gap_size: Option<usize>,
    ) -> BoxFuture<'static, Result<BackfillDeriveStats, Error>> {
        // With InMemoryMapping we can ensure that mapping entries are written only after
        // all corresponding blobs were successfully saved
        let in_memory_mapping_inner = Arc::new(InMemoryMapping::new(
            self.orig_mapping.clone(),
            repo.clone(),
        ));
        let in_memory_mapping = BonsaiDerivedMappingContainer::new(
            ctx.fb,
            repo.name(),
            None,
            in_memory_mapping_inner.clone(),
        );
        let mapping = self.mapping.clone();

        // Use `MemWritesBlobstore` to avoid blocking on writes to underlying blobstore.
        // `::persist` is later used to bulk write all pending data.
        let mut memblobstore = None;
        let repo = repo
            .dangerous_override(|_| Arc::new(DummyLease {}) as Arc<dyn LeaseOps>)
            .dangerous_override(|blobstore| -> Arc<dyn Blobstore> {
                let blobstore = Arc::new(MemWritesBlobstore::new(blobstore));
                memblobstore = Some(blobstore.clone());
                blobstore
            });
        let memblobstore = memblobstore.expect("memblobstore should have been updated");

        let mut scuba = create_derive_graph_scuba_sample(&ctx, &csids, self.name());
        async move {
            // create new context so each derivation has its own trace, but same QPS limits
            let ctx = ctx.session().new_context(
                ctx.logger().clone(),
                MononokeScubaSampleBuilder::with_discard(),
            );

            let res = if parallel || gap_size.is_some() {
                // derive the batch of derived data in parallel
                let (stats, _) =
                    Derivable::batch_derive(&ctx, &repo, csids, &in_memory_mapping, gap_size)
                        .try_timed()
                        .await?;
                BackfillDeriveStats::Parallel(stats.completion_time)
            } else {
                let mut per_commit_stats = vec![];
                for csid in csids {
                    // derive each changeset sequentially
                    let (stats, _) = derive_impl::derive_impl::<Derivable>(
                        &ctx,
                        &repo,
                        &in_memory_mapping,
                        csid,
                    )
                    .try_timed()
                    .await?;
                    per_commit_stats.push((csid, stats.completion_time));
                }
                BackfillDeriveStats::Serial(per_commit_stats)
            };

            {
                let ctx = derived_data::override_ctx(ctx.clone(), &repo);
                // flush blobstore
                let (stats, _) = memblobstore.persist(&ctx).try_timed().await?;
                scuba
                    .add_future_stats(&stats)
                    .log_with_msg("Flushed derived blobs", None);
            }

            // flush mapping
            let futs = FuturesUnordered::new();
            {
                let buffer = in_memory_mapping_inner.clone_buffer();
                let buffer = buffer.lock().unwrap();
                for (cs_id, value) in buffer.iter() {
                    futs.push({
                        cloned!(value, cs_id);
                        borrowed!(ctx, mapping);
                        async move { mapping.put(ctx, cs_id, &value).await }
                    });
                }
            }
            let (stats, _) = futs.try_for_each(|_| future::ok(())).try_timed().await?;
            scuba
                .add_future_stats(&stats)
                .log_with_msg("Flushed mapping", None);
            Ok(res)
        }
        .boxed()
    }

    async fn pending(
        &self,
        ctx: CoreContext,
        _repo: BlobRepo,
        mut csids: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetId>, Error> {
        let derived = self.mapping.get(&ctx, csids.clone()).await?;
        csids.retain(|csid| !derived.contains_key(&csid));
        Ok(csids)
    }

    async fn count_underived(
        &self,
        ctx: &CoreContext,
        repo: &BlobRepo,
        csid: ChangesetId,
    ) -> Result<u64, Error> {
        let underived = derive_impl::find_topo_sorted_underived::<Derivable, _>(
            ctx,
            repo,
            &self.mapping,
            Some(csid),
            None, // No limit
        )
        .await?;
        Ok(underived.len() as u64)
    }

    async fn find_underived<'a>(
        &'a self,
        ctx: &'a CoreContext,
        repo: &'a BlobRepo,
        csid: ChangesetId,
    ) -> Result<HashMap<ChangesetId, Vec<ChangesetId>>, Error> {
        derive_impl::find_underived(ctx, repo, &self.mapping, Some(csid), None).await
    }

    fn regenerate(&self, csids: &Vec<ChangesetId>) {
        self.orig_mapping.regenerate(csids.iter().copied())
    }

    fn clear_regenerate(&self) {
        self.orig_mapping.clear_regenerate()
    }

    fn name(&self) -> &'static str {
        Derivable::NAME
    }
}

pub type BackfillDeriveStats = BatchDeriveStats;

#[derive(Clone)]
struct InMemoryMapping<M: BonsaiDerivedMapping + Clone> {
    mapping: M,
    buffer: Arc<Mutex<HashMap<ChangesetId, M::Value>>>,
    repo: BlobRepo,
}

impl<M> InMemoryMapping<M>
where
    M: BonsaiDerivedMapping + Clone,
    <M as BonsaiDerivedMapping>::Value: Clone,
{
    fn new(mapping: M, repo: BlobRepo) -> Self {
        Self {
            mapping,
            buffer: Arc::new(Mutex::new(HashMap::new())),
            repo,
        }
    }

    fn clone_buffer(&self) -> Arc<Mutex<HashMap<ChangesetId, M::Value>>> {
        self.buffer.clone()
    }
}

#[async_trait]
impl<M> BonsaiDerivedMapping for InMemoryMapping<M>
where
    M: BonsaiDerivedMapping + Clone,
    <M as BonsaiDerivedMapping>::Value: Clone,
{
    type Value = M::Value;

    async fn get(
        &self,
        ctx: &CoreContext,
        mut csids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Self::Value>, Error> {
        let mut ans = HashMap::new();
        {
            let buffer = self.buffer.lock().unwrap();
            csids.retain(|cs_id| {
                if let Some(v) = buffer.get(cs_id) {
                    ans.insert(*cs_id, v.clone());
                    false
                } else {
                    true
                }
            });
        }

        let fetched = self.mapping.get(ctx, csids).await?;
        Ok(ans.into_iter().chain(fetched.into_iter()).collect())
    }

    async fn put(
        &self,
        _ctx: &CoreContext,
        csid: ChangesetId,
        id: &Self::Value,
    ) -> Result<(), Error> {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.insert(csid, id.clone());
        Ok(())
    }

    fn options(&self) -> <M::Value as BonsaiDerivable>::Options {
        self.mapping.options()
    }
}

#[derive(Clone)]
struct DerivedUtilsFromManager<Derivable> {
    manager: DerivedDataManager,
    rederive: Arc<Mutex<HashSet<ChangesetId>>>,
    phantom: PhantomData<Derivable>,
}

impl<Derivable> DerivedUtilsFromManager<Derivable> {
    fn new(repo: &BlobRepo, config: &DerivedDataTypesConfig) -> Self {
        let lease = repo.repo_derived_data().lease().clone();
        let scuba = repo.repo_derived_data().manager().scuba().clone();
        let manager = DerivedDataManager::new(
            repo.get_repoid(),
            repo.name().clone(),
            repo.changesets_arc(),
            repo.bonsai_hg_mapping_arc(),
            repo.filenodes_arc(),
            repo.repo_blobstore().clone(),
            lease,
            scuba,
            config.clone(),
        );
        Self {
            manager,
            rederive: Default::default(),
            phantom: PhantomData,
        }
    }
}

impl<Derivable> Rederivation for DerivedUtilsFromManager<Derivable>
where
    Derivable: NewBonsaiDerivable,
{
    fn needs_rederive(&self, derivable_name: &str, csid: ChangesetId) -> Option<bool> {
        if derivable_name == Derivable::NAME {
            if self.rederive.with(|rederive| rederive.contains(&csid)) {
                return Some(true);
            }
        }
        None
    }

    fn mark_derived(&self, derivable_name: &str, csid: ChangesetId) {
        if derivable_name == Derivable::NAME {
            self.rederive.with(|rederive| rederive.remove(&csid));
        }
    }
}

#[async_trait]
impl<Derivable> DerivedUtils for DerivedUtilsFromManager<Derivable>
where
    Derivable: NewBonsaiDerivable + std::fmt::Debug,
{
    fn derive(
        &self,
        ctx: CoreContext,
        _repo: BlobRepo,
        csid: ChangesetId,
    ) -> BoxFuture<'static, Result<String, Error>> {
        let utils = Arc::new(self.clone());
        async move {
            let derived = utils
                .manager
                .derive::<Derivable>(&ctx, csid, Some(utils.clone()))
                .await?;
            Ok(format!("{:?}", derived))
        }
        .boxed()
    }

    fn backfill_batch_dangerous(
        &self,
        ctx: CoreContext,
        _repo: BlobRepo,
        csids: Vec<ChangesetId>,
        parallel: bool,
        gap_size: Option<usize>,
    ) -> BoxFuture<'static, Result<BackfillDeriveStats, Error>> {
        let options = if parallel || gap_size.is_some() {
            BatchDeriveOptions::Parallel { gap_size }
        } else {
            BatchDeriveOptions::Serial
        };
        let utils = Arc::new(self.clone());
        async move {
            let stats = utils
                .manager
                .backfill_batch::<Derivable>(&ctx, csids, options, Some(utils.clone()))
                .await?;
            Ok(stats)
        }
        .boxed()
    }

    async fn pending(
        &self,
        ctx: CoreContext,
        _repo: BlobRepo,
        mut csids: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetId>, Error> {
        let utils = Arc::new(self.clone());
        let derived = self
            .manager
            .fetch_derived_batch::<Derivable>(&ctx, csids.clone(), Some(utils))
            .await?;
        csids.retain(|csid| !derived.contains_key(&csid));
        Ok(csids)
    }

    async fn count_underived(
        &self,
        ctx: &CoreContext,
        _repo: &BlobRepo,
        csid: ChangesetId,
    ) -> Result<u64, Error> {
        let utils = Arc::new(self.clone());
        Ok(self
            .manager
            .count_underived::<Derivable>(ctx, csid, None, Some(utils))
            .await?)
    }

    fn regenerate(&self, csids: &Vec<ChangesetId>) {
        self.rederive
            .with(|rederive| rederive.extend(csids.iter().copied()));
    }

    fn clear_regenerate(&self) {
        self.rederive.with(|rederive| rederive.clear());
    }

    fn name(&self) -> &'static str {
        Derivable::NAME
    }

    async fn find_underived<'a>(
        &'a self,
        ctx: &'a CoreContext,
        _repo: &'a BlobRepo,
        csid: ChangesetId,
    ) -> Result<HashMap<ChangesetId, Vec<ChangesetId>>, Error> {
        let utils = Arc::new(self.clone());
        self.manager
            .find_underived::<Derivable>(ctx, csid, None, Some(utils))
            .await
    }
}

pub fn derived_data_utils(
    fb: FacebookInit,
    repo: &BlobRepo,
    name: impl AsRef<str>,
) -> Result<Arc<dyn DerivedUtils>, Error> {
    let name = name.as_ref();
    let config = repo.get_derived_data_config();
    let types_config = if config.enabled.types.contains(name) {
        &config.enabled
    } else {
        return Err(anyhow!("Derived data type {} is not configured", name));
    };
    derived_data_utils_impl(fb, repo, name, types_config)
}

pub fn derived_data_utils_for_backfill(
    fb: FacebookInit,
    repo: &BlobRepo,
    name: impl AsRef<str>,
) -> Result<Arc<dyn DerivedUtils>, Error> {
    let name = name.as_ref();
    let config = repo.get_derived_data_config();
    let types_config = if config.backfilling.types.contains(name) {
        &config.backfilling
    } else if config.enabled.types.contains(name) {
        &config.enabled
    } else {
        return Err(anyhow!(
            "Derived data type {} is not configured for backfilling",
            name
        ));
    };
    derived_data_utils_impl(fb, repo, name, types_config)
}

fn derived_data_utils_impl(
    fb: FacebookInit,
    repo: &BlobRepo,
    name: &str,
    config: &DerivedDataTypesConfig,
) -> Result<Arc<dyn DerivedUtils>, Error> {
    match name {
        RootUnodeManifestId::NAME => Ok(Arc::new(
            DerivedUtilsFromManager::<RootUnodeManifestId>::new(repo, config),
        )),
        RootFastlog::NAME => Ok(Arc::new(DerivedUtilsFromManager::<RootFastlog>::new(
            repo, config,
        ))),
        MappedHgChangesetId::NAME => Ok(Arc::new(
            DerivedUtilsFromManager::<MappedHgChangesetId>::new(repo, config),
        )),
        RootFsnodeId::NAME => Ok(Arc::new(DerivedUtilsFromManager::<RootFsnodeId>::new(
            repo, config,
        ))),
        BlameRoot::NAME => match config.blame_version {
            BlameVersion::V1 => Ok(Arc::new(DerivedUtilsFromManager::<BlameRoot>::new(
                repo, config,
            ))),
            BlameVersion::V2 => Ok(Arc::new(DerivedUtilsFromManager::<RootBlameV2>::new(
                repo, config,
            ))),
        },
        ChangesetInfo::NAME => Ok(Arc::new(DerivedUtilsFromManager::<ChangesetInfo>::new(
            repo, config,
        ))),
        RootDeletedManifestId::NAME => Ok(Arc::new(
            DerivedUtilsFromManager::<RootDeletedManifestId>::new(repo, config),
        )),
        FilenodesOnlyPublic::NAME => {
            let mapping = FilenodesOnlyPublicMapping::new(repo, config)?;
            Ok(Arc::new(DerivedUtilsFromMapping::new(
                fb,
                mapping,
                repo.clone(),
            )))
        }
        RootSkeletonManifestId::NAME => Ok(Arc::new(DerivedUtilsFromManager::<
            RootSkeletonManifestId,
        >::new(repo, config))),
        TreeHandle::NAME => Ok(Arc::new(DerivedUtilsFromManager::<TreeHandle>::new(
            repo, config,
        ))),
        name => Err(format_err!("Unsupported derived data type: {}", name)),
    }
}

pub struct DeriveGraphInner {
    pub id: usize,
    // deriver can be None only for the root element, and csids for this element is empty.
    pub deriver: Option<Arc<dyn DerivedUtils>>,
    pub csids: Vec<ChangesetId>,
    pub dependencies: Vec<DeriveGraph>,
}

#[derive(Clone)]
pub struct DeriveGraph {
    inner: Arc<DeriveGraphInner>,
}

impl DeriveGraph {
    fn new(
        id: usize,
        deriver: Arc<dyn DerivedUtils>,
        csids: Vec<ChangesetId>,
        dependencies: Vec<DeriveGraph>,
    ) -> Self {
        let inner = DeriveGraphInner {
            id,
            deriver: Some(deriver),
            csids,
            dependencies,
        };
        Self {
            inner: Arc::new(inner),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.size() == 0
    }

    /// Number of derivation to be carried out by this graph
    pub fn size(&self) -> usize {
        let mut stack = vec![self];
        let mut visited = HashSet::new();
        let mut count = 0;
        while let Some(node) = stack.pop() {
            count += node.csids.len();
            for dep in node.dependencies.iter() {
                if visited.insert(dep.id) {
                    stack.push(dep);
                }
            }
        }
        count
    }

    /// Find all commits that will be derived
    pub fn commits(&self) -> HashSet<ChangesetId> {
        let mut stack = vec![self];
        let mut visited = HashSet::new();
        let mut res = HashSet::new();
        while let Some(node) = stack.pop() {
            res.extend(node.csids.iter().map(|cs_id| *cs_id));
            for dep in node.dependencies.iter() {
                if visited.insert(dep.id) {
                    stack.push(dep);
                }
            }
        }
        res
    }

    /// Derive all data in the graph
    pub async fn derive(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        parallel: bool,
        gap_size: Option<usize>,
    ) -> Result<(), Error> {
        bounded_traversal::bounded_traversal_dag(
            100,
            self.clone(),
            |node| {
                async move {
                    let deps = node.dependencies.clone();
                    Ok((node, deps))
                }
                .boxed()
            },
            move |node, _| {
                cloned!(ctx, repo);
                async move {
                    if let Some(deriver) = &node.deriver {
                        let job = deriver
                            .backfill_batch_dangerous(
                                ctx.clone(),
                                repo,
                                node.csids.clone(),
                                parallel,
                                gap_size,
                            )
                            .try_timed();
                        let (stats, _) = tokio::spawn(job).await??;
                        if let (Some(first), Some(last)) = (node.csids.first(), node.csids.last()) {
                            slog::debug!(
                                ctx.logger(),
                                "[{}:{}] count:{} start:{} end:{}",
                                deriver.name(),
                                node.id,
                                node.csids.len(),
                                first,
                                last
                            );
                            let mut scuba =
                                create_derive_graph_scuba_sample(&ctx, &node.csids, deriver.name());
                            scuba
                                .add_future_stats(&stats)
                                .log_with_msg("Derived stack", None);
                        }
                    }
                    Ok::<_, Error>(())
                }
                .boxed()
            },
        )
        .await?
        .ok_or_else(|| anyhow!("derive graph contains a cycle"))
    }

    // render derive graph as digraph that can be rendered with graphviz
    // for debugging purposes.
    // $ dot -Tpng <outout> -o <image>
    pub fn digraph(&self, w: &mut dyn Write) -> std::io::Result<()> {
        writeln!(w, "digraph DeriveGraph {{")?;

        let mut stack = vec![self];
        let mut visited = HashSet::new();
        while let Some(node) = stack.pop() {
            let deriver = node.deriver.as_ref().map_or_else(|| "root", |d| d.name());
            writeln!(
                w,
                " {0} [label=\"[{1}] id:{0} csids:{2}\"]",
                node.id,
                deriver,
                node.csids.len()
            )?;
            for dep in node.dependencies.iter() {
                writeln!(w, " {} -> {}", node.id, dep.id)?;
                if visited.insert(dep.id) {
                    stack.push(dep);
                }
            }
        }

        writeln!(w, "}}")?;
        Ok(())
    }
}

pub fn create_derive_graph_scuba_sample(
    ctx: &CoreContext,
    nodes: &[ChangesetId],
    derived_data_name: &str,
) -> MononokeScubaSampleBuilder {
    let mut scuba_sample = ctx.scuba().clone();
    scuba_sample
        .add("stack_size", nodes.len())
        .add("derived_data", derived_data_name);
    if let (Some(first), Some(last)) = (nodes.first(), nodes.last()) {
        scuba_sample
            .add("first_csid", format!("{}", first))
            .add("last_csid", format!("{}", last));
    }
    scuba_sample
}

impl std::ops::Deref for DeriveGraph {
    type Target = DeriveGraphInner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl PartialEq for DeriveGraph {
    fn eq(&self, other: &Self) -> bool {
        self.inner.id == other.inner.id
    }
}

impl Eq for DeriveGraph {}

impl Hash for DeriveGraph {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.inner.id.hash(state)
    }
}

/// Generate batched derivation graph
///
/// This function generates graph of changeset batches with assocated `DerivedUtil` that
/// accounts for dependencies between changesets and derived data types.
///
/// NOTE: This function might take very long time to run and consume a lot of memory,
///       if there are a lot of underived changesets is present. Avoid direct use of this
///       function in mononoke.
pub async fn build_derive_graph(
    ctx: &CoreContext,
    repo: &BlobRepo,
    csids: Vec<ChangesetId>,
    mut derivers: Vec<Arc<dyn DerivedUtils>>,
    batch_size: usize,
    thin_out: ThinOut,
) -> Result<DeriveGraph, Error> {
    // resolve derived data types dependencies, and require derivers for all dependencies
    // to be provided.
    derivers.sort_by_key(|d| DERIVED_DATA_ORDER.get(d.name()));
    let deriver_to_index: HashMap<_, _> = derivers
        .iter()
        .enumerate()
        .map(|(i, d)| (d.name(), i))
        .collect();
    let mut derivers_dependiencies = Vec::new();
    derivers_dependiencies.resize_with(derivers.len(), Vec::new);
    for (index, deriver) in derivers.iter().enumerate() {
        let dep_names = DERIVED_DATA_DEPS
            .get(deriver.name())
            .ok_or_else(|| anyhow!("unknown derived data type: {}", deriver.name()))?;
        for dep_name in dep_names {
            let dep_index = deriver_to_index.get(dep_name).ok_or_else(|| {
                anyhow!(
                    "{0} depends on {1}, but deriver for {1} is not provided",
                    deriver.name(),
                    dep_name,
                )
            })?;
            derivers_dependiencies[index].push(dep_index);
        }
    }

    // find underived changesets
    let mut underived_to_derivers = HashMap::new();
    let mut underived_dag = HashMap::new();
    let mut underived_stream =
        find_underived_many(ctx.clone(), repo.clone(), csids, derivers.clone(), thin_out);
    let mut found_changesets = 0usize;
    let start = std::time::Instant::now();
    while let Some((csid, parents, derivers)) = underived_stream.try_next().await? {
        underived_dag.insert(csid, parents);
        underived_to_derivers.insert(csid, derivers);
        found_changesets += 1;
        if found_changesets % 1000 == 0 {
            slog::debug!(
                ctx.logger(),
                "found changesets: {} {:.3}/s",
                found_changesets,
                found_changesets as f32 / start.elapsed().as_secs_f32(),
            );
        }
    }
    if found_changesets > 0 {
        slog::debug!(
            ctx.logger(),
            "found changesets: {} {:.3}/s",
            found_changesets,
            found_changesets as f32 / start.elapsed().as_secs_f32(),
        );
    }

    // topologically sort changeset
    let underived_ordered = sort_topological(&underived_dag).expect("commit graph has cycles!");
    let underived_ordered: Vec<_> = underived_ordered
        .into_iter()
        // Note - sort_topological returns all nodes including commits which were already
        // derived i.e. sort_topological({"a" -> ["b"]}) return ("a", "b").
        // The '.filter()' below removes ["b"]
        .filter(|csid| underived_dag.contains_key(csid))
        .collect();

    // build derive graph
    // `nodes` keeps a list of most recent nodes for each derived data type.
    // It is important to note that derived data types are sorted in topolotical
    // order (each type can only depend on types with small index in this list),
    // it is used during construction of the node for a given chunk of changesets
    // to create a dependency between different types.
    let mut nodes: Vec<Option<DeriveGraph>> = Vec::new();
    nodes.resize_with(derivers.len(), || None);
    let mut node_ids = 0;
    for csids in underived_ordered.chunks(batch_size) {
        // group csids by derivers
        let mut csids_by_deriver = Vec::new();
        csids_by_deriver.resize_with(derivers.len(), Vec::new);
        for csid in csids {
            match underived_to_derivers.get(csid) {
                None => continue,
                Some(csid_derivers) => {
                    for deriver in csid_derivers.iter() {
                        let index = deriver_to_index[deriver.name()];
                        csids_by_deriver[index].push(*csid);
                    }
                }
            }
        }

        // generate node per deriver
        for (index, csids) in csids_by_deriver.into_iter().enumerate() {
            if csids.is_empty() {
                continue;
            }
            let mut dependencies = Vec::new();
            // add dependency on the previous chunk associated with the same
            // derived data type.
            dependencies.extend(nodes[index].clone());
            // add dependencies on derived types which are required for derivation
            // of the given type. `dep_index` is always less then `index` since
            // derived data types are topologically sorted.
            for dep_index in derivers_dependiencies[index].iter() {
                dependencies.extend(nodes[**dep_index].clone());
            }
            // update node associated with current type of derived data
            let node = DeriveGraph::new(node_ids, derivers[index].clone(), csids, dependencies);
            node_ids += 1;
            nodes[index] = Some(node);
        }
    }

    let root = DeriveGraphInner {
        id: node_ids,
        deriver: None,
        csids: vec![],
        dependencies: nodes.into_iter().flatten().collect(),
    };
    Ok(DeriveGraph {
        inner: Arc::new(root),
    })
}

// This structure is used to thin out some actions. Intially it allows all of them
// until threshold is reached, then 1/step is allowed until threshold elemnts are
// collected, then 1/step^2 is allowed ...
#[derive(Clone, Copy)]
pub struct ThinOut {
    counter: f64,
    step: f64,
    threshold: f64,
    multiplier: f64,
}

impl ThinOut {
    pub fn new(threshold: f64, multiplier: f64) -> Self {
        Self {
            counter: 0.0,
            step: 1.0,
            threshold,
            multiplier,
        }
    }

    pub fn new_keep_all() -> Self {
        Self::new(1.0, 1.0)
    }

    pub fn check_and_update(&mut self) -> bool {
        self.counter += 1.0;
        if self.counter > self.step * self.threshold {
            self.step *= self.multiplier;
            self.counter = 1.0;
        }
        self.counter.rem_euclid(self.step) < 1.0
    }
}

/// Find underived ancestors for many derived data type at once.
///
/// This function finds all underived ancestors for derived data types specified by `derivers`
/// reachable for provided set of changesets `csids`. It also accepts `thin_out` argument that
/// can reduce number of lookups to derived data mapping at the expense of overfetching changesets,
/// that is it will return some changesets for which we might already have all derived data.
///
/// Returns a `Stream` of (in no particular order)
///   - underived changeset id
///   - parents of this changeset
///   - derivers that should be used on this changeset
pub fn find_underived_many(
    ctx: CoreContext,
    repo: BlobRepo,
    csids: Vec<ChangesetId>,
    derivers: Vec<Arc<dyn DerivedUtils>>,
    thin_out: ThinOut,
) -> impl Stream<
    Item = Result<
        (
            ChangesetId,
            Vec<ChangesetId>,
            Arc<Vec<Arc<dyn DerivedUtils>>>,
        ),
        Error,
    >,
> {
    let derivers = Arc::new(derivers);
    let init: Vec<_> = csids
        .into_iter()
        .map(|csid| (csid, derivers.clone(), thin_out))
        .collect();

    let changeset_fetcher = repo.get_changeset_fetcher();
    let visited = Arc::new(Mutex::new(HashSet::new()));
    bounded_traversal::bounded_traversal_stream(100, init, {
        move |(csid, derivers, mut thin_out)| {
            cloned!(changeset_fetcher, visited, repo, ctx);
            async move {
                let derivers = if thin_out.check_and_update() {
                    let repo = &repo;
                    let ctx = &ctx;
                    // exclude derivers not applicable (already derived) to this changeset id
                    let derivers = derivers.iter().map(|deriver| async move {
                        if deriver
                            .pending(ctx.clone(), repo.clone(), vec![csid])
                            .await?
                            .is_empty()
                        {
                            Ok::<_, Error>(None)
                        } else {
                            Ok(Some(deriver.clone()))
                        }
                    });
                    let derivers = try_join_all(derivers)
                        .await?
                        .into_iter()
                        .filter_map(|v| v)
                        .collect::<Vec<_>>();
                    Arc::new(derivers)
                } else {
                    // based on thin-out, we are not actually checking that we need to derive
                    // data for this changeset, and instead doing unconditional unfold.
                    derivers
                };
                if derivers.is_empty() {
                    // all derived data has already been derived
                    Ok::<_, Error>((None, Vec::new()))
                } else {
                    let parents = changeset_fetcher.get_parents(ctx.clone(), csid).await?;
                    let dependencies: Vec<_> = parents
                        .iter()
                        .copied()
                        .filter_map(|p| {
                            if visited.with(|vs| vs.insert(p)) {
                                Some((p, derivers.clone(), thin_out))
                            } else {
                                None
                            }
                        })
                        .collect();
                    Ok((Some((csid, parents, derivers)), dependencies))
                }
            }
            .boxed()
        }
    })
    .try_filter_map(future::ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bookmarks::BookmarkName;
    use derived_data::BonsaiDerived;
    use fbinit::FacebookInit;
    use fixtures::merge_even;
    use maplit::{btreemap, hashset};
    use metaconfig_types::UnodeVersion;
    use std::{
        collections::BTreeMap,
        sync::atomic::{AtomicUsize, Ordering},
    };
    use tests_utils::drawdag::create_from_dag;

    // decompose graph into map between node indices and list of nodes
    fn derive_graph_unpack(node: &DeriveGraph) -> (BTreeMap<usize, Vec<usize>>, Vec<DeriveGraph>) {
        let mut graph = BTreeMap::new();
        let mut nodes = Vec::new();
        let mut stack = vec![node];
        while let Some(node) = stack.pop() {
            let mut deps = Vec::new();
            for dep in node.dependencies.iter() {
                deps.push(dep.id);
                if !graph.contains_key(&dep.id) {
                    stack.push(dep);
                }
            }
            graph.insert(node.id, deps);
            nodes.push(node.clone());
        }
        nodes.sort_by_key(|n| n.id);
        (graph, nodes)
    }

    #[fbinit::test]
    async fn test_build_derive_graph(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = merge_even::getrepo(fb).await;
        let thin_out = ThinOut::new_keep_all();
        let master = repo
            .get_bonsai_bookmark(ctx.clone(), &BookmarkName::new("master").unwrap())
            .await?
            .unwrap();
        let blame_deriver = derived_data_utils(ctx.fb, &repo, "blame")?;
        let unodes_deriver = derived_data_utils(ctx.fb, &repo, "unodes")?;

        // make sure we require all dependencies, blame depens on unodes
        let graph = build_derive_graph(
            &ctx,
            &repo,
            vec![master],
            vec![blame_deriver.clone()],
            2,
            thin_out,
        )
        .await;
        assert!(graph.is_err());

        let graph = build_derive_graph(
            &ctx,
            &repo,
            vec![master],
            vec![blame_deriver.clone(), unodes_deriver.clone()],
            2,
            thin_out,
        )
        .await?;
        assert_eq!(graph.size(), 16);
        let (graph_ids, nodes) = derive_graph_unpack(&graph);
        assert_eq!(
            graph_ids,
            btreemap! {
                0 => vec![],
                1 => vec![0],
                2 => vec![0],
                3 => vec![1, 2],
                4 => vec![2],
                5 => vec![3, 4],
                6 => vec![4],
                7 => vec![5, 6],
                8 => vec![6, 7],
            }
        );
        assert_eq!(nodes[0].deriver.as_ref().unwrap().name(), "unodes");
        assert_eq!(nodes[1].deriver.as_ref().unwrap().name(), "blame");
        assert_eq!(nodes[2].deriver.as_ref().unwrap().name(), "unodes");

        let graph = build_derive_graph(
            &ctx,
            &repo,
            vec![master],
            vec![blame_deriver.clone(), unodes_deriver.clone()],
            2,
            thin_out,
        )
        .await?;
        graph.derive(ctx.clone(), repo.clone(), false, None).await?;

        let graph = build_derive_graph(
            &ctx,
            &repo,
            vec![master],
            vec![blame_deriver, unodes_deriver],
            2,
            thin_out,
        )
        .await?;
        assert!(graph.is_empty());

        Ok::<_, Error>(())
    }

    #[test]
    fn test_thin_out() {
        let mut thin_out = ThinOut::new(3.0, 2.0);
        let result: String = (0..40)
            .map(|_| {
                if thin_out.check_and_update() {
                    "X"
                } else {
                    "_"
                }
            })
            .collect();
        assert_eq!("XXX_X_X_X___X___X___X_______X_______X___", result);

        let mut thin_out = ThinOut::new_keep_all();
        assert_eq!(40, (0..40).filter(|_| thin_out.check_and_update()).count());

        let mut thin_out = ThinOut::new(1000.0, 1.5);
        let count: usize = (0..10_000_000)
            .map(|_| if thin_out.check_and_update() { 1 } else { 0 })
            .sum();
        assert_eq!(count, 20988);
    }

    struct CountedDerivedUtils {
        deriver: Arc<dyn DerivedUtils>,
        count: Arc<AtomicUsize>,
    }

    impl CountedDerivedUtils {
        fn new(deriver: Arc<dyn DerivedUtils>) -> Self {
            Self {
                deriver,
                count: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn count(&self) -> usize {
            self.count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl DerivedUtils for CountedDerivedUtils {
        fn derive(
            &self,
            ctx: CoreContext,
            repo: BlobRepo,
            csid: ChangesetId,
        ) -> BoxFuture<'static, Result<String, Error>> {
            self.deriver.derive(ctx, repo, csid)
        }

        fn backfill_batch_dangerous(
            &self,
            ctx: CoreContext,
            repo: BlobRepo,
            csids: Vec<ChangesetId>,
            parallel: bool,
            gap_size: Option<usize>,
        ) -> BoxFuture<'static, Result<BackfillDeriveStats, Error>> {
            self.deriver
                .backfill_batch_dangerous(ctx, repo, csids, parallel, gap_size)
        }

        async fn pending(
            &self,
            ctx: CoreContext,
            repo: BlobRepo,
            csids: Vec<ChangesetId>,
        ) -> Result<Vec<ChangesetId>, Error> {
            self.count.fetch_add(1, Ordering::SeqCst);
            self.deriver.pending(ctx, repo, csids).await
        }

        async fn count_underived(
            &self,
            _ctx: &CoreContext,
            _repo: &BlobRepo,
            _csid: ChangesetId,
        ) -> Result<u64, Error> {
            unimplemented!()
        }

        fn regenerate(&self, _csids: &Vec<ChangesetId>) {
            unimplemented!()
        }

        fn clear_regenerate(&self) {
            unimplemented!()
        }

        fn name(&self) -> &'static str {
            self.deriver.name()
        }

        async fn find_underived<'a>(
            &'a self,
            _ctx: &'a CoreContext,
            _repo: &'a BlobRepo,
            _csid: ChangesetId,
        ) -> Result<HashMap<ChangesetId, Vec<ChangesetId>>, Error> {
            unimplemented!()
        }
    }

    #[fbinit::test]
    async fn test_find_underived_many(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty().unwrap();
        let dag = create_from_dag(&ctx, &repo, "A-B-C").await?;
        let a = *dag.get("A").unwrap();
        let b = *dag.get("B").unwrap();
        let c = *dag.get("C").unwrap();

        let thin_out = ThinOut::new_keep_all();
        let blame_deriver = derived_data_utils(ctx.fb, &repo, "blame")?;
        let unodes_deriver = {
            let deriver = derived_data_utils(ctx.fb, &repo, "unodes")?;
            Arc::new(CountedDerivedUtils::new(deriver))
        };

        let entries: BTreeMap<_, _> = find_underived_many(
            ctx.clone(),
            repo.clone(),
            vec![c],
            vec![blame_deriver.clone(), unodes_deriver.clone()],
            thin_out,
        )
        .map_ok(|(csid, _parents, derivers)| {
            let names: Vec<_> = derivers.iter().map(|d| d.name()).collect();
            (csid, names)
        })
        .try_collect()
        .await?;
        assert_eq!(unodes_deriver.count(), 3);
        assert_eq!(
            entries,
            btreemap! {
                a => vec!["blame", "unodes"],
                b => vec!["blame", "unodes"],
                c => vec!["blame", "unodes"],
            }
        );

        unodes_deriver.derive(ctx.clone(), repo.clone(), b).await?;
        blame_deriver.derive(ctx.clone(), repo.clone(), a).await?;

        let entries: BTreeMap<_, _> = find_underived_many(
            ctx.clone(),
            repo.clone(),
            vec![c],
            vec![blame_deriver.clone(), unodes_deriver.clone()],
            thin_out,
        )
        .map_ok(|(csid, _parents, derivers)| {
            let names: Vec<_> = derivers.iter().map(|d| d.name()).collect();
            (csid, names)
        })
        .try_collect()
        .await?;
        assert_eq!(unodes_deriver.count(), 5);
        assert_eq!(
            entries,
            btreemap! {
                b => vec!["blame"],
                c => vec!["blame", "unodes"],
            }
        );

        Ok::<_, Error>(())
    }

    #[fbinit::test]
    async fn multiple_independent_mappings(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty().unwrap();
        let dag = create_from_dag(&ctx, &repo, "A-B-C").await?;
        let a = *dag.get("A").unwrap();
        let b = *dag.get("B").unwrap();
        let c = *dag.get("C").unwrap();

        // Create utils for both versions of unodes.
        let utils_v1 = derived_data_utils_impl(
            fb,
            &repo,
            "unodes",
            &DerivedDataTypesConfig {
                types: hashset! { String::from("unodes") },
                unode_version: UnodeVersion::V1,
                ..Default::default()
            },
        )?;

        let utils_v2 = derived_data_utils_impl(
            fb,
            &repo,
            "unodes",
            &DerivedDataTypesConfig {
                types: hashset! { String::from("unodes") },
                unode_version: UnodeVersion::V2,
                ..Default::default()
            },
        )?;

        assert_eq!(
            utils_v1
                .pending(ctx.clone(), repo.clone(), vec![a, b, c])
                .await?,
            vec![a, b, c]
        );
        assert_eq!(
            utils_v2
                .pending(ctx.clone(), repo.clone(), vec![a, b, c])
                .await?,
            vec![a, b, c]
        );

        // Derive V1 of A using the V1 utils.  V2 of A should still be underived.
        utils_v1.derive(ctx.clone(), repo.clone(), a).await?;
        assert_eq!(
            utils_v1
                .pending(ctx.clone(), repo.clone(), vec![a, b, c])
                .await?,
            vec![b, c]
        );
        assert_eq!(
            utils_v2
                .pending(ctx.clone(), repo.clone(), vec![a, b, c])
                .await?,
            vec![a, b, c]
        );

        // Derive B directly, which should use the V2 mapping, as that is the
        // version configured on the repo.  V1 of B should still be underived.
        RootUnodeManifestId::derive(&ctx, &repo, b).await?;
        assert_eq!(
            utils_v1
                .pending(ctx.clone(), repo.clone(), vec![a, b, c])
                .await?,
            vec![b, c]
        );
        assert_eq!(
            utils_v2
                .pending(ctx.clone(), repo.clone(), vec![a, b, c])
                .await?,
            vec![c]
        );

        Ok(())
    }
}
