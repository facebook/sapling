/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::{hash_map, HashMap, HashSet};

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::compat::Future01CompatExt;
use mercurial_types::HgChangesetId;
use mononoke_types::{DateTime, RepositoryId};
use smallvec::SmallVec;
use sql::{queries, Connection};
use sql_ext::SqlConnections;

use crate::entry::{HgMutationEntry, HgMutationEntrySet};
use crate::HgMutationStore;

pub struct SqlHgMutationStore {
    repo_id: RepositoryId,
    connections: SqlConnections,
}

/// Convenience alias for the type of each row returned from the entries queries.
type EntryRow = (
    HgChangesetId, // Successor
    HgChangesetId, // Primordial
    u64,           // Predecessor Count
    u64,           // Predecessor Index
    HgChangesetId, // Predecessor
    HgChangesetId, // Predecessor's Primordial
    u64,           // Split Count
    String,        // Op
    String,        // User
    i64,           // Timestamp
    i32,           // Time Zone
    String,        // Extras
);

impl SqlHgMutationStore {
    pub fn new(repo_id: RepositoryId, connections: SqlConnections) -> Self {
        Self {
            repo_id,
            connections,
        }
    }

    pub async fn add_raw_entries_for_test(
        &self,
        entries: Vec<HgMutationEntry>,
        changeset_primordials: HashMap<HgChangesetId, HgChangesetId>,
    ) -> Result<()> {
        let txn = self
            .connections
            .write_connection
            .start_transaction()
            .compat()
            .await?;

        let mut db_csets = Vec::new();
        let mut db_entries = Vec::new();
        let mut db_preds = Vec::new();
        let mut db_splits = Vec::new();
        for changeset in changeset_primordials.keys() {
            db_csets.push((&self.repo_id, changeset));
        }
        for entry in entries.iter() {
            let primordial = changeset_primordials
                .get(entry.successor())
                .expect("successor must have primordial");
            db_entries.push((
                &self.repo_id,
                entry.successor(),
                primordial,
                entry.predecessors().len() as u64,
                entry.split().len() as u64,
                entry.op(),
                entry.user(),
                entry.time().timestamp_secs(),
                entry.time().tz_offset_secs(),
                serde_json::to_string(entry.extra())?,
            ));
            for (index, predecessor) in entry.predecessors().iter().enumerate() {
                let primordial = changeset_primordials
                    .get(predecessor)
                    .expect("predecessor must have primordial");
                db_preds.push((
                    &self.repo_id,
                    entry.successor(),
                    index as u64,
                    predecessor,
                    primordial,
                ));
            }
            for (index, split_successor) in entry.split().iter().enumerate() {
                db_splits.push((
                    &self.repo_id,
                    entry.successor(),
                    index as u64,
                    split_successor,
                ));
            }
        }

        // Convert the numbers to references.
        let ref_db_entries: Vec<_> = db_entries
            .iter()
            .map(
                |&(
                    repo_id,
                    successor,
                    primordial,
                    ref extra_pred_count,
                    ref split_count,
                    op,
                    user,
                    ref time,
                    ref tz,
                    ref extra,
                )| {
                    (
                        repo_id,
                        successor,
                        primordial,
                        extra_pred_count,
                        split_count,
                        op,
                        user,
                        time,
                        tz,
                        extra.as_str(),
                    )
                },
            )
            .collect();
        let ref_db_preds: Vec<_> = db_preds
            .iter()
            .map(
                |&(repo_id, successor, ref index, predecessor, primordial)| {
                    (repo_id, successor, index, predecessor, primordial)
                },
            )
            .collect();
        let ref_db_splits: Vec<_> = db_splits
            .iter()
            .map(|&(repo_id, successor, ref index, split_successor)| {
                (repo_id, successor, index, split_successor)
            })
            .collect();

        let (txn, _) = AddChangesets::query_with_transaction(txn, db_csets.as_slice())
            .compat()
            .await?;
        let (txn, _) = AddEntries::query_with_transaction(txn, ref_db_entries.as_slice())
            .compat()
            .await?;
        let (txn, _) = AddPreds::query_with_transaction(txn, ref_db_preds.as_slice())
            .compat()
            .await?;
        let (txn, _) = AddSplits::query_with_transaction(txn, ref_db_splits.as_slice())
            .compat()
            .await?;

        txn.commit().compat().await?;

        Ok(())
    }

