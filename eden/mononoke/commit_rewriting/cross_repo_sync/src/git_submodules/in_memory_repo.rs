/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::bail;
use anyhow::Result;
use async_trait::async_trait;
use bonsai_hg_mapping::MemWritesBonsaiHgMapping;
use cacheblob::MemWritesBlobstore;
use changesets::ChangesetEntry;
use changesets::ChangesetInsert;
use changesets::Changesets;
use changesets::SortOrder;
use changesets_impl::SqlChangesets;
use changesets_impl::SqlChangesetsBuilder;
use commit_graph::CommitGraph;
use context::CoreContext;
use derivative::Derivative;
use filestore::FilestoreConfig;
use futures::stream::BoxStream;
use futures::try_join;
use itertools::Itertools;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::Generation;
use mononoke_types::RepositoryId;
use readonlyblob::ReadOnlyBlobstore;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;
use scuba_ext::MononokeScubaSampleBuilder;
use sql_construct::SqlConstruct;
use vec1::Vec1;

use crate::git_submodules::dummy_struct::DummyStruct;
use crate::types::Repo;

/// Container to access a repo's blobstore and derived data without writing
/// anything to its blobstore or changeset table.
/// It's current purpose is to perform validation of git submodule expansion
/// by deriving fsnodes from uncommitted bonsais in the large repo.
#[facet::container]
#[derive(Clone)]
pub struct InMemoryRepo {
    #[facet]
    pub(crate) repo_blobstore: RepoBlobstore,

    #[facet]
    pub(crate) derived_data: RepoDerivedData,

    #[facet]
    filestore_config: FilestoreConfig,

    // Only needed for creation of RepoDerivedData and for unit test
    #[facet]
    pub changesets: dyn Changesets,
}

impl InMemoryRepo {
    pub fn from_repo<R: Repo + Send + Sync>(repo: &R) -> Result<Self> {
        let repo_identity = repo.repo_identity();

        let scuba = MononokeScubaSampleBuilder::with_discard();

        let original_blobstore = repo.repo_blobstore().clone();

        let mem_writes_repo_blobstore =
            RepoBlobstore::new_with_wrapped_inner_blobstore(original_blobstore, |blobstore| {
                let readonly_blobstore = ReadOnlyBlobstore::new(blobstore);
                Arc::new(MemWritesBlobstore::new(readonly_blobstore))
            });
        let orig_derived_data = repo.repo_derived_data();

        let orig_changesets = repo.changesets_arc().clone();
        let in_memory_changesets = Arc::new(InMemoryChangesets::new(orig_changesets)?);
        let bonsai_hg_mapping = MemWritesBonsaiHgMapping::new(repo.bonsai_hg_mapping_arc());

        let filenodes = Arc::new(DummyStruct);
        let bonsai_git_mapping = Arc::new(DummyStruct);

        let commit_graph_storage = Arc::new(DummyStruct);
        let commit_graph = CommitGraph::new(commit_graph_storage);

        let lease = orig_derived_data.lease().clone();

        let derived_data_config = orig_derived_data.config().clone();

        let derivation_service_client = None;

        let repo_derived_data = RepoDerivedData::new(
            repo_identity.id(),
            repo_identity.name().to_string(),
            in_memory_changesets.clone(),
            commit_graph.into(),
            Arc::new(bonsai_hg_mapping),
            bonsai_git_mapping,
            filenodes,
            mem_writes_repo_blobstore.clone(),
            lease,
            scuba,
            derived_data_config,
            derivation_service_client,
        )?;

        let filestore_config = repo.filestore_config().clone();
        Ok(Self {
            repo_blobstore: mem_writes_repo_blobstore.into(),
            derived_data: repo_derived_data.into(),
            filestore_config: filestore_config.into(),
            changesets: in_memory_changesets,
        })
    }
}

#[derive(Derivative, Clone)]
pub struct InMemoryChangesets {
    inner: Arc<dyn Changesets>,
    sql_in_memory: Arc<SqlChangesets>,
}

impl InMemoryChangesets {
    pub fn new(inner: Arc<dyn Changesets>) -> Result<Self> {
        let sql_in_memory: SqlChangesets = SqlChangesetsBuilder::with_sqlite_in_memory()?
            .build(Default::default(), inner.repo_id());
        Ok(Self {
            inner,
            sql_in_memory: Arc::new(sql_in_memory),
        })
    }

    async fn get_ephemeral(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
    ) -> Result<Vec<ChangesetEntry>> {
        self.sql_in_memory.get_many(ctx, cs_ids.to_vec()).await
    }
}

#[async_trait]
impl Changesets for InMemoryChangesets {
    fn repo_id(&self) -> RepositoryId {
        self.inner.repo_id()
    }

    async fn add(&self, ctx: &CoreContext, cs: ChangesetInsert) -> Result<bool> {
        let parents_len = cs.parents.len();
        let parents = self.get_many(ctx, cs.parents.clone()).await?;
        if parents.len() != parents_len {
            bail!(
                "Not all parents found, expected [{}], found [{}]",
                cs.parents.into_iter().map(|id| id.to_string()).join(", "),
                parents
                    .into_iter()
                    .map(|entry| entry.cs_id.to_string())
                    .join(", ")
            );
        }
        self.sql_in_memory.add(ctx, cs).await
    }

    async fn add_many(
        &self,
        ctx: &CoreContext,
        css: Vec1<(ChangesetInsert, Generation)>,
    ) -> Result<()> {
        // If necessary, this can be optimised.
        for (cs, _) in css {
            self.add(ctx, cs).await?;
        }
        Ok(())
    }

    async fn get(&self, ctx: &CoreContext, cs_id: ChangesetId) -> Result<Option<ChangesetEntry>> {
        Ok(self.get_many(ctx, vec![cs_id]).await?.into_iter().next())
    }

    async fn get_many(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetEntry>> {
        let ephemeral = self.get_ephemeral(ctx, &cs_ids);
        let persistent = self.inner.get_many(ctx, cs_ids.clone());
        let (mut ephemeral, persistent) = try_join!(ephemeral, persistent)?;
        ephemeral.extend(persistent);
        Ok(ephemeral)
    }

    /// Use caching for the full changeset ids and slower path otherwise.
    async fn get_many_by_prefix(
        &self,
        _ctx: &CoreContext,
        _cs_prefix: ChangesetIdPrefix,
        _limit: usize,
    ) -> Result<ChangesetIdsResolvedFromPrefix> {
        unimplemented!()
    }

    fn prime_cache(&self, _ctx: &CoreContext, _changesets: &[ChangesetEntry]) {
        // no caching involved
    }

    async fn enumeration_bounds(
        &self,
        _ctx: &CoreContext,
        _read_from_master: bool,
        _known_heads: Vec<ChangesetId>,
    ) -> Result<Option<(u64, u64)>> {
        unimplemented!()
    }

    fn list_enumeration_range(
        &self,
        _ctx: &CoreContext,
        _min_id: u64,
        _max_id: u64,
        _sort_and_limit: Option<(SortOrder, u64)>,
        _read_from_master: bool,
    ) -> BoxStream<'_, Result<(ChangesetId, u64)>> {
        unimplemented!()
    }
}
