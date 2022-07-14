/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::hash_map;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Read;

use abomonation_derive::Abomonation;
use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use edenapi_types::Extra;
use edenapi_types::HgMutationEntryContent;
use hg_mutation_entry_thrift as thrift;
use mercurial_types::HgChangesetId;
use mercurial_types::HgNodeHash;
use types::mutation::MutationEntry;
use types::HgId;

use crate::grouper::Grouper;

/// Record of a Mercurial mutation operation (e.g. amend or rebase).
#[derive(Abomonation, Clone, Debug, Hash, Eq, PartialEq)]
pub struct HgMutationEntry {
    /// The commit that resulted from the mutation operation.
    successor: HgChangesetId,
    /// The commits that were mutated to create the successor.
    ///
    /// There may be multiple predecessors, e.g. if the commits were folded.
    predecessors: Vec<HgChangesetId>,
    /// Other commits that were created by the mutation operation splitting the predecessors.
    ///
    /// Where a commit is split into two or more commits, the successor will be the final commit,
    /// and this list will contain the other commits.
    split: Vec<HgChangesetId>,
    /// The name of the operation.
    op: String,
    /// The user who performed the mutation operation.  This may differ from the commit author.
    user: String,
    /// The timestamp of the mutation operation.  This may differ from the commit time.
    timestamp: i64,
    /// The timezone offset of the mutation operation.  This may differ from the commit time.
    timezone: i32,
    /// Extra information about this mutation operation.
    extra: Vec<(String, String)>,
}

impl HgMutationEntry {
    pub fn new(
        successor: HgChangesetId,
        predecessors: Vec<HgChangesetId>,
        split: Vec<HgChangesetId>,
        op: String,
        user: String,
        timestamp: i64,
        timezone: i32,
        extra: Vec<(String, String)>,
    ) -> Self {
        Self {
            successor,
            predecessors,
            split,
            op,
            user,
            timestamp,
            timezone,
            extra,
        }
    }

