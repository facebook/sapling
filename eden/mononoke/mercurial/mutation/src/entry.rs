/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::{hash_map, HashMap, HashSet};

use anyhow::{anyhow, Result};
use mercurial_types::HgChangesetId;
use mononoke_types::DateTime;
use smallvec::SmallVec;

/// Record of a Mercurial mutation operation (e.g. amend or rebase).
#[derive(Clone, Debug, PartialEq)]
pub struct HgMutationEntry {
    /// The commit that resulted from the mutation operation.
    successor: HgChangesetId,
    /// The commits that were mutated to create the successor.
    ///
    /// There may be multiple predecessors, e.g. if the commits were folded.
    predecessors: SmallVec<[HgChangesetId; 1]>,
    /// Other commits that were created by the mutation operation splitting the predecessors.
    ///
    /// Where a commit is split into two or more commits, the successor will be the final commit,
    /// and this list will contain the other commits.
    split: Vec<HgChangesetId>,
    /// The name of the operation.
    op: String,
    /// The user who performed the mutation operation.  This may differ from the commit author.
    user: String,
    /// The time of the mutation operation.  This may differ from the commit time.
    time: DateTime,
    /// Extra information about this mutation operation.
    extra: Vec<(String, String)>,
}

impl HgMutationEntry {
    pub fn new(
        successor: HgChangesetId,
        predecessors: SmallVec<[HgChangesetId; 1]>,
        split: Vec<HgChangesetId>,
        op: String,
        user: String,
        time: DateTime,
        extra: Vec<(String, String)>,
    ) -> Self {
        Self {
            successor,
            predecessors,
            split,
            op,
            user,
            time,
            extra,
        }
    }

    pub fn successor(&self) -> &HgChangesetId {
        &self.successor
    }

    pub fn predecessors(&self) -> &[HgChangesetId] {
        self.predecessors.as_slice()
    }

    pub fn split(&self) -> &[HgChangesetId] {
        self.split.as_slice()
    }

    pub fn op(&self) -> &str {
        &self.op
    }

    pub fn user(&self) -> &str {
        &self.user
    }

    pub fn time(&self) -> &DateTime {
        &self.time
    }

    pub fn extra(&self) -> &[(String, String)] {
        self.extra.as_slice()
    }

    /// Add the next predecessor to the entry.
    pub(crate) fn add_predecessor(&mut self, index: u64, pred: HgChangesetId) -> Result<()> {
        // This method is used when progressively loading entries from the
        // database. Each predecessor is received in a separate row, and we may
        // receive each predecessor multiple times.  They should always be
        // received in order, so only extend the list of predecessors if the
        // index of the new predecessor matches the expected index of the next
        // predecessor.
        let expected_index = self.predecessors.len() as u64;
        if index > expected_index {
            // We have received a predecessor past the end of the current
            // predecessor list.  This probably means the predecessor table is
            // missing a row.
            return Err(anyhow!(
                "Unexpected out-of-order predecessor {}, expected index {}",
                pred,
                expected_index
            ));
        }
        if index == expected_index {
            self.predecessors.push(pred);
        }
        Ok(())
    }

    /// Add the next split to the entry.
    pub(crate) fn add_split(&mut self, index: u64, split: HgChangesetId) -> Result<()> {
        // This method is used when progressively loading entries from the
        // database. Each split successor is received in a separate row, and we
        // may receive each split successor multiple times.  They should always
        // be received in order, so only extend the list of split successors if
        // the index of the new split successor matches the expected index of
        // the next split successor.
        let expected_index = self.split.len() as u64;
        if index > expected_index {
            // We have received a split successor past the end of the current
            // split successor list.  This probably means the split table is
            // missing a row.
            return Err(anyhow!(
                "Unexpected out-of-order split successor {}, expected index {}",
                split,
                expected_index
            ));
        }
        if index == expected_index {
            self.split.push(split);
        }
        Ok(())
    }
}

pub(crate) struct HgMutationEntrySet {
    // The loaded entries, indexed by successor.
    pub(crate) entries: HashMap<HgChangesetId, HgMutationEntry>,

    // The known primordial changeset ID for any changeset ID.
    pub(crate) changeset_primordials: HashMap<HgChangesetId, HgChangesetId>,
}

/// Result of a request to add new entries to an entry set.
pub(crate) struct HgMutationEntrySetAdded {
    /// The keys for entries that were successfully added.
    pub(crate) added: Vec<HgChangesetId>,

    /// The changeset IDs of changesets that could not be added because their
    /// primordial changesets are not known.
    pub(crate) missing_primordials: Vec<HgChangesetId>,

    /// The remaining entries that were not added.
    pub(crate) remaining_entries: HashMap<HgChangesetId, HgMutationEntry>,
}

impl HgMutationEntrySet {
    pub(crate) fn new() -> Self {
        Self {
            entries: HashMap::new(),
            changeset_primordials: HashMap::new(),
        }
    }

