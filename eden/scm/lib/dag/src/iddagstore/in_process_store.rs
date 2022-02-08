/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt;
use std::iter;
use std::result::Result as StdResult;

use serde::de::Error;
use serde::de::SeqAccess;
use serde::de::Visitor;
use serde::ser::SerializeSeq;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;

use super::IdDagStore;
use super::StoreId;
use crate::errors::bug;
use crate::id::Group;
use crate::id::Id;
use crate::ops::Persist;
use crate::segment::Segment;
use crate::spanset::Span;
use crate::IdSet;
use crate::Level;
use crate::Result;

#[derive(Clone)]
pub struct InProcessStore {
    master_segments: Vec<Segment>,
    non_master_segments: Vec<Segment>,
    // level -> head -> serialized Segment
    level_head_index: Vec<BTreeMap<Id, StoreId>>,
    // (child-group, parent) -> child (in a flat segment).
    parent_index: BTreeMap<(Group, Id), BTreeSet<Id>>,
    // IdSet covered by flat segments in specified groups.
    id_set_by_group: [IdSet; Group::COUNT],
}

impl IdDagStore for InProcessStore {
    fn max_level(&self) -> Result<Level> {
        Ok((self.level_head_index.len().max(1) - 1) as Level)
    }

    fn find_segment_by_head_and_level(&self, head: Id, level: Level) -> Result<Option<Segment>> {
        let answer = self
            .get_head_index(level)
            .and_then(|head_index| head_index.get(&head))
            .map(|store_id| self.get_segment(store_id));
        Ok(answer)
    }

    fn find_flat_segment_including_id(&self, id: Id) -> Result<Option<Segment>> {
        let level = 0;
        let answer = self
            .get_head_index(level)
            .and_then(|head_index| head_index.range(id..).next())
            .map(|(_, store_id)| self.get_segment(store_id));
        if let Some(ref seg) = &answer {
            if seg.span()?.low > id {
                return Ok(None);
            }
        }
        Ok(answer)
    }

    fn insert_segment(&mut self, segment: Segment) -> Result<()> {
        let span = segment.span()?;
        let high = span.high;
        let level = segment.level()?;
        let parents = segment.parents()?;
        let group = high.group();

        // Can we merge the segment with the nearby flat segment?
        if let Some(merged) = self.maybe_merged_flat_segment(&segment)? {
            let (&last_high, &last_store_id) = match self
                .get_head_index(0)
                // This does a duplicated lookup that was done in maybe_merged_flat_segment.
                // But the overhead is probably okay, and the code is not hard to read.
                // The motivation was to share logic for stores (ex. `maybe_merged_flat_segment`).
                .and_then(|index| index.range(..span.low).rev().next())
            {
                Some((high, store_id)) => {
                    if cfg!(debug_assertions) {
                        let seg = self.get_segment(store_id);
                        debug_assert_eq!(seg.span()?.low, merged.span()?.low);
                        debug_assert_eq!(seg.parents()?, merged.parents()?);
                    }
                    (high, store_id)
                }
                None => return bug("last segment should exist if segments are mergeable"),
            };

            // Store the merged segment.
            self.set_segment(&last_store_id, merged);

            // Update the "head" index.
            let index = self.get_head_index_mut(level);
            index.remove(&last_high);
            index.insert(high, last_store_id);

            // No need to update "parents" index.

            // Update "covered" IdSet.
            self.id_set_by_group[group.0].push(span);

            return Ok(());
        }

        let store_id = match high.group() {
            Group::MASTER => {
                self.master_segments.push(segment);
                StoreId::Master(self.master_segments.len() - 1)
            }
            _ => {
                self.non_master_segments.push(segment);
                StoreId::NonMaster(self.non_master_segments.len() - 1)
            }
        };
        if level == 0 {
            for parent in parents {
                let children = self
                    .parent_index
                    .entry((group, parent))
                    .or_insert_with(BTreeSet::new);
                children.insert(span.low);
            }
            // Update "covered" IdSet.
            self.id_set_by_group[group.0].push(span);
        }
        self.get_head_index_mut(level).insert(high, store_id);
        Ok(())
    }

    fn remove_non_master(&mut self) -> Result<()> {
        for segment in self.non_master_segments.iter() {
            let level = segment.level()?;
            let head = segment.head()?;
            self.level_head_index
                .get_mut(level as usize)
                .map(|head_index| head_index.remove(&head));
        }
        let group = Group::NON_MASTER;
        for (_key, children) in self
            .parent_index
            .range_mut((group, group.min_id())..=(group, group.max_id()))
        {
            children.clear();
        }
        self.non_master_segments = Vec::new();
        self.id_set_by_group[Group::NON_MASTER.0] = IdSet::empty();
        Ok(())
    }

    fn all_ids_in_groups(&self, groups: &[Group]) -> Result<IdSet> {
        let mut result = IdSet::empty();
        for group in groups {
            result = result.union(&self.id_set_by_group[group.0]);
        }
        Ok(result)
    }

    fn next_segments(&self, id: Id, level: Level) -> Result<Vec<Segment>> {
        match self.get_head_index(level) {
            None => Ok(vec![]),
            Some(head_index) => {
                let segments = head_index
                    .range(id..id.group().max_id())
                    .map(|(_, store_id)| self.get_segment(store_id))
                    .collect();
                Ok(segments)
            }
        }
    }

