/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::Error;
use context::{CoreContext, PerfCounterType};
use manifest::Entry;
use mononoke_types::{
    hash::Blake2, path_bytes_from_mpath, ChangesetId, FileUnodeId, MPath, ManifestUnodeId,
    RepositoryId,
};
use path_hash::{PathHash, PathHashBytes};
use sql::{queries, Connection};
use sql_construct::{SqlConstruct, SqlConstructFromMetadataDatabaseConfig};
use sql_ext::SqlConnections;
use std::sync::Arc;

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

    pub fn src_path(&self) -> &Option<MPath> {
        &self.src_path
    }

    fn src_path_hash(&self) -> &PathHash {
        &self.src_path_hash
    }

    pub fn src_cs_id(&self) -> ChangesetId {
        self.src_cs_id
    }

    pub fn get_src_unode(&self) -> Entry<ManifestUnodeId, FileUnodeId> {
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
}

impl MutableRenames {
    pub fn new(repo_id: RepositoryId, store: SqlMutableRenamesStore) -> Self {
        Self {
            repo_id,
            store: Arc::new(store),
        }
    }

    pub async fn add_or_overwrite_renames(
        &self,
        ctx: &CoreContext,
        renames: Vec<MutableRenameEntry>,
    ) -> Result<(), Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);

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

    async fn has_rename(&self, ctx: &CoreContext, dst_cs_id: ChangesetId) -> Result<bool, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);

        let rename_targets =
            HasRenameCheck::query(&self.store.read_connection, &self.repo_id, &dst_cs_id).await?;

        Ok(!rename_targets.is_empty())
    }

    pub async fn get_rename(
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
}