    /// Add new entries for the given ids to the entry set.
    ///
    /// New entries associated with `new_ids` are moved from `new_entries` into
    /// the entry set, provided that their primordial IDs are already known.
    ///
    /// Returns the changeset IDs for the entries that were added and the
    /// changeset IDs which could not be added because their primordial IDs are
    /// not known.
    pub(crate) fn add_entries<'a>(
        &mut self,
        mut new_entries: HashMap<HgChangesetId, HgMutationEntry>,
        new_ids: impl IntoIterator<Item = &'a HgChangesetId>,
    ) -> Result<HgMutationEntrySetAdded> {
        let mut added = Vec::new();
        let mut missing_primordials = Vec::new();

        for changeset_id in new_ids {
            // Check if we already have an entry for this changeset.
            if self.entries.contains_key(changeset_id) {
                continue;
            }

            // This changeset does not have a stored entry for which it is the
            // successor.
            match new_entries.entry(*changeset_id) {
                hash_map::Entry::Vacant(_) => {
                    // This changeset is a new primordial changeset.
                    added.push(*changeset_id);
                }
                hash_map::Entry::Occupied(entry) => {
                    // This changeset has a new entry to store.  See if all its
                    // predecessors' primordials are known.
                    for predecessor_id in entry.get().predecessors().iter() {
                        if !self.changeset_primordials.contains_key(predecessor_id) {
                            missing_primordials.push(*predecessor_id);
                        }
                    }
                    // The first predecessor's primordial should be
                    // propagated to this changeset.
                    let predecessor_id =
                        entry.get().predecessors().iter().next().ok_or_else(|| {
                            anyhow!(
                                "Mutation entry for {} has no predecessors",
                                entry.get().successor()
                            )
                        })?;
                    match self.changeset_primordials.get(predecessor_id) {
                        Some(&primordial_id) => {
                            // The entry's first predecessor's primordial is known.
                            // Move the new entry over to the entry set, and
                            // copy the first predecessor's primordial.
                            self.entries.insert(*changeset_id, entry.remove());
                            self.changeset_primordials
                                .insert(*changeset_id, primordial_id);
                            added.push(*changeset_id);
                        }
                        None => {
                            // The entry's first predecessor's primordial is not
                            // known yet.  We need to include this changeset in
                            // the primordial search as well, so that it will be
                            // filled in once it is found. Add this changeset to
                            // the missing set.
                            missing_primordials.push(*changeset_id)
                        }
                    }
                }
            }
        }

        Ok(HgMutationEntrySetAdded {
            added,
            missing_primordials,
            remaining_entries: new_entries,
        })
    }

    /// Add new entries for the given ids and their predecessors to the entry
    /// set.
    ///
    /// New entries associated with `new_ids` are moved from `new_entries` into
    /// the entry set.
    ///
    /// For these new entries, the primordial changeset IDs are filled in by
    /// searching all predecessors for either a known primordial, or a new
    /// primordial.
    ///
    /// Returns all changeset IDs that were added.
    pub(crate) fn add_entries_and_find_primordials<'a>(
        &mut self,
        mut new_entries: HashMap<HgChangesetId, HgMutationEntry>,
        new_ids: impl IntoIterator<Item = &'a HgChangesetId>,
    ) -> Result<Vec<HgChangesetId>> {
        let mut added = Vec::new();

        // We will allocate primordials by seeking back to the first commit with
        // a known primordial, or the primordial commit itself.
        //
        // The candidate_primordials vector will contain the current set of
        // commits that could be primordial, along with the commits that they
        // would be primordial for, if they are primordial, which is initially
        // empty.
        let mut candidate_primordials: Vec<_> = new_ids
            .into_iter()
            .map(|changeset_id| (*changeset_id, Vec::new()))
            .collect();

        // Look at each candidate primoridial.  If it really is primoridial, or
        // we know what its primordial should be, then we are done.  Otherwise,
        // expand it to its predecessors, which are the new candidate
        // primordials.
        while let Some((candidate_primordial_id, mut changeset_ids)) = candidate_primordials.pop() {
            let primordial_id = if let Some(primordial_id) =
                self.changeset_primordials.get(&candidate_primordial_id)
            {
                // We have reached a changeset with a known primordial
                *primordial_id
            } else if let Some(entry) = new_entries.get(&candidate_primordial_id) {
                // This is not the primordial commit, we must look at its
                // predecessors.  The first predecessor is the new candidate
                // primordial commit for itself, this changeset and its
                // successors.  The subsequent predecessors are all candidate
                // primordial commits for themselves.
                if entry.predecessors().is_empty() {
                    return Err(anyhow!(
                        "Mutation entry for {} has no predecessors",
                        entry.successor()
                    ));
                }
                changeset_ids.push(candidate_primordial_id);
                for predecessor in entry.predecessors() {
                    candidate_primordials.push((*predecessor, changeset_ids));
                    changeset_ids = Vec::new();
                }
                continue;
            } else {
                // We have reached a new primordial changeset.
                changeset_ids.push(candidate_primordial_id);
                candidate_primordial_id
            };

            // Now we know the primordial ID, apply it to all of the associated
            // changesets.
            for changeset_id in changeset_ids {
                self.changeset_primordials
                    .insert(changeset_id, primordial_id);
                if let Some(new_entry) = new_entries.remove(&changeset_id) {
                    self.entries.insert(changeset_id, new_entry);
                }
                added.push(changeset_id);
            }
        }

        Ok(added)
    }

    /// Extracts all entries for predecessors of the given changeset ids.
    pub(crate) fn into_all_predecessors(
        mut self,
        changeset_ids: HashSet<HgChangesetId>,
    ) -> Vec<HgMutationEntry> {
        let mut changeset_ids: Vec<_> = changeset_ids.into_iter().collect();
        let mut entries = Vec::new();
        while let Some(changeset_id) = changeset_ids.pop() {
            // See if we have an entry for this changeset_id.
            if let Some(entry) = self.entries.remove(&changeset_id) {
                // Add all of this entry's predecessors to the queue of
                // additional changesets we will need to process.  Push
                // predecessors in reverse order so that we process them in
                // forwards order.
                for predecessor_id in entry.predecessors().iter().rev() {
                    // Check if there is an entry for the predecessor.  If there
                    // isn't one, or if we have already processed this
                    // predecessor, don't waste time enqueuing it.
                    if self.entries.contains_key(predecessor_id) {
                        changeset_ids.push(predecessor_id.clone());
                    }
                }
                entries.push(entry);
            }
        }
        entries
    }
}