    /// Get an appropriate read connection for querying mutation information
    /// about some changesets.
    ///
    /// Checks if mutation information for all of the given changesets have been
    /// replicated to the read-only replica.  If so, returns a read connection
    /// to the replica.  If not all mutation information has been replicated, or
    /// if no mutation information for the changesets exists, returns a read
    /// connection to the master.
    ///
    /// Note that this only guarantees that mutation information for this
    /// commit and all of its predecessors are present.  For checking the
    /// successors of a commit, the replica may still lag behind.
    async fn read_connection_for_changesets(
        &self,
        changeset_ids: &HashSet<HgChangesetId>,
    ) -> Result<&Connection> {
        // Check if the replica is up-to-date with respect to all changesets we
        // are interested in.
        let changeset_ids: Vec<_> = changeset_ids.iter().collect();
        let count = CountChangesets::query(
            &self.connections.read_connection,
            &self.repo_id,
            changeset_ids.as_slice(),
        )
        .compat()
        .await?;
        if let Some((count,)) = count.into_iter().next() {
            if count as usize == changeset_ids.len() {
                // The replica knows all of the changesets, so use it.
                return Ok(&self.connections.read_connection);
            }
        }
        // The replica doesn't know all of the changesets, use the connection to
        // the master.
        Ok(&self.connections.read_master_connection)
    }

    /// Collect entries from the database into an entry set.
    async fn collect_entries<I>(
        &self,
        connection: &Connection,
        entry_set: &mut HgMutationEntrySet,
        rows: I,
    ) -> Result<()>
    where
        I: IntoIterator<Item = EntryRow>,
    {
        let mut to_fetch_split = Vec::new();
        for (
            successor,
            primordial,
            pred_count,
            p_seq,
            p_predecessor,
            p_primordial,
            split_count,
            op,
            user,
            timestamp,
            tz,
            extra,
        ) in rows
        {
            if !entry_set.changeset_primordials.contains_key(&successor) {
                entry_set
                    .changeset_primordials
                    .insert(successor.clone(), primordial.clone());
            }
            if !entry_set.changeset_primordials.contains_key(&p_predecessor) {
                entry_set
                    .changeset_primordials
                    .insert(p_predecessor.clone(), p_primordial.clone());
            }
            let entry = match entry_set.entries.entry(successor) {
                hash_map::Entry::Occupied(entry) => entry.into_mut(),
                hash_map::Entry::Vacant(entry) => {
                    let successor = entry.key().clone();
                    if split_count > 0 {
                        to_fetch_split.push(successor.clone());
                    }
                    entry.insert(HgMutationEntry::new(
                        successor,
                        SmallVec::with_capacity(pred_count as usize),
                        Vec::with_capacity(split_count as usize),
                        op,
                        user,
                        DateTime::from_timestamp(timestamp, tz)?,
                        serde_json::from_str(&extra)?,
                    ))
                }
            };
            entry
                .add_predecessor(p_seq, p_predecessor)
                .with_context(|| {
                    format!("Failed to collect predecessor {} for {}", p_seq, successor)
                })?;
        }
        if !to_fetch_split.is_empty() {
            let rows = SelectSplitsBySuccessor::query(
                connection,
                &self.repo_id,
                to_fetch_split.as_slice(),
            )
            .compat()
            .await?;
            for (successor, seq, split_successor) in rows {
                if let Some(entry) = entry_set.entries.get_mut(&successor) {
                    entry.add_split(seq, split_successor).with_context(|| {
                        format!(
                            "Failed to collect split successor {} for {}",
                            seq, successor
                        )
                    })?;
                }
            }
        }
        Ok(())
    }

    /// Fetch all mutation information where the given changesets are the
    /// successor and add it to the entry set.
    async fn fetch_by_successor(
        &self,
        connection: &Connection,
        entry_set: &mut HgMutationEntrySet,
        changesets: &HashSet<HgChangesetId>,
    ) -> Result<()> {
        let to_fetch: Vec<_> = changesets
            .iter()
            .filter(|hg_cs_id| !entry_set.entries.contains_key(hg_cs_id))
            .collect();
        let rows = SelectBySuccessorRef::query(connection, &self.repo_id, to_fetch.as_slice())
            .compat()
            .await?;
        self.collect_entries(connection, entry_set, rows).await?;
        Ok(())
    }

    /// Fetch all predecessor entries for the entries in the entry set.
    async fn fetch_all_predecessors(
        &self,
        connection: &Connection,
        entry_set: &mut HgMutationEntrySet,
    ) -> Result<()> {
        let mut fetched_primordials = HashSet::new();
        loop {
            let mut to_fetch = HashSet::new();
            for primordial in entry_set.changeset_primordials.values() {
                if !fetched_primordials.contains(primordial) {
                    to_fetch.insert(primordial.clone());
                }
            }
            if to_fetch.is_empty() {
                break;
            }
            let to_fetch: Vec<_> = to_fetch.into_iter().collect();
            let rows = SelectByPrimordial::query(connection, &self.repo_id, to_fetch.as_slice())
                .compat()
                .await?;
            self.collect_entries(connection, entry_set, rows).await?;
            fetched_primordials.extend(to_fetch);
        }
        Ok(())
    }
}

