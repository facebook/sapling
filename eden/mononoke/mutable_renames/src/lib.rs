/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::Error;
use context::{CoreContext, PerfCounterType};
use manifest::Entry;
use mononoke_types::{
    hash::Blake2, ChangesetId, FileUnodeId, MPath, ManifestUnodeId, RepositoryId,
};
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
    dst_path_bytes: Vec<u8>,
    src_cs_id: ChangesetId,
    src_path_bytes: Vec<u8>,
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
    ) -> Self {
        let dst_path_bytes = convert_path(dst_path);
        let src_path_bytes = convert_path(src_path);

        let (src_unode, is_tree) = match src_unode {
            Entry::Tree(ref mf_unode_id) => (*mf_unode_id.blake2(), 1),
            Entry::Leaf(ref leaf_unode_id) => (*leaf_unode_id.blake2(), 0),
        };

        Self {
            dst_cs_id,
            dst_path_bytes,
            src_cs_id,
            src_path_bytes,
            src_unode,
            is_tree,
        }
    }

    pub fn src_cs_id(&self) -> ChangesetId {
        self.src_cs_id
    }

    pub fn src_path(&self) -> Result<Option<MPath>, Error> {
        MPath::new_opt(self.src_path_bytes.clone())
    }

    pub fn src_unode_entry(&self) -> Entry<ManifestUnodeId, FileUnodeId> {
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
        let mut rows = vec![];

        for rename in &renames {
            rows.push((
                &self.repo_id,
                &rename.dst_cs_id,
                &rename.dst_path_bytes,
                &rename.src_cs_id,
                &rename.src_path_bytes,
                &rename.src_unode,
                &rename.is_tree,
            ));
        }

        AddRenames::query(&self.store.write_connection, &rows[..]).await?;

        Ok(())
    }

    pub async fn get_rename(
        &self,
        ctx: &CoreContext,
        dst_cs_id: ChangesetId,
        dst_path: Option<MPath>,
    ) -> Result<Option<MutableRenameEntry>, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let dst_path_bytes = convert_path(dst_path);
        let mut rows = GetRename::query(
            &self.store.read_connection,
            &self.repo_id,
            &dst_cs_id,
            &dst_path_bytes,
        )
        .await?;
        match rows.pop() {
            Some((src_cs_id, src_path_bytes, src_unode, is_tree)) => Ok(Some(MutableRenameEntry {
                dst_cs_id,
                dst_path_bytes,
                src_cs_id,
                src_path_bytes,
                src_unode,
                is_tree,
            })),
            None => Ok(None),
        }
    }
}

fn convert_path(path: Option<MPath>) -> Vec<u8> {
    match path {
        Some(path) => path.to_vec(),
        None => vec![],
    }
}

queries! {
    write AddRenames(values: (
        repo_id: RepositoryId,
        dst_cs_id: ChangesetId,
        dst_path: Vec<u8>,
        src_cs_id: ChangesetId,
        src_path: Vec<u8>,
        src_unode_id: Blake2,
        is_tree: i8,
    )) {
        none,
        mysql(
            "INSERT INTO mutable_renames (repo_id, dst_cs_id, dst_path, src_cs_id, src_path, src_unode_id, is_tree) VALUES {values}
            ON DUPLICATE KEY UPDATE src_cs_id = VALUES(src_cs_id), src_path = VALUES(src_path), src_unode_id = VALUES(src_unode_id), is_tree = VALUES(is_tree)
            "
        )
        sqlite(
            "REPLACE INTO mutable_renames (repo_id, dst_cs_id, dst_path, src_cs_id, src_path, src_unode_id, is_tree) VALUES {values}"
        )
    }

    read GetRename(repo_id: RepositoryId, dst_cs_id: ChangesetId, dst_path: Vec<u8>) -> (
       ChangesetId,
       Vec<u8>,
       Blake2,
       i8
    ) {
        "
        SELECT
            src_cs_id,
            src_path,
            src_unode_id,
            is_tree
        FROM mutable_renames
        WHERE repo_id = {repo_id}
           AND dst_cs_id = {dst_cs_id}
           AND dst_path = {dst_path}
        "
    }
}