    pub fn deserialize(r: &mut dyn Read) -> Result<Self> {
        HgMutationEntry::try_from(MutationEntry::deserialize(r)?)
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

    pub fn timestamp(&self) -> i64 {
        self.timestamp
    }

    pub fn timezone(&self) -> i32 {
        self.timezone
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

    pub(crate) fn from_thrift(entry: thrift::HgMutationEntry) -> Result<Self> {
        let preds = entry
            .predecessors
            .into_iter()
            .map(HgChangesetId::from_thrift)
            .collect::<Result<Vec<HgChangesetId>>>()?;
        let split = entry
            .split
            .into_iter()
            .map(HgChangesetId::from_thrift)
            .collect::<Result<Vec<HgChangesetId>>>()?;
        let extra = entry
            .extra
            .into_iter()
            .map(|e| (e.key, e.value))
            .collect::<Vec<(String, String)>>();
        Ok(HgMutationEntry::new(
            HgChangesetId::from_thrift(entry.successor)?,
            preds,
            split,
            entry.op,
            entry.user,
            entry.timestamp,
            entry.timezone,
            extra,
        ))
    }

    pub(crate) fn into_thrift(self) -> thrift::HgMutationEntry {
        thrift::HgMutationEntry {
            successor: HgChangesetId::into_thrift(self.successor),
            predecessors: self
                .predecessors
                .into_iter()
                .map(HgChangesetId::into_thrift)
                .collect(),
            split: self
                .split
                .into_iter()
                .map(HgChangesetId::into_thrift)
                .collect(),
            op: self.op,
            user: self.user,
            timestamp: self.timestamp,
            timezone: self.timezone,
            extra: self
                .extra
                .into_iter()
                .map(|(key, value)| thrift::ExtraProperty { key, value })
                .collect(),
        }
    }
}

// Conversion from client mutation entry
impl TryFrom<MutationEntry> for HgMutationEntry {
    type Error = Error;

    fn try_from(entry: MutationEntry) -> Result<HgMutationEntry> {
        let entry = HgMutationEntry {
            successor: HgChangesetId::new(HgNodeHash::from(entry.succ)),
            predecessors: entry
                .preds
                .into_iter()
                .map(HgNodeHash::from)
                .map(HgChangesetId::new)
                .collect(),
            split: entry
                .split
                .into_iter()
                .map(HgNodeHash::from)
                .map(HgChangesetId::new)
                .collect(),
            op: entry.op,
            user: entry.user,
            timestamp: entry.time,
            timezone: entry.tz,
            extra: entry
                .extra
                .into_iter()
                .map(|(key, value)| -> Result<(String, String), Error> {
                    Ok((
                        String::from_utf8(key.into())?,
                        String::from_utf8(value.into())?,
                    ))
                })
                .collect::<Result<_>>()?,
        };
        Ok(entry)
    }
}

// Conversion to client mutation entry
impl Into<MutationEntry> for HgMutationEntry {
    fn into(self: HgMutationEntry) -> MutationEntry {
        MutationEntry {
            succ: self.successor.into_nodehash().into(),
            preds: self
                .predecessors
                .into_iter()
                .map(HgChangesetId::into_nodehash)
                .map(HgId::from)
                .collect(),
            split: self
                .split
                .into_iter()
                .map(HgChangesetId::into_nodehash)
                .map(HgId::from)
                .collect(),
            op: self.op,
            user: self.user,
            time: self.timestamp,
            tz: self.timezone,
            extra: self
                .extra
                .into_iter()
                .map(|(key, value)| {
                    (
                        key.into_bytes().into_boxed_slice(),
                        value.into_bytes().into_boxed_slice(),
                    )
                })
                .collect(),
        }
    }
}

impl TryFrom<HgMutationEntryContent> for HgMutationEntry {
    type Error = Error;
    fn try_from(mutation: HgMutationEntryContent) -> Result<Self> {
        let successor = HgChangesetId::new(HgNodeHash::from(mutation.successor));
        let predecessors = mutation
            .predecessors
            .into_iter()
            .map(HgNodeHash::from)
            .map(HgChangesetId::new)
            .collect::<Vec<_>>();
        let split = mutation
            .split
            .into_iter()
            .map(HgNodeHash::from)
            .map(HgChangesetId::new)
            .collect::<Vec<_>>();
        let op = mutation.op;
        let user = std::str::from_utf8(&mutation.user)?.to_string();
        let timestamp = mutation.time;
        let timezone = mutation.tz;
        let exta = mutation
            .extras
            .into_iter()
            .map(|extra| {
                Ok((
                    std::str::from_utf8(&extra.key)?.to_string(),
                    std::str::from_utf8(&extra.value)?.to_string(),
                ))
            })
            .collect::<Result<_, Error>>()?;

        Ok(HgMutationEntry::new(
            successor,
            predecessors,
            split,
            op,
            user,
            timestamp,
            timezone,
            exta,
        ))
    }
}
impl From<HgMutationEntry> for HgMutationEntryContent {
    fn from(mutation: HgMutationEntry) -> Self {
        let successor = mutation.successor.into();
        let predecessors = mutation
            .predecessors
            .into_iter()
            .map(Into::into)
            .collect::<Vec<_>>();
        let split = mutation
            .split
            .into_iter()
            .map(Into::into)
            .collect::<Vec<_>>();
        let op = mutation.op;
        let user = mutation.user.into_bytes();
        let time = mutation.timestamp;
        let tz = mutation.timezone;
        let extras = mutation
            .extra
            .into_iter()
            .map(|(key, value)| Extra {
                key: key.into_bytes(),
                value: value.into_bytes(),
            })
            .collect();

        Self {
            successor,
            predecessors,
            split,
            op,
            user,
            time,
            tz,
            extras,
        }
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
    /// If the set of entries contains cycles, then it may not be possible to
    /// determine an appropriate primordial commit.  In which case, the
    /// entries that form a cycle will be ignored.
    ///
    /// Returns all changeset IDs that were added.
    pub(crate) fn add_entries_and_find_primordials<'a>(
        &mut self,
        mut new_entries: HashMap<HgChangesetId, HgMutationEntry>,
        new_ids: impl IntoIterator<Item = &'a HgChangesetId>,
    ) -> Result<Vec<HgChangesetId>> {
        // The changesets IDs that were added
        let mut added = Vec::new();

        // We will allocate primordials by seeking back to the first commit with
        // a known primordial, or the primordial commit itself.
        //
        // Commits that are yet to be processed are candidates.
        let mut candidates: Vec<_> = new_ids.into_iter().copied().collect();

        // Commits that are queued to be processed or have been
        // processed (to break cycles).
        let mut seen: HashSet<_> = candidates.iter().copied().collect();

        // A Grouper to group commits together into primordial groups.
        let mut grouper = Grouper::new();

        // Look at each candidate.  If it is primoridial, or we know
        // what its primordial should be, then we are done.  Otherwise,
        // expand it to its predecessors, which are the new candidates.
        while let Some(candidate) = candidates.pop() {
            if let Some(primordial_id) = self.changeset_primordials.get(&candidate) {
                // We have reached a changeset with a known primordial
                grouper.set_primordial(candidate, *primordial_id);
            } else if let Some(entry) = new_entries.get(&candidate) {
                // This is not the primordial commit, we must look at its
                // predecessors.
                let predecessors = entry.predecessors();
                if let Some(first) = predecessors.first() {
                    // Merge this candidate's group with the group of its
                    // first predecessor: they will have the same primordial.
                    grouper.merge(candidate, *first);
                } else {
                    return Err(anyhow!(
                        "Mutation entry for {} has no predecessors",
                        entry.successor()
                    ));
                }
                for &predecessor in predecessors.iter() {
                    if seen.insert(predecessor) {
                        candidates.push(predecessor);
                    }
                }
            } else {
                // We have reached a new primordial changeset.
                grouper.set_primordial(candidate, candidate);
                added.push(candidate);
            };
        }

        // Apply calculated primordials to their groups, and work out
        // which entries should be moved into the store.
        let mut move_entries = Vec::new();
        for (primordial, members) in grouper.groups() {
            if let Some(primordial) = primordial {
                // Apply this primordial changeset to all of the members
                // of this group.
                for changeset_id in members {
                    if self
                        .changeset_primordials
                        .insert(changeset_id, primordial)
                        .is_none()
                    {
                        move_entries.push(changeset_id);
                    }
                }
            }
        }

        // Move valid entries into the store.
        for changeset_id in move_entries.into_iter() {
            if let Some(new_entry) = new_entries.remove(&changeset_id) {
                if new_entry
                    .predecessors()
                    .iter()
                    .all(|predecessor| self.changeset_primordials.contains_key(predecessor))
                {
                    // We have found primordials for all predecessors of this
                    // entry, so we can add it.
                    self.entries.insert(changeset_id, new_entry);
                    added.push(changeset_id);
                }
            }
        }

        Ok(added)
    }

    /// Extracts all entries for predecessors of the given changeset ids.
    pub(crate) fn into_all_predecessors_by_changeset(
        self,
        successor_ids: HashSet<HgChangesetId>,
    ) -> HashMap<HgChangesetId, Vec<HgMutationEntry>> {
        let mut mutation_history = HashMap::new();
        for successor_id in successor_ids {
            let mut entries = Vec::new();
            let mut processed = HashSet::new();
            let mut changeset_ids = vec![successor_id];
            while let Some(changeset_id) = changeset_ids.pop() {
                // See if we have an entry for this changeset_id.
                if let Some(entry) = self.entries.get(&changeset_id).cloned() {
                    // Add all of this entry's predecessors to the queue of
                    // additional changesets we will need to process.  Push
                    // predecessors in reverse order so that we process them in
                    // forwards order.
                    for predecessor_id in entry.predecessors().iter().rev() {
                        // Only enqueue the predecessor if:
                        // 1. There is an entry for it
                        if self.entries.contains_key(predecessor_id)
                            // 2. AND we haven't already processed this predecessor
                            && !processed.contains(predecessor_id)
                            // 3. AND its not the same as the successor
                            && successor_id != *predecessor_id
                        {
                            changeset_ids.push(predecessor_id.clone());
                        }
                    }
                    entries.push(entry);
                }
                processed.insert(changeset_id);
            }
            mutation_history.insert(successor_id, entries);
        }
        mutation_history
    }
}
