/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::iter::Extend;
use std::iter::IntoIterator;
use std::iter::Iterator;
use std::ops::Deref;
use std::ops::DerefMut;
use std::ops::RangeBounds;

use maplit::btreemap;
use maplit::hashset;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;

/// A frontier of changesets ordered by generation number.
#[derive(Clone, Debug)]
pub struct ChangesetFrontier(BTreeMap<Generation, HashSet<ChangesetId>>);

impl ChangesetFrontier {
    pub fn new() -> Self {
        Self(Default::default())
    }

    pub fn new_single(cs_id: ChangesetId, generation: Generation) -> Self {
        Self(btreemap! { generation => hashset! { cs_id }})
    }

    pub fn highest_generation_contains(&self, cs_id: ChangesetId, generation: Generation) -> bool {
        match self.last_key_value() {
            None => false,
            Some((highest_frontier_generation, cs_ids)) => {
                *highest_frontier_generation == generation && cs_ids.contains(&cs_id)
            }
        }
    }

    pub fn highest_generation_intersection(
        &self,
        other_frontier: &ChangesetFrontier,
    ) -> Vec<ChangesetId> {
        match self.last_key_value() {
            None => vec![],
            Some((highest_frontier_generation, cs_ids)) => {
                match other_frontier.get(highest_frontier_generation) {
                    None => vec![],
                    Some(other_cs_ids) => cs_ids.intersection(other_cs_ids).copied().collect(),
                }
            }
        }
    }

    pub fn is_disjoint(&self, other_frontier: &ChangesetFrontier) -> bool {
        for (gen, cs_ids) in self.iter().rev() {
            if let Some(other_cs_ids) = other_frontier.get(gen) {
                if !cs_ids.is_disjoint(other_cs_ids) {
                    return false;
                }
            }
        }
        true
    }

    /// Return an iterator over tuples of each changeset in the frontier
    /// together with its generation number.
    pub fn into_flat_iter(self) -> impl Iterator<Item = (ChangesetId, Generation)> {
        self.0
            .into_iter()
            .flat_map(|(gen, cs_ids)| cs_ids.into_iter().map(move |cs_id| (cs_id, gen)))
    }

    /// Returns a vec of all changesets in the frontier.
    pub fn changesets(&self) -> Vec<ChangesetId> {
        self.iter()
            .flat_map(|(_, cs_ids)| cs_ids.iter())
            .copied()
            .collect()
    }

    /// Returns a vec of all changesets in the frontier inside
    /// of the given range.
    pub fn changesets_in_range(
        &self,
        range: impl RangeBounds<Generation>,
    ) -> impl Iterator<Item = ChangesetId> + '_ {
        self.range(range)
            .flat_map(|(_, cs_ids)| cs_ids.iter())
            .copied()
    }
}

impl Deref for ChangesetFrontier {
    type Target = BTreeMap<Generation, HashSet<ChangesetId>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ChangesetFrontier {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl FromIterator<(ChangesetId, Generation)> for ChangesetFrontier {
    fn from_iter<T: IntoIterator<Item = (ChangesetId, Generation)>>(iter: T) -> Self {
        let mut frontier = Self::new();

        for (cs_id, gen) in iter {
            frontier.entry(gen).or_default().insert(cs_id);
        }

        frontier
    }
}

impl Extend<(ChangesetId, Generation)> for ChangesetFrontier {
    fn extend<T: IntoIterator<Item = (ChangesetId, Generation)>>(&mut self, iter: T) {
        for (cs_id, gen) in iter {
            self.entry(gen).or_default().insert(cs_id);
        }
    }
}
