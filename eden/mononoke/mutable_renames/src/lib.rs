/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Error;
use cachelib::VolatileLruCachePool;
use changesets::Changesets;
use context::CoreContext;
use context::PerfCounterType;
use fbinit::FacebookInit;
use futures::try_join;
use manifest::Entry;
use maplit::hashset;
use mononoke_types::hash::Blake2;
use mononoke_types::path_bytes_from_mpath;
use mononoke_types::ChangesetId;
use mononoke_types::FileUnodeId;
use mononoke_types::MPath;
use mononoke_types::ManifestUnodeId;
use mononoke_types::RepositoryId;
use path_hash::PathHash;
use path_hash::PathHashBytes;
use sql::queries;
use sql::Connection;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;
use std::collections::HashSet;
use std::sync::Arc;

mod caching;
use crate::caching::CacheHandlers;
use crate::caching::GetCsIdsKey;
use crate::caching::RenameKey;
#[cfg(test)]
mod tests;

pub struct SqlMutableRenamesStore {
    write_connection: Connection,
    read_connection: Connection,
}

impl SqlConstruct for SqlMutableRenamesStore {
    const LABEL: &'static str = "mutable_renames";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-mutable-renames.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            write_connection: connections.write_connection,
            read_connection: connections.read_connection,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlMutableRenamesStore {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MutableRenameEntry {
    dst_cs_id: ChangesetId,
    dst_path_hash: PathHash,
    src_cs_id: ChangesetId,
    src_path: Option<MPath>,
    src_path_hash: PathHash,
    src_unode: Blake2,
    is_tree: i8,
}

impl MutableRenameEntry {
    /// Create a new entry to pass to `add`
    /// This says that dst_path in dst_cs_id is in fact the immediate child of src_path at src_cs_id
    /// The unode is needed to allow us to reconstruct unode history correctly.
    /// If either path is `None`, this represents the root of the repo
    pub fn new(
        dst_cs_id: ChangesetId,
        dst_path: Option<MPath>,
        src_cs_id: ChangesetId,
        src_path: Option<MPath>,
        src_unode: Entry<ManifestUnodeId, FileUnodeId>,
    ) -> Result<Self, Error> {
        let (src_unode, is_tree) = match src_unode {
            Entry::Tree(ref mf_unode_id) => (*mf_unode_id.blake2(), true),
            Entry::Leaf(ref leaf_unode_id) => (*leaf_unode_id.blake2(), false),
        };

        let dst_path_hash = PathHash::from_path_and_is_tree(dst_path.as_ref(), is_tree);
        let src_path_hash = PathHash::from_path_and_is_tree(src_path.as_ref(), is_tree);
        let is_tree = *dst_path_hash.sql_is_tree();

        Ok(Self {
            dst_cs_id,
            dst_path_hash,
            src_cs_id,
            src_path,
            src_path_hash,
            src_unode,
            is_tree,
        })
    }

    fn dst_path_hash(&self) -> &PathHash {
        &self.dst_path_hash
    }

    /// Get the source path for this entry, or None if the source
    /// is the repo root
    pub fn src_path(&self) -> Option<&MPath> {
        self.src_path.as_ref()
    }

    fn src_path_hash(&self) -> &PathHash {
        &self.src_path_hash
    }

    /// Get the source changeset ID for this entry
    pub fn src_cs_id(&self) -> ChangesetId {
        self.src_cs_id
    }

    /// Get the unode you would find by looking up src_path()
    /// in src_cs_id() - this is faster because it's pre-cached
    pub fn src_unode(&self) -> Entry<ManifestUnodeId, FileUnodeId> {
        if self.is_tree == 1 {
            Entry::Tree(ManifestUnodeId::new(self.src_unode))
        } else {
            Entry::Leaf(FileUnodeId::new(self.src_unode))
        }
    }
}

#[facet::facet]
#[derive(Clone)]
pub struct MutableRenames {
    repo_id: RepositoryId,
    store: Arc<SqlMutableRenamesStore>,
    cache_handlers: Option<CacheHandlers>,
}

impl MutableRenames {
    pub fn new(
        fb: FacebookInit,
        repo_id: RepositoryId,
        store: SqlMutableRenamesStore,
        cache_pool: Option<VolatileLruCachePool>,
    ) -> Result<Self, Error> {
        let cache_handlers = cache_pool
            .map(|pool| CacheHandlers::new(fb, pool))
            .transpose()?;
        Ok(Self {
            repo_id,
            store: Arc::new(store),
            cache_handlers,
        })
    }

    pub fn new_test(repo_id: RepositoryId, store: SqlMutableRenamesStore) -> Self {
        let cache_handlers = Some(CacheHandlers::new_test());
        Self {
            repo_id,
            store: Arc::new(store),
            cache_handlers,
        }
    }

    pub async fn add_or_overwrite_renames(
        &self,
        ctx: &CoreContext,
        changesets: &dyn Changesets,
        renames: Vec<MutableRenameEntry>,
    ) -> Result<(), Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);

        // Check to see if any of the added renames has an src that's a
        // descendant of its dst. If so, we reject this as we cannot sanely
        // handle cycles in history
        for (src, dst) in renames.iter().map(|mre| (mre.src_cs_id, mre.dst_cs_id)) {
            let (src_entry, dst_entry) = try_join!(
                changesets.get(ctx.clone(), src),
                changesets.get(ctx.clone(), dst)
            )?;
            let src_entry = src_entry.ok_or_else(|| anyhow!("Commit {} does not exist", src))?;
            let dst_entry = dst_entry.ok_or_else(|| anyhow!("Commit {} does not exist", dst))?;
            if src_entry.gen >= dst_entry.gen {
                // The source commit could potentially be a descendant of the target
                // Ideally, we'd do a proper check here to see if this forms a loop
                // in history, allowing for both mutable and immutable history
                //
                // For now, though, just bail
                bail!(
                    "{} is a potential descendant of {} - rejecting to avoid loops in history",
                    src_entry.cs_id,
                    dst_entry.cs_id
                );
            }
        }

        // First insert path <-> path_hash mapping
        let mut rows = vec![];
        for rename in &renames {
            rows.push((
                &rename.dst_path_hash().hash.0,
                &rename.dst_path_hash().path_bytes.0,
            ));
            rows.push((
                &rename.src_path_hash().hash.0,
                &rename.src_path_hash().path_bytes.0,
            ));
        }

        AddPaths::query(&self.store.write_connection, &rows[..]).await?;

        // Now insert the renames
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);
        let mut rows = vec![];

