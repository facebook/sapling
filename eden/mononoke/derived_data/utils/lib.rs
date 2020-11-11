/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{anyhow, format_err, Error};
use async_trait::async_trait;
use blame::{BlameRoot, BlameRootMapping};
use blobrepo::BlobRepo;
use blobrepo_override::DangerousOverride;
use blobstore::{Blobstore, Loadable};
use cacheblob::{dummy::DummyLease, LeaseOps, MemWritesBlobstore};
use changeset_info::{ChangesetInfo, ChangesetInfoMapping};
use cloned::cloned;
use context::CoreContext;
use deleted_files_manifest::{RootDeletedManifestId, RootDeletedManifestMapping};
use derived_data::{
    derive_impl::derive_impl, BonsaiDerived, BonsaiDerivedMapping, DeriveError, Mode as DeriveMode,
    RegenerateMapping,
};
use derived_data_filenodes::{FilenodesOnlyPublic, FilenodesOnlyPublicMapping};
use fastlog::{RootFastlog, RootFastlogMapping};
use fsnodes::{RootFsnodeId, RootFsnodeMapping};
use futures::{
    compat::Future01CompatExt,
    future::{self, ready, try_join_all, BoxFuture, FutureExt},
    stream::{self, futures_unordered::FuturesUnordered},
    Future, Stream, StreamExt, TryFutureExt, TryStreamExt,
};
use lazy_static::lazy_static;
use lock_ext::LockExt;
use mercurial_derived_data::{HgChangesetIdMapping, MappedHgChangesetId};
use mononoke_types::{BonsaiChangeset, ChangesetId};
use skeleton_manifest::{RootSkeletonManifestId, RootSkeletonManifestMapping};
use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    io::Write,
    sync::{Arc, Mutex},
};
use topo_sort::sort_topological;
use unodes::{RootUnodeManifestId, RootUnodeManifestMapping};

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
        let derived_utils = derived_data_utils(repo.clone(), data_type)?;

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
    ) -> BoxFuture<'static, Result<(), Error>>;

    /// Find pending changeset (changesets for which data have not been derived)
    async fn pending(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csids: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetId>, Error>;

    /// Regenerate derived data for specified set of commits
    fn regenerate(&self, csids: &Vec<ChangesetId>);

    /// Get a name for this type of derived data
    fn name(&self) -> &'static str;

    async fn find_oldest_underived<'a>(
        &'a self,
        ctx: &'a CoreContext,
        repo: &'a BlobRepo,
        csids: &'a Vec<ChangesetId>,
    ) -> Result<Option<BonsaiChangeset>, Error>;
}

#[derive(Clone)]
struct DerivedUtilsFromMapping<M> {
    mapping: RegenerateMapping<M>,
    mode: DeriveMode,
}

impl<M> DerivedUtilsFromMapping<M> {
    fn new(mapping: M, mode: DeriveMode) -> Self {
        let mapping = RegenerateMapping::new(mapping);
        Self { mapping, mode }
    }
}

