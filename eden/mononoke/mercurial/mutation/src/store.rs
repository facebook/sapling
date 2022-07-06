/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::hash_map;
use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use context::PerfCounterType;
use futures::future;
use futures::future::FutureExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use itertools::Itertools;
use mercurial_types::HgChangesetId;
use mononoke_types::RepositoryId;
use slog::debug;
use sql::queries;
use sql::Connection;
use sql_ext::SqlConnections;

use crate::entry::HgMutationEntry;
use crate::entry::HgMutationEntrySet;
use crate::entry::HgMutationEntrySetAdded;
use crate::HgMutationStore;

/// To avoid overloading the database with too many changesets in a single
/// select, we chunk selects to this size.
const SELECT_CHUNK_SIZE: usize = 100;

pub struct SqlHgMutationStore {
    repo_id: RepositoryId,
    connections: SqlConnections,
    mutation_chain_limit: usize,
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
    pub fn new(
        repo_id: RepositoryId,
        connections: SqlConnections,
        mutation_chain_limit: usize,
    ) -> Self {
        Self {
            repo_id,
            connections,
            mutation_chain_limit,
        }
    }

    /// Store new mutation entries in the mutation store.
    ///
    /// Stores the mutation entries for the changesets in `changeset_ids` into
    /// the store.
    ///
    /// Mutation entries in `entry_set` for which these changesets are the
    /// successor will be written into the store.  Other changesets (which are
    /// expected to be primordial changesets) will just be recorded as having
    /// been added to the store.
    async fn store<'a>(
        &self,
        ctx: &CoreContext,
        entry_set: &HgMutationEntrySet,
        changeset_ids: impl IntoIterator<Item = &'a HgChangesetId>,
    ) -> Result<()> {
        let txn = self
            .connections
            .write_connection
            .start_transaction()
            .await?;

        let mut db_csets = Vec::new();
        let mut db_entries = Vec::new();
        let mut db_preds = Vec::new();
        let mut db_splits = Vec::new();
        for changeset_id in changeset_ids.into_iter() {
            db_csets.push((&self.repo_id, changeset_id));
            let entry = match entry_set.entries.get(changeset_id) {
                Some(entry) => entry,
                None => continue,
            };
            let primordial = entry_set
                .changeset_primordials
                .get(entry.successor())
                .ok_or_else(|| {
                    anyhow!(
                        "failed to determine primordial commit for successor {}",
                        entry.successor()
                    )
                })?;
            db_entries.push((
                &self.repo_id,
                entry.successor(),
                primordial,
                entry.predecessors().len() as u64,
                entry.split().len() as u64,
                entry.op(),
                entry.user(),
                entry.timestamp(),
                entry.timezone(),
                serde_json::to_string(entry.extra())?,
            ));
            for (index, predecessor) in entry.predecessors().iter().enumerate() {
                let primordial = entry_set
                    .changeset_primordials
                    .get(predecessor)
                    .ok_or_else(|| {
                        anyhow!(
                            "failed to determine primordial commit for predecessor {}",
                            predecessor
                        )
                    })?;
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

        // Sort the entries so that they are inserted in a stable
        // order.  This prevents deadlocks where one transaction is
        // adding commits "A, B, C" and gains the lock on A while
        // another transaction is adding "C, D, A" and gains the lock
        // on C.
        db_csets.sort_unstable();
        db_entries.sort_unstable();
        db_preds.sort_unstable();
        db_splits.sort_unstable();

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

        let (txn, _) = AddChangesets::query_with_transaction(txn, db_csets.as_slice()).await?;
        let (txn, _) = AddEntries::query_with_transaction(txn, ref_db_entries.as_slice()).await?;
        let (txn, _) = AddPreds::query_with_transaction(txn, ref_db_preds.as_slice()).await?;
        let (txn, _) = AddSplits::query_with_transaction(txn, ref_db_splits.as_slice()).await?;

        txn.commit().await?;

        debug!(
            ctx.logger(),
            "Mutation store added {} entries for {} changesets",
            db_entries.len(),
            db_csets.len()
        );
        ctx.perf_counters().add_to_counter(
            PerfCounterType::HgMutationStoreNumAdded,
            db_entries.len() as i64,
        );

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
        if changeset_ids.is_empty() {
            // There are no interesting changesets, so just use the replica.
            return Ok(&self.connections.read_connection);
        }
        let count = CountChangesets::query(
            &self.connections.read_connection,
            &self.repo_id,
            changeset_ids.as_slice(),
        )
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
                        Vec::with_capacity(pred_count as usize),
                        Vec::with_capacity(split_count as usize),
                        op,
                        user,
                        timestamp,
                        tz,
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
        let chunks = changesets
            .iter()
            .cloned()
            .filter(|hg_cs_id| !entry_set.entries.contains_key(hg_cs_id))
            .chunks(SELECT_CHUNK_SIZE)
            .into_iter()
            .map(|chunk| chunk.collect::<Vec<_>>())
            .collect::<Vec<_>>();

        let chunk_rows = stream::iter(chunks.into_iter().map(move |chunk| async move {
            SelectBySuccessor::query(connection, &self.repo_id, chunk.as_slice())
                .await
                .with_context(|| format!("Error fetching successors: {:?}", chunk))
        }))
        .buffered(10)
        .try_collect::<Vec<_>>()
        .await?;

        self.collect_entries(connection, entry_set, chunk_rows.into_iter().flatten())
            .await?;
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
            fetched_primordials.extend(to_fetch.iter().copied());

            let obtained_rows = stream::iter(to_fetch.into_iter().map(|changeset| async move {
                let (mut primordial_rows, successor_rows) = future::try_join(
                    SelectByPrimordial::query(
                        connection,
                        &self.repo_id,
                        &self.mutation_chain_limit,
                        &[changeset],
                    )
                    .map(|r| {
                        r.with_context(|| format!("Error fetching primordials: {:?}", &changeset))
                    }),
                    SelectBySuccessor::query(connection, &self.repo_id, &[changeset]).map(|r| {
                        r.with_context(|| format!("Error fetching successors: {:?}", &changeset))
                    }),
                )
                .await?;
                // If we reach the limit and ended up cutting some predecessors of a mutation
                // then remove the mutation all together.
                if primordial_rows.len() == self.mutation_chain_limit {
                    if let Some(last) = primordial_rows.iter().last() {
                        let (pred_count, seq) = (last.2, last.3);
                        if seq + 1 != pred_count {
                            // we don't have the whole of the last entry, remove it
                            let last_count = primordial_rows
                                .iter()
                                .rev()
                                .take_while(|row| row.0 == last.0) //0 -> successor
                                .count();
                            primordial_rows.truncate(primordial_rows.len() - last_count)
                        }
                    }
                }
                Ok::<_, Error>(
                    primordial_rows
                        .into_iter()
                        .chain(successor_rows.into_iter())
                        .collect::<Vec<_>>(),
                )
            }))
            .buffered(100)
            .try_collect::<Vec<_>>()
            .await?;

            self.collect_entries(connection, entry_set, obtained_rows.into_iter().flatten())
                .await?;
        }
        Ok(())
    }
}