        for rename in &renames {
            rows.push((
                &self.repo_id,
                &rename.dst_cs_id,
                &rename.dst_path_hash().hash.0,
                &rename.src_cs_id,
                &rename.src_path_hash().hash.0,
                &rename.src_unode,
                &rename.is_tree,
            ));
        }

        AddRenames::query(&self.store.write_connection, &rows[..]).await?;

        Ok(())
    }

    pub async fn has_rename_uncached(
        &self,
        ctx: &CoreContext,
        dst_cs_id: ChangesetId,
    ) -> Result<bool, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);

        let rename_targets =
            HasRenameCheck::query(&self.store.read_connection, &self.repo_id, &dst_cs_id).await?;

        Ok(!rename_targets.is_empty())
    }

    pub async fn has_rename(
        &self,
        ctx: &CoreContext,
        dst_cs_id: ChangesetId,
    ) -> Result<bool, Error> {
        match &self.cache_handlers {
            None => self.has_rename_uncached(ctx, dst_cs_id).await,
            Some(cache_handlers) => {
                let keys = hashset![dst_cs_id];

                let cache = cache_handlers.has_rename(self, ctx);

                let res = caching_ext::get_or_fill(cache, keys).await?;

                Ok(res.get(&dst_cs_id).map_or(false, |r| r.0))
            }
        }
    }

    pub async fn get_rename_uncached(
        &self,
        ctx: &CoreContext,
        dst_cs_id: ChangesetId,
        dst_path: Option<MPath>,
    ) -> Result<Option<MutableRenameEntry>, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);

        if !self.has_rename(ctx, dst_cs_id).await? {
            return Ok(None);
        }

        let dst_path_bytes = path_bytes_from_mpath(dst_path.as_ref());
        let dst_path_hash = PathHashBytes::new(&dst_path_bytes);
        let mut rows = GetRename::query(
            &self.store.read_connection,
            &self.repo_id,
            &dst_cs_id,
            &dst_path_hash.0,
        )
        .await?;
        match rows.pop() {
            Some((src_cs_id, src_path_bytes, src_unode, is_tree)) => {
                let src_path = MPath::new_opt(src_path_bytes)?;
                let src_unode = if is_tree == 1 {
                    Entry::Tree(ManifestUnodeId::new(src_unode))
                } else {
                    Entry::Leaf(FileUnodeId::new(src_unode))
                };

                Ok(Some(MutableRenameEntry::new(
                    dst_cs_id, dst_path, src_cs_id, src_path, src_unode,
                )?))
            }
            None => Ok(None),
        }
    }

    pub async fn get_rename(
        &self,
        ctx: &CoreContext,
        dst_cs_id: ChangesetId,
        dst_path: Option<MPath>,
    ) -> Result<Option<MutableRenameEntry>, Error> {
        match &self.cache_handlers {
            None => self.get_rename_uncached(ctx, dst_cs_id, dst_path).await,
            Some(cache_handlers) => {
                let key = RenameKey::new(dst_cs_id, dst_path);
                let keys = hashset![key.clone()];

                let cache = cache_handlers.get_rename(self, ctx);

                let mut res = caching_ext::get_or_fill(cache, keys).await?;

                let res = res
                    .remove(&key)
                    .and_then(|r| r.0.map(MutableRenameEntry::try_from))
                    .transpose()?;
                Ok(res)
            }
        }
    }

    pub async fn get_cs_ids_with_rename_uncached(
        &self,
        ctx: &CoreContext,
        dst_path: Option<MPath>,
    ) -> Result<HashSet<ChangesetId>, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);

        let dst_path_bytes = path_bytes_from_mpath(dst_path.as_ref());
        let dst_path_hash = PathHashBytes::new(&dst_path_bytes);
        let rows = FindRenames::query(&self.store.read_connection, &self.repo_id, &dst_path_hash.0)
            .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    pub async fn get_cs_ids_with_rename(
        &self,
        ctx: &CoreContext,
        dst_path: Option<MPath>,
    ) -> Result<HashSet<ChangesetId>, Error> {
        match &self.cache_handlers {
            None => self.get_cs_ids_with_rename_uncached(ctx, dst_path).await,
            Some(cache_handlers) => {
                let key = GetCsIdsKey::new(dst_path);
                let keys = hashset![key.clone()];

                let cache = cache_handlers.get_cs_ids_with_rename(self, ctx);

                caching_ext::get_or_fill(cache, keys).await.map(|mut r| {
                    let res = r.remove(&key);
                    res.map_or(HashSet::new(), |r| r.into())
                })
            }
        }
    }
}