#[async_trait]
impl<M> DerivedUtils for DerivedUtilsFromMapping<M>
where
    M: BonsaiDerivedMapping + Clone + 'static,
    M::Value: BonsaiDerived + std::fmt::Debug,
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
        cloned!(self.mapping, self.mode);
        async move {
            let result = derive_impl::<M::Value, _>(&ctx, &repo, &mapping, csid, mode).await?;
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
    ) -> BoxFuture<'static, Result<(), Error>> {
        let orig_mapping = self.mapping.clone();
        // With InMemoryMapping we can ensure that mapping entries are written only after
        // all corresponding blobs were successfully saved
        let in_memory_mapping = InMemoryMapping::new(self.mapping.clone());

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

        async move {
            for csid in csids {
                // create new context so each derivation would have its own trace
                let ctx = CoreContext::new_with_logger(ctx.fb, ctx.logger().clone());
                derive_impl::<M::Value, _>(
                    &ctx,
                    &repo,
                    &in_memory_mapping,
                    csid,
                    DeriveMode::Unsafe,
                )
                .await?;
            }

            // flush blobstore
            memblobstore.persist(ctx.clone()).compat().await?;
            // flush mapping
            let futs = FuturesUnordered::new();
            {
                let buffer = in_memory_mapping.into_buffer();
                let buffer = buffer.lock().unwrap();
                for (cs_id, value) in buffer.iter() {
                    futs.push(orig_mapping.put(ctx.clone(), *cs_id, value.clone()));
                }
            }
            futs.try_for_each(|_| future::ok(())).await?;
            Ok(())
        }
        .boxed()
    }

    async fn pending(
        &self,
        ctx: CoreContext,
        _repo: BlobRepo,
        mut csids: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetId>, Error> {
        let derived = self.mapping.get(ctx, csids.clone()).await?;
        csids.retain(|csid| !derived.contains_key(&csid));
        Ok(csids)
    }

    async fn find_oldest_underived<'a>(
        &'a self,
        ctx: &'a CoreContext,
        repo: &'a BlobRepo,
        csids: &'a Vec<ChangesetId>,
    ) -> Result<Option<BonsaiChangeset>, Error> {
        let mut underived_ancestors = vec![];
        for cs_id in csids {
            underived_ancestors.push(M::Value::find_all_underived_ancestors(
                &ctx,
                &repo,
                vec![*cs_id],
            ));
        }

        let boxed_stream = stream::iter(underived_ancestors)
            .map(Result::<_, DeriveError>::Ok)
            .try_buffer_unordered(100)
            // boxed() is necessary to avoid "one type is more general than the other" error
            .boxed();

        let res = boxed_stream.try_collect::<Vec<_>>().await?;
        let oldest_changesets = stream::iter(
            res.into_iter()
                // The first element is the first underived ancestor in toposorted order.
                // Let's use it as a proxy for the oldest underived commit
                .map(|all_underived| all_underived.get(0).cloned())
                .flatten()
                .map(|cs_id| async move { cs_id.load(ctx.clone(), repo.blobstore()).await }),
        )
        .map(Ok)
        .try_buffer_unordered(100)
        // boxed() is necessary to avoid "one type is more general than the other" error
        .boxed();

        let oldest_changesets = oldest_changesets.try_collect::<Vec<_>>().await?;
        Ok(oldest_changesets
            .into_iter()
            .min_by_key(|bcs| *bcs.author_date()))
    }

    fn regenerate(&self, csids: &Vec<ChangesetId>) {
        self.mapping.regenerate(csids.iter().copied())
    }

    fn name(&self) -> &'static str {
        M::Value::NAME
    }
}

#[derive(Clone)]
struct InMemoryMapping<M: BonsaiDerivedMapping + Clone> {
    mapping: M,
    buffer: Arc<Mutex<HashMap<ChangesetId, M::Value>>>,
}