#[async_trait]
impl HgMutationStore for SqlHgMutationStore {
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn add_entries(
        &self,
        ctx: &CoreContext,
        new_changeset_ids: HashSet<HgChangesetId>,
        entries: Vec<HgMutationEntry>,
    ) -> Result<()> {
        if new_changeset_ids.is_empty() {
            // Nothing to do.
            return Ok(());
        }

        let mut entry_set = HgMutationEntrySet::new();
        let entries: HashMap<_, _> = entries
            .into_iter()
            .filter(|entry| {
                // Filter out entries that are self-referential (these are
                // revive obsmarkers incorrectly converted to mutation entries).
                let successor = entry.successor();
                !entry
                    .predecessors()
                    .iter()
                    .any(|predecessor| predecessor == successor)
            })
            .map(|entry| (*entry.successor(), entry))
            .collect();

        // First fetch all the new changesets and their immediate predecessors.
        // Most of the time this will be enough, as we can learn the primordial
        // changeset ID from the immediate predecessor.
        let mut changeset_ids = HashSet::with_capacity(new_changeset_ids.len() * 2);
        for changeset_id in &new_changeset_ids {
            changeset_ids.insert(changeset_id.clone());
            if let Some(entry) = entries.get(changeset_id) {
                changeset_ids.extend(entry.predecessors().iter().cloned());
            }
        }
        self.fetch_by_successor(
            &self.connections.read_master_connection,
            &mut entry_set,
            &changeset_ids,
        )
        .await?;

        // Attempt to add these new entries to the entry set, and find out which
        // changesets have missing primordial IDs.
        let HgMutationEntrySetAdded {
            mut added,
            missing_primordials,
            remaining_entries,
        } = entry_set.add_entries(entries, &new_changeset_ids)?;

        debug!(
            ctx.logger(),
            "Mutation store fast path found primordial changesets for {} changesets ({} remaining)",
            added.len(),
            missing_primordials.len(),
        );

        // If any entries were missing their primoridal IDs, we will need to
        // fetch them and all of their predecessors in order to find which
        // predecessor entries are missing and what their primordial commits
        // are.
        if !missing_primordials.is_empty() {
            // Find all predecessors of the entries that are missing in the
            // store by iterating over the entries provided by the client,
            // starting with the missing ones, and fetching all of their
            // predecessors.
            let mut predecessor_stack = missing_primordials.clone();
            let mut predecessor_ids = HashSet::new();
            while let Some(missing_id) = predecessor_stack.pop() {
                if let Some(entry) = remaining_entries.get(&missing_id) {
                    for predecessor_id in entry.predecessors() {
                        if predecessor_ids.insert(*predecessor_id) {
                            predecessor_stack.push(*predecessor_id);
                        }
                    }
                }
            }

            debug!(
                ctx.logger(),
                "Mutation store slow path fetching {} additional entries",
                predecessor_ids.len(),
            );

            self.fetch_by_successor(
                &self.connections.read_master_connection,
                &mut entry_set,
                &predecessor_ids,
            )
            .await?;

            // Add these new entries, finding their primordials in the process.
            added.extend(
                entry_set
                    .add_entries_and_find_primordials(remaining_entries, &missing_primordials)?,
            );
        }

        // Store the new entries.
        self.store(ctx, &entry_set, &added).await?;
        Ok(())
    }