#[async_trait]
impl HgMutationStore for SqlHgMutationStore {
    async fn all_predecessors(
        &self,
        changeset_ids: HashSet<HgChangesetId>,
    ) -> Result<Vec<HgMutationEntry>> {
        let mut entry_set = HgMutationEntrySet::new();
        let connection = self.read_connection_for_changesets(&changeset_ids).await?;
        self.fetch_by_successor(connection, &mut entry_set, &changeset_ids)
            .await?;
        self.fetch_all_predecessors(connection, &mut entry_set)
            .await?;
        Ok(entry_set.into_all_predecessors(changeset_ids))
    }
}

queries! {
    write AddChangesets(
        values: (
            repo_id: RepositoryId,
            changeset_id: HgChangesetId,
        )
    ) {
        insert_or_ignore,
        "{insert_or_ignore} INTO hg_mutation_changesets (
            repo_id,
            changeset_id
        ) VALUES {values}"
    }

    write AddEntries(
        values: (
            repo_id: RepositoryId,
            successor: HgChangesetId,
            primordial: HgChangesetId,
            pred_count: u64,
            split_count: u64,
            op: str,
            user: str,
            timestamp: i64,
            tz: i32,
            extra: str,
        )
    ) {
        insert_or_ignore,
        "{insert_or_ignore} INTO hg_mutation_info (
            repo_id,
            successor,
            primordial,
            pred_count,
            split_count,
            op,
            user,
            timestamp,
            tz,
            extra
        ) VALUES {values}"
    }

    write AddPreds(
        values: (
            repo_id: RepositoryId,
            successor: HgChangesetId,
            seq: u64,
            predecessor: HgChangesetId,
            primordial: HgChangesetId
        )
    ) {
        insert_or_ignore,
        "{insert_or_ignore} INTO hg_mutation_preds (
            repo_id,
            successor,
            seq,
            predecessor,
            primordial
        ) VALUES {values}"
    }

    write AddSplits(
        values: (
            repo_id: RepositoryId,
            successor: HgChangesetId,
            seq: u64,
            split_successor: HgChangesetId,
        )
    ) {
        insert_or_ignore,
        "{insert_or_ignore} INTO hg_mutation_splits (
            repo_id,
            successor,
            seq,
            split_successor
        ) VALUES {values}"
    }

    read CountChangesets(repo_id: RepositoryId, >list cs_id: &HgChangesetId) -> (i64) {
        "SELECT
            COUNT(*)
        FROM
            hg_mutation_changesets
        WHERE repo_id = {repo_id} AND changeset_id in {cs_id}"
    }

    read SelectBySuccessorRef(repo_id: RepositoryId, >list cs_id: &HgChangesetId) -> (
        HgChangesetId,
        HgChangesetId,
        u64,
        u64,
        HgChangesetId,
        HgChangesetId,
        u64,
        String,
        String,
        i64,
        i32,
        String
    ) {
        "SELECT
            m.successor, m.primordial,
            m.pred_count, p.seq, p.predecessor, p.primordial,
            m.split_count,
            m.op, m.user, m.timestamp, m.tz, m.extra
        FROM
            hg_mutation_info m
            LEFT JOIN hg_mutation_preds p on m.successor = p.successor
        WHERE m.repo_id = {repo_id} AND m.successor IN {cs_id}
        ORDER BY m.successor, p.seq ASC"
    }

    read SelectByPrimordial(repo_id: RepositoryId, >list cs_id: HgChangesetId) -> (
        HgChangesetId,
        HgChangesetId,
        u64,
        u64,
        HgChangesetId,
        HgChangesetId,
        u64,
        String,
        String,
        i64,
        i32,
        String
    ) {
        "SELECT
            m.successor, m.primordial,
            m.pred_count, p.seq, p.predecessor, p.primordial,
            m.split_count,
            m.op, m.user, m.timestamp, m.tz, m.extra
        FROM
            hg_mutation_info m
            LEFT JOIN hg_mutation_preds p on m.successor = p.successor
        WHERE m.repo_id = {repo_id} AND m.primordial IN {cs_id}
        ORDER BY m.successor, p.seq ASC"
    }

    read SelectSplitsBySuccessor(repo_id: RepositoryId, >list cs_id: HgChangesetId) -> (
        HgChangesetId,
        u64,
        HgChangesetId,
    ) {
        "SELECT successor, seq, split_successor
        FROM hg_mutation_splits
        WHERE repo_id = {repo_id} AND successor IN {cs_id}
        ORDER BY successor, seq ASC"
    }
}
