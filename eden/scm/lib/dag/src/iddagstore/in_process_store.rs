/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::GetLock;
use super::IdDagStore;
use super::StoreId;
use crate::id::{Group, Id};
use crate::segment::Segment;
use crate::Level;
use crate::Result;
use std::collections::{BTreeMap, BTreeSet};
use std::iter;

#[derive(Clone)]
pub struct InProcessStore {
    master_segments: Vec<Segment>,
    non_master_segments: Vec<Segment>,
    // level -> head -> serialized Segment
    level_head_index: Vec<BTreeMap<Id, StoreId>>,
    // (child-group, parent) -> serialized Segment
    parent_index: BTreeMap<(Group, Id), BTreeSet<StoreId>>,
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
        Ok(answer)
    }

    fn insert_segment(&mut self, segment: Segment) -> Result<()> {
        let high = segment.high()?;
        let level = segment.level()?;
        let parents = segment.parents()?;

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
            let group = high.group();
            for parent in parents {
                let children = self
                    .parent_index
                    .entry((group, parent))
                    .or_insert_with(BTreeSet::new);
                children.insert(store_id);
            }
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
        Ok(())
    }

    fn next_free_id(&self, level: Level, group: Group) -> Result<Id> {
        match self.get_head_index(level).and_then(|head_index| {
            head_index
                .range(group.min_id()..=group.max_id())
                .rev()
                .next()
        }) {
            None => Ok(group.min_id()),
            Some((_, store_id)) => {
                let segment = self.get_segment(store_id);
                Ok(segment.high()? + 1)
            }
        }
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

    fn iter_master_flat_segments_with_parent<'a>(
        &'a self,
        parent: Id,
    ) -> Result<Box<dyn Iterator<Item = Result<Segment>> + 'a>> {
        match self.parent_index.get(&(Group::MASTER, parent)) {
            None => Ok(Box::new(iter::empty())),
            Some(children) => {
                let iter = children
                    .iter()
                    .map(move |store_id| Ok(self.get_segment(store_id)));
                Ok(Box::new(iter))
            }
        }
    }

    fn iter_flat_segments_with_parent<'a>(
        &'a self,
        parent: Id,
    ) -> Result<Box<dyn Iterator<Item = Result<Segment>> + 'a>> {
        let get_iter = |group: Group| -> Result<Box<dyn Iterator<Item = Result<Segment>> + 'a>> {
            match self.parent_index.get(&(group, parent)) {
                None => Ok(Box::new(iter::empty())),
                Some(children) => {
                    let iter = children
                        .iter()
                        .map(move |store_id| Ok(self.get_segment(store_id)));
                    Ok(Box::new(iter))
                }
            }
        };
        let iter = get_iter(Group::MASTER)?.chain(get_iter(Group::NON_MASTER)?);
        Ok(Box::new(iter))
    }
}

impl GetLock for InProcessStore {
    type LockT = ();

    fn get_lock(&self) -> Result<()> {
        Ok(())
    }

    fn reload(&mut self, _lock: &Self::LockT) -> Result<()> {
        Ok(())
    }

    fn persist(&mut self, _lock: &Self::LockT) -> Result<()> {
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
}

impl InProcessStore {
    pub fn new() -> Self {
        InProcessStore {
            master_segments: Vec::new(),
            non_master_segments: Vec::new(),
            level_head_index: Vec::new(),
            parent_index: BTreeMap::new(),
        }
    }
}
