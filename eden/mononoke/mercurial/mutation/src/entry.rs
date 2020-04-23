/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::{HashMap, HashSet};

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

impl HgMutationEntrySet {
    pub(crate) fn new() -> Self {
        Self {
            entries: HashMap::new(),
            changeset_primordials: HashMap::new(),
        }
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