impl<M> InMemoryMapping<M>
where
    M: BonsaiDerivedMapping + Clone,
    <M as BonsaiDerivedMapping>::Value: Clone,
{
    fn new(mapping: M) -> Self {
        Self {
            mapping,
            buffer: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn into_buffer(self) -> Arc<Mutex<HashMap<ChangesetId, M::Value>>> {
        self.buffer
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
        ctx: CoreContext,
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
        _ctx: CoreContext,
        csid: ChangesetId,
        id: Self::Value,
    ) -> Result<(), Error> {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.insert(csid, id);
        Ok(())
    }
}

pub fn derived_data_utils(
    repo: BlobRepo,
    name: impl AsRef<str>,
) -> Result<Arc<dyn DerivedUtils>, Error> {
    derived_data_utils_impl(repo, name, DeriveMode::OnlyIfEnabled)
}

pub fn derived_data_utils_unsafe(
    repo: BlobRepo,
    name: impl AsRef<str>,
) -> Result<Arc<dyn DerivedUtils>, Error> {
    derived_data_utils_impl(repo, name, DeriveMode::Unsafe)
}

fn derived_data_utils_impl(
    repo: BlobRepo,
    name: impl AsRef<str>,
    mode: DeriveMode,
) -> Result<Arc<dyn DerivedUtils>, Error> {
    match name.as_ref() {
        RootUnodeManifestId::NAME => {
            let mapping = RootUnodeManifestMapping::new(
                repo.get_blobstore(),
                repo.get_derived_data_config().unode_version,
            );
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping, mode)))
        }
        RootFastlog::NAME => {
            let mapping = RootFastlogMapping::new(repo.get_blobstore().boxed());
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping, mode)))
        }
        MappedHgChangesetId::NAME => {
            let mapping = HgChangesetIdMapping::new(&repo);
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping, mode)))
        }
        RootFsnodeId::NAME => {
            let mapping = RootFsnodeMapping::new(repo.get_blobstore());
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping, mode)))
        }
        BlameRoot::NAME => {
            let mapping = BlameRootMapping::new(repo.get_blobstore().boxed());
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping, mode)))
        }
        ChangesetInfo::NAME => {
            let mapping = ChangesetInfoMapping::new(repo.get_blobstore().boxed());
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping, mode)))
        }
        RootDeletedManifestId::NAME => {
            let mapping = RootDeletedManifestMapping::new(repo.get_blobstore());
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping, mode)))
        }
        FilenodesOnlyPublic::NAME => {
            let mapping = FilenodesOnlyPublicMapping::new(repo);
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping, mode)))
        }
        RootSkeletonManifestId::NAME => {
            let mapping = RootSkeletonManifestMapping::new(repo.get_blobstore());
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping, mode)))
        }
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

    /// Derive all data in the graph
    pub async fn derive(&self, ctx: CoreContext, repo: BlobRepo) -> Result<(), Error> {
        bounded_traversal::bounded_traversal_dag(
            100,
            self.clone(),
            |node| async move {
                let deps = node.dependencies.clone();
                Ok((node, deps))
            },
            move |node, _| {
                cloned!(ctx, repo);
                async move {
                    if let Some(deriver) = &node.deriver {
                        deriver
                            .backfill_batch_dangerous(ctx.clone(), repo, node.csids.clone())
                            .await?;
                        if let (Some(first), Some(last)) = (node.csids.first(), node.csids.last()) {
                            slog::info!(
                                ctx.logger(),
                                "[{}:{}] count:{} start:{} end:{}",
                                deriver.name(),
                                node.id,
                                node.csids.len(),
                                first,
                                last
                            );
                        }
                    }
                    Ok::<_, Error>(())
                }
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
            slog::info!(
                ctx.logger(),
                "found changsets: {} {:.3}/s",
                found_changesets,
                found_changesets as f32 / start.elapsed().as_secs_f32(),
            );
        }
    }
    if found_changesets > 0 {
        slog::info!(
            ctx.logger(),
            "found changsets: {} {:.3}/s",
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
                    let parents = changeset_fetcher
                        .get_parents(ctx.clone(), csid)
                        .compat()
                        .await?;
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
        }
    })
    .try_filter_map(future::ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bookmarks::BookmarkName;
    use fbinit::FacebookInit;
    use fixtures::merge_even;
    use maplit::btreemap;
    use std::{
        collections::BTreeMap,
        sync::atomic::{AtomicUsize, Ordering},
    };
    use tests_utils::drawdag::{changes, create_from_dag_with_changes};

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

    #[fbinit::compat_test]
    async fn test_build_derive_graph(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = merge_even::getrepo(fb).await;
        let thin_out = ThinOut::new_keep_all();
        let master = repo
            .get_bonsai_bookmark(ctx.clone(), &BookmarkName::new("master").unwrap())
            .compat()
            .await?
            .unwrap();
        let blame_deriver = derived_data_utils(repo.clone(), "blame")?;
        let unodes_deriver = derived_data_utils(repo.clone(), "unodes")?;

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
        graph.derive(ctx.clone(), repo.clone()).await?;

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
        ) -> BoxFuture<'static, Result<(), Error>> {
            self.deriver.backfill_batch_dangerous(ctx, repo, csids)
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

        fn regenerate(&self, _csids: &Vec<ChangesetId>) {
            unimplemented!()
        }

        fn name(&self) -> &'static str {
            self.deriver.name()
        }

        async fn find_oldest_underived<'a>(
            &'a self,
            _ctx: &'a CoreContext,
            _repo: &'a BlobRepo,
            _csids: &'a Vec<ChangesetId>,
        ) -> Result<Option<BonsaiChangeset>, Error> {
            unimplemented!()
        }
    }

    #[fbinit::compat_test]
    async fn test_find_underived_many(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = blobrepo_factory::new_memblob_empty(None)?;

        let dag = create_from_dag_with_changes(&ctx, &repo, "A-B-C", changes! {}).await?;
        let a = *dag.get("A").unwrap();
        let b = *dag.get("B").unwrap();
        let c = *dag.get("C").unwrap();

        let thin_out = ThinOut::new_keep_all();
        let blame_deriver = derived_data_utils(repo.clone(), "blame")?;
        let unodes_deriver = {
            let deriver = derived_data_utils(repo.clone(), "unodes")?;
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
}
