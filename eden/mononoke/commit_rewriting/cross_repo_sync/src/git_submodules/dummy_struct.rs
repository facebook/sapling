/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use ::sql::Transaction;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use bonsai_git_mapping::AddGitMappingErrorKind;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_git_mapping::BonsaiGitMappingEntry;
use bonsai_git_mapping::BonsaisOrGitShas;
use commit_graph_types::edges::ChangesetEdges;
use commit_graph_types::storage::CommitGraphStorage;
use commit_graph_types::storage::FetchedChangesetEdges;
use commit_graph_types::storage::Prefetch;
use context::CoreContext;
use filenodes::FilenodeInfo;
use filenodes::FilenodeRange;
use filenodes::FilenodeResult;
use filenodes::Filenodes;
use filenodes::PreparedFilenode;
use mercurial_types::HgFileNodeId;
use mononoke_types::hash::GitSha1;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::RepoPath;
use mononoke_types::RepositoryId;
use vec1::Vec1;

/// Struct created to satisfy the type system when creating a `RepoDerivedData`
/// for the `InMemoryRepo`.
pub(crate) struct DummyStruct;

#[async_trait]
impl Filenodes for DummyStruct {
    async fn add_filenodes(
        &self,
        _ctx: &CoreContext,
        _info: Vec<PreparedFilenode>,
    ) -> Result<FilenodeResult<()>> {
        unimplemented!()
    }

    async fn add_or_replace_filenodes(
        &self,
        _ctx: &CoreContext,
        _info: Vec<PreparedFilenode>,
    ) -> Result<FilenodeResult<()>> {
        unimplemented!()
    }

    async fn get_filenode(
        &self,
        _ctx: &CoreContext,
        _path: &RepoPath,
        _filenode: HgFileNodeId,
    ) -> Result<FilenodeResult<Option<FilenodeInfo>>> {
        unimplemented!()
    }

    async fn get_all_filenodes_maybe_stale(
        &self,
        _ctx: &CoreContext,
        _path: &RepoPath,
        _limit: Option<u64>,
    ) -> Result<FilenodeResult<FilenodeRange>> {
        unimplemented!()
    }

    fn prime_cache(&self, _ctx: &CoreContext, _filenodes: &[PreparedFilenode]) {
        unimplemented!()
    }
}

#[async_trait]
impl BonsaiGitMapping for DummyStruct {
    fn repo_id(&self) -> RepositoryId {
        unimplemented!()
    }

    async fn add(
        &self,
        _ctx: &CoreContext,
        _entry: BonsaiGitMappingEntry,
    ) -> Result<(), AddGitMappingErrorKind> {
        unimplemented!()
    }

    async fn bulk_add(
        &self,
        _ctx: &CoreContext,
        _entries: &[BonsaiGitMappingEntry],
    ) -> Result<(), AddGitMappingErrorKind> {
        unimplemented!()
    }

    async fn bulk_add_git_mapping_in_transaction(
        &self,
        _ctx: &CoreContext,
        _entries: &[BonsaiGitMappingEntry],
        _transaction: Transaction,
    ) -> Result<Transaction, AddGitMappingErrorKind> {
        unimplemented!()
    }

    async fn get(
        &self,
        _ctx: &CoreContext,
        _cs: BonsaisOrGitShas,
    ) -> Result<Vec<BonsaiGitMappingEntry>, Error> {
        unimplemented!()
    }

    /// Use caching for the ranges of one element, use slower path otherwise.
    async fn get_in_range(
        &self,
        _ctx: &CoreContext,
        _low: GitSha1,
        _high: GitSha1,
        _limit: usize,
    ) -> Result<Vec<GitSha1>, Error> {
        unimplemented!()
    }
}

#[async_trait]
impl CommitGraphStorage for DummyStruct {
    fn repo_id(&self) -> RepositoryId {
        unimplemented!()
    }

    async fn add(&self, _ctx: &CoreContext, _edges: ChangesetEdges) -> Result<bool> {
        unimplemented!()
    }

    async fn add_many(
        &self,
        _ctx: &CoreContext,
        _many_edges: Vec1<ChangesetEdges>,
    ) -> Result<usize> {
        unimplemented!()
    }

    async fn fetch_edges(&self, _ctx: &CoreContext, _cs_id: ChangesetId) -> Result<ChangesetEdges> {
        unimplemented!()
    }

    async fn maybe_fetch_edges(
        &self,
        _ctx: &CoreContext,
        _cs_id: ChangesetId,
    ) -> Result<Option<ChangesetEdges>> {
        unimplemented!()
    }

    async fn fetch_many_edges(
        &self,
        _ctx: &CoreContext,
        _cs_ids: &[ChangesetId],
        _prefetch: Prefetch,
    ) -> Result<HashMap<ChangesetId, FetchedChangesetEdges>> {
        unimplemented!()
    }

    async fn maybe_fetch_many_edges(
        &self,
        _ctx: &CoreContext,
        _cs_ids: &[ChangesetId],
        _prefetch: Prefetch,
    ) -> Result<HashMap<ChangesetId, FetchedChangesetEdges>> {
        unimplemented!()
    }

    async fn find_by_prefix(
        &self,
        _ctx: &CoreContext,
        _cs_prefix: ChangesetIdPrefix,
        _limit: usize,
    ) -> Result<ChangesetIdsResolvedFromPrefix> {
        unimplemented!()
    }

    async fn fetch_children(
        &self,
        _ctx: &CoreContext,
        _cs_id: ChangesetId,
    ) -> Result<Vec<ChangesetId>> {
        unimplemented!()
    }
}