queries! {
    write AddRenames(values: (
        repo_id: RepositoryId,
        dst_cs_id: ChangesetId,
        dst_path_hash: Vec<u8>,
        src_cs_id: ChangesetId,
        src_path_hash: Vec<u8>,
        src_unode_id: Blake2,
        is_tree: i8,
    )) {
        none,
        mysql(
            "INSERT INTO mutable_renames (repo_id, dst_cs_id, dst_path_hash, src_cs_id, src_path_hash, src_unode_id, is_tree) VALUES {values}
            ON DUPLICATE KEY UPDATE src_cs_id = VALUES(src_cs_id), src_path_hash = VALUES(src_path_hash), src_unode_id = VALUES(src_unode_id), is_tree = VALUES(is_tree)
            "
        )
        sqlite(
            "REPLACE INTO mutable_renames (repo_id, dst_cs_id, dst_path_hash, src_cs_id, src_path_hash, src_unode_id, is_tree) VALUES {values}"
        )
    }

    write AddPaths(values: (
        path_hash: Vec<u8>,
        path: Vec<u8>,
    )) {
        insert_or_ignore,
        "{insert_or_ignore} INTO mutable_renames_paths (path_hash, path) VALUES {values}"
    }

    read GetRename(repo_id: RepositoryId, dst_cs_id: ChangesetId, dst_path_hash: Vec<u8>) -> (
       ChangesetId,
       Vec<u8>,
       Blake2,
       i8
    ) {
        "
        SELECT
            mutable_renames.src_cs_id,
            mutable_renames_paths.path,
            mutable_renames.src_unode_id,
            mutable_renames.is_tree
        FROM mutable_renames JOIN mutable_renames_paths
        ON mutable_renames.src_path_hash = mutable_renames_paths.path_hash
        WHERE mutable_renames.repo_id = {repo_id}
           AND mutable_renames.dst_cs_id = {dst_cs_id}
           AND  mutable_renames.dst_path_hash = {dst_path_hash}
        "
    }

    read HasRenameCheck(repo_id: RepositoryId, dst_cs_id: ChangesetId) -> (ChangesetId) {
        "
        SELECT
            mutable_renames.src_cs_id
        FROM mutable_renames
        WHERE
            mutable_renames.repo_id = {repo_id}
           AND mutable_renames.dst_cs_id = {dst_cs_id}
        LIMIT 1
        "
    }

    read FindRenames(repo_id: RepositoryId, dst_path_hash: Vec<u8>) -> (ChangesetId) {
        "
        SELECT
            mutable_renames.dst_cs_id
        FROM mutable_renames
        WHERE
            mutable_renames.repo_id = {repo_id}
            AND mutable_renames.dst_path_hash = {dst_path_hash}
        "
    }
}