    fn iter_segments_descending<'a>(
        &'a self,
        max_high_id: Id,
        level: Level,
    ) -> Result<Box<dyn Iterator<Item = Result<Segment>> + 'a>> {
        match self.get_head_index(level) {
            None => Ok(Box::new(iter::empty())),
            Some(head_index) => {
                let iter = head_index
                    .range(Id::MIN..=max_high_id)
                    .rev()
                    .map(move |(_, store_id)| Ok(self.get_segment(store_id)));
                Ok(Box::new(iter))
            }
        }
    }

    fn iter_segments_ascending<'a>(
        &'a self,
        min_high_id: Id,
        level: Level,
    ) -> Result<Box<dyn Iterator<Item = Result<Segment>> + 'a + Send + Sync>> {
        match self.get_head_index(level) {
            None => Ok(Box::new(iter::empty())),
            Some(head_index) => {
                let iter = head_index
                    .range(min_high_id..=Id::MAX)
                    .map(move |(_, store_id)| Ok(self.get_segment(store_id)));
                Ok(Box::new(iter))
            }
        }
    }

    fn iter_flat_segments_with_parent_span<'a>(
        &'a self,
        parent_span: Span,
    ) -> Result<Box<dyn Iterator<Item = Result<(Id, Segment)>> + 'a>> {
        let mut iter: Box<dyn Iterator<Item = Result<(Id, Segment)>> + 'a> =
            Box::new(std::iter::empty());
        for group in Group::ALL {
            let range = (group, parent_span.low)..=(group, parent_span.high);
            let group_iter =
                self.parent_index
                    .range(range)
                    .flat_map(move |((_group, parent_id), child_ids)| {
                        let parent_id = *parent_id;
                        child_ids.iter().filter_map(move |&id| {
                            let seg = match self.find_flat_segment_including_id(id) {
                                Ok(Some(s)) => s,
                                Err(e) => return Some(Err(e)),
                                Ok(None) => return None,
                            };
                            Some(Ok((parent_id, seg)))
                        })
                    });
            iter = Box::new(iter.chain(group_iter));
        }
        Ok(Box::new(iter))
    }

    fn iter_flat_segments_with_parent<'a>(
        &'a self,
        parent: Id,
    ) -> Result<Box<dyn Iterator<Item = Result<Segment>> + 'a>> {
        let get_iter = |group: Group| -> Result<Box<dyn Iterator<Item = Result<_>> + 'a>> {
            match self.parent_index.get(&(group, parent)) {
                None => Ok(Box::new(iter::empty())),
                Some(children) => {
                    let iter = children.iter().filter_map(move |&id| {
                        let seg = match self.find_flat_segment_including_id(id) {
                            Ok(Some(s)) => s,
                            Err(e) => return Some(Err(e)),
                            Ok(None) => return None,
                        };
                        Some(Ok(seg))
                    });
                    Ok(Box::new(iter))
                }
            }
        };
        let iter = get_iter(Group::MASTER)?.chain(get_iter(Group::NON_MASTER)?);
        Ok(Box::new(iter))
    }
}

impl Persist for InProcessStore {
    type Lock = ();

    fn lock(&mut self) -> Result<()> {
        Ok(())
    }

    fn reload(&mut self, _lock: &Self::Lock) -> Result<()> {
        Ok(())
    }

    fn persist(&mut self, _lock: &Self::Lock) -> Result<()> {
        Ok(())
    }
}

impl InProcessStore {
    fn get_head_index(&self, level: Level) -> Option<&BTreeMap<Id, StoreId>> {
        self.level_head_index.get(level as usize)
    }

    fn get_head_index_mut(&mut self, level: Level) -> &mut BTreeMap<Id, StoreId> {
        if self.level_head_index.len() <= level as usize {
            self.level_head_index
                .resize(level as usize + 1, BTreeMap::new());
        }
        &mut self.level_head_index[level as usize]
    }

    fn get_segment(&self, store_id: &StoreId) -> Segment {
        match store_id {
            &StoreId::Master(offset) => self.master_segments[offset].clone(),
            &StoreId::NonMaster(offset) => self.non_master_segments[offset].clone(),
        }
    }

    fn set_segment(&mut self, store_id: &StoreId, segment: Segment) {
        match store_id {
            &StoreId::Master(offset) => self.master_segments[offset] = segment,
            &StoreId::NonMaster(offset) => self.non_master_segments[offset] = segment,
        }
    }
}

impl InProcessStore {
    pub fn new() -> Self {
        InProcessStore {
            master_segments: Vec::new(),
            non_master_segments: Vec::new(),
            level_head_index: Vec::new(),
            parent_index: BTreeMap::new(),
            id_set_by_group: [IdSet::empty(), IdSet::empty()],
        }
    }
}

impl Serialize for InProcessStore {
    fn serialize<S>(&self, serializer: S) -> StdResult<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(
            self.master_segments.len() + self.non_master_segments.len(),
        ))?;
        for e in &self.master_segments {
            seq.serialize_element(e)?;
        }
        for e in &self.non_master_segments {
            seq.serialize_element(e)?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for InProcessStore {
    fn deserialize<D>(deserializer: D) -> StdResult<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct InProcessStoreVisitor;
        impl<'de> Visitor<'de> for InProcessStoreVisitor {
            type Value = InProcessStore;
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a list of segments")
            }
            fn visit_seq<A>(self, mut access: A) -> StdResult<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut store = InProcessStore::new();
                while let Some(segment) = access.next_element()? {
                    store.insert_segment(segment).map_err(|e| {
                        A::Error::custom(format!("failed to deserialize IdDagStore: {} ", e))
                    })?;
                }
                Ok(store)
            }
        }

        deserializer.deserialize_seq(InProcessStoreVisitor)
    }
}