    /// Get all predecessor information for the given changeset id, keyed by
    /// the successor changeset id.
    ///
    /// Returns all entries that describe the mutation history of the commits.
    /// keyed by the successor changeset ids.
    async fn all_predecessors_by_changeset(
        &self,
        ctx: &CoreContext,
        changeset_ids: HashSet<HgChangesetId>,
    ) -> Result<HashMap<HgChangesetId, Vec<HgMutationEntry>>> {
        if changeset_ids.is_empty() {
            // Nothing to fetch
            return Ok(HashMap::new());
        }

        let mut entry_set = HgMutationEntrySet::new();
        let connection = self.read_connection_for_changesets(&changeset_ids).await?;
        self.fetch_by_successor(connection, &mut entry_set, &changeset_ids)
            .await?;
        self.fetch_all_predecessors(connection, &mut entry_set)
            .await?;
        let changeset_count = changeset_ids.len();
        let entries = entry_set.into_all_predecessors_by_changeset(changeset_ids);
        debug!(
            ctx.logger(),
            "Mutation store fetched {} entries for {} changesets",
            entries.len(),
            changeset_count,
        );
        ctx.perf_counters().add_to_counter(
            PerfCounterType::HgMutationStoreNumFetched,
            entries.len() as i64,
        );
        Ok(entries)
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
        WHERE repo_id = {repo_id} AND changeset_id IN {cs_id}"
    }

    read SelectBySuccessor(repo_id: RepositoryId, >list cs_id: HgChangesetId) -> (
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
            hg_mutation_info m LEFT JOIN hg_mutation_preds p
            ON m.repo_id = p.repo_id AND m.successor = p.successor
        WHERE m.repo_id = {repo_id} AND m.successor IN {cs_id}
        ORDER BY m.successor, p.seq ASC"
    }

    read SelectByPrimordial(repo_id: RepositoryId, mut_lim: usize, >list cs_id: HgChangesetId) -> (
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
            hg_mutation_info m LEFT JOIN hg_mutation_preds p
            ON m.repo_id = p.repo_id AND m.successor = p.successor
        WHERE m.repo_id = {repo_id} AND m.primordial IN {cs_id}
        ORDER BY m.id DESC, p.seq ASC
        LIMIT {mut_lim}"
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
