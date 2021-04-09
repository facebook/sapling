/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::errors::bug;
use crate::id::{Group, Id};
use crate::segment::{Segment, SegmentFlags};
use crate::Level;
use crate::Result;
use serde::{Deserialize, Serialize};

mod in_process_store;

#[cfg(any(test, feature = "indexedlog-backend"))]
pub(crate) mod indexedlog_store;

pub(crate) use in_process_store::InProcessStore;

#[cfg(any(test, feature = "indexedlog-backend"))]
pub(crate) use indexedlog_store::IndexedLogStore;

pub trait IdDagStore: Send + Sync + 'static {
    /// Maximum level segment in the store
    fn max_level(&self) -> Result<Level>;

    /// Find segment by level and head.
    fn find_segment_by_head_and_level(&self, head: Id, level: u8) -> Result<Option<Segment>>;

    /// Find flat segment containing the given id.
    fn find_flat_segment_including_id(&self, id: Id) -> Result<Option<Segment>>;

    /// Add a new segment.
    ///
    /// For simplicity, it does not check if the new segment overlaps with
    /// an existing segment (which is a logic error). Those checks can be
    /// offline.
    fn insert(
        &mut self,
        flags: SegmentFlags,
        level: Level,
        low: Id,
        high: Id,
        parents: &[Id],
    ) -> Result<()> {
        let segment = Segment::new(flags, level, low, high, parents);
        self.insert_segment(segment)
    }

    fn insert_segment(&mut self, segment: Segment) -> Result<()>;

    /// Return the next unused id for segments of the specified level.
    ///
    /// Useful for building segments incrementally.
    fn next_free_id(&self, level: Level, group: Group) -> Result<Id>;

    /// Find segments that covers `id..` range at the given level, within a same group.
    fn next_segments(&self, id: Id, level: Level) -> Result<Vec<Segment>>;

    /// Iterate through segments at the given level in descending order.
    fn iter_segments_descending<'a>(
        &'a self,
        max_high_id: Id,
        level: Level,
    ) -> Result<Box<dyn Iterator<Item = Result<Segment>> + 'a>>;

    /// Iterate through segments at the given level in ascending order.
    fn iter_segments_ascending<'a>(
        &'a self,
        min_high_id: Id,
        level: Level,
    ) -> Result<Box<dyn Iterator<Item = Result<Segment>> + 'a + Send + Sync>>;

    /// Iterate through master flat segments that have the given parent.
    fn iter_master_flat_segments_with_parent<'a>(
        &'a self,
        parent: Id,
    ) -> Result<Box<dyn Iterator<Item = Result<Segment>> + 'a>>;

    /// Iterate through flat segments that have the given parent.
    fn iter_flat_segments_with_parent<'a>(
        &'a self,
        parent: Id,
    ) -> Result<Box<dyn Iterator<Item = Result<Segment>> + 'a>>;

    /// Remove all non master Group identifiers from the DAG.
    fn remove_non_master(&mut self) -> Result<()>;

    /// Attempt to merge the flat `segment` with the last flat segment to reduce
    /// fragmentation.
    ///
    /// ```plain,ignore
    /// [---last segment---] [---segment---]
    ///                    ^---- the only parent of segment
    /// [---merged segment-----------------]
    /// ```
    ///
    /// Return the merged segment if it's meregable.
    fn maybe_merged_flat_segment(&self, segment: &Segment) -> Result<Option<Segment>> {
        let level = segment.level()?;
        if level != 0 {
            // Only applies to flat segments.
            return Ok(None);
        }
        if segment.has_root()? {
            // Cannot merge if segment has roots (implies no parent for a flat segment).
            return Ok(None);
        }
        let span = segment.span()?;
        let group = span.low.group();
        if group != Group::MASTER {
            // Do not merge non-master groups for simplicity.
            return Ok(None);
        }
        let parents = segment.parents()?;
        if parents.len() != 1 || parents[0] + 1 != span.low {
            // Cannot merge - span.low dos not have parent [low-1] (non linear).
            return Ok(None);
        }
        let last_segment = match self.iter_segments_descending(group.max_id(), 0)?.next() {
            Some(Ok(s)) => s,
            _ => return Ok(None), // Cannot merge - No last flat segment.
        };
        let last_span = last_segment.span()?;
        if last_span.high + 1 != span.low {
            // Cannot merge - Two spans are not connected.
            return Ok(None);
        }

        // Can merge!

        // Sanity check: No high-level segments should cover "last_span".
        for lv in 1..=self.max_level()? {
            if self
                .find_segment_by_head_and_level(last_span.high, lv)?
                .is_some()
            {
                return bug(format!(
                    "lv{} segment should not cover last flat segment {:?}! ({})",
                    lv, last_span, "check build_high_level_segments"
                ));
            }
        }

        // Calculate the merged segment.
        let merged = {
            let last_parents = last_segment.parents()?;
            let flags = {
                let last_flags = last_segment.flags()?;
                let flags = segment.flags()?;
                (flags & SegmentFlags::ONLY_HEAD) | (last_flags & SegmentFlags::HAS_ROOT)
            };
            Segment::new(flags, level, last_span.low, span.high, &last_parents)
        };

        tracing::debug!(
            "merge flat segments {:?} + {:?} => {:?}",
            &last_segment,
            &segment,
            &merged
        );

        Ok(Some(merged))
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[derive(Serialize, Deserialize)]
enum StoreId {
    Master(usize),
    NonMaster(usize),
}

#[cfg(test)]
mod tests {
    use super::*;
    use once_cell::sync::Lazy;
    use std::ops::Deref;

    fn nid(id: u64) -> Id {
        Group::NON_MASTER.min_id() + id
    }
    //  0--1--2--3--4--5--10--11--12--13--N0--N1--N2--N5--N6
    //         \-6-7-8--9-/-----------------\-N3--N4--/
    static LEVEL0_HEAD2: Lazy<Segment> =
        Lazy::new(|| Segment::new(SegmentFlags::HAS_ROOT, 0 as Level, Id(0), Id(2), &[]));
    static LEVEL0_HEAD5: Lazy<Segment> =
        Lazy::new(|| Segment::new(SegmentFlags::ONLY_HEAD, 0 as Level, Id(3), Id(5), &[Id(2)]));
    static LEVEL0_HEAD9: Lazy<Segment> =
        Lazy::new(|| Segment::new(SegmentFlags::empty(), 0 as Level, Id(6), Id(9), &[Id(2)]));
    static LEVEL0_HEAD13: Lazy<Segment> = Lazy::new(|| {
        Segment::new(
            SegmentFlags::empty(),
            0 as Level,
            Id(10),
            Id(13),
            &[Id(5), Id(9)],
        )
    });

    static MERGED_LEVEL0_HEAD5: Lazy<Segment> = Lazy::new(|| {
        Segment::new(
            SegmentFlags::HAS_ROOT | SegmentFlags::ONLY_HEAD,
            0 as Level,
            Id(0),
            Id(5),
            &[],
        )
    });

    static LEVEL0_HEADN2: Lazy<Segment> =
        Lazy::new(|| Segment::new(SegmentFlags::empty(), 0 as Level, nid(0), nid(2), &[Id(13)]));
    static LEVEL0_HEADN4: Lazy<Segment> = Lazy::new(|| {
        Segment::new(
            SegmentFlags::empty(),
            0 as Level,
            nid(3),
            nid(4),
            &[nid(0), Id(9)],
        )
    });
    static LEVEL0_HEADN6: Lazy<Segment> = Lazy::new(|| {
        Segment::new(
            SegmentFlags::empty(),
            0 as Level,
            nid(5),
            nid(6),
            &[nid(2), nid(4)],
        )
    });

    static LEVEL1_HEAD13: Lazy<Segment> =
        Lazy::new(|| Segment::new(SegmentFlags::HAS_ROOT, 1 as Level, Id(0), Id(13), &[]));
    static LEVEL1_HEADN6: Lazy<Segment> = Lazy::new(|| {
        Segment::new(
            SegmentFlags::HAS_ROOT,
            1 as Level,
            nid(0),
            nid(6),
            &[Id(13)],
        )
    });

    fn insert_segments(store: &mut dyn IdDagStore, segments: Vec<&Segment>) {
        for segment in segments {
            store.insert_segment(segment.clone()).unwrap();
        }
    }

    fn get_segments() -> Vec<&'static Segment> {
        vec![
            &LEVEL0_HEAD2,
            &LEVEL0_HEAD5,
            &LEVEL0_HEAD9,
            &LEVEL0_HEAD13,
            &LEVEL1_HEAD13,
            &LEVEL0_HEADN2,
            &LEVEL0_HEADN4,
            &LEVEL0_HEADN6,
            &LEVEL1_HEADN6,
        ]
    }

    fn segments_to_owned(segments: &[&Segment]) -> Vec<Segment> {
        segments.into_iter().cloned().cloned().collect()
    }

    fn test_find_segment_by_head_and_level(store: &dyn IdDagStore) {
        let segment = store
            .find_segment_by_head_and_level(Id(13), 1 as Level)
            .unwrap()
            .unwrap();
        assert_eq!(&segment, LEVEL1_HEAD13.deref());

        let opt_segment = store
            .find_segment_by_head_and_level(Id(2), 0 as Level)
            .unwrap();
        assert!(opt_segment.is_none());

        let segment = store
            .find_segment_by_head_and_level(Id(5), 0 as Level)
            .unwrap()
            .unwrap();
        assert_eq!(&segment, MERGED_LEVEL0_HEAD5.deref());

        let segment = store
            .find_segment_by_head_and_level(nid(2), 0 as Level)
            .unwrap()
            .unwrap();
        assert_eq!(&segment, LEVEL0_HEADN2.deref());
    }

    fn test_find_flat_segment_including_id(store: &dyn IdDagStore) {
        let segment = store
            .find_flat_segment_including_id(Id(10))
            .unwrap()
            .unwrap();
        assert_eq!(&segment, LEVEL0_HEAD13.deref());

        let segment = store
            .find_flat_segment_including_id(Id(0))
            .unwrap()
            .unwrap();
        assert_eq!(&segment, MERGED_LEVEL0_HEAD5.deref());

        let segment = store
            .find_flat_segment_including_id(Id(2))
            .unwrap()
            .unwrap();
        assert_eq!(&segment, MERGED_LEVEL0_HEAD5.deref());

        let segment = store
            .find_flat_segment_including_id(Id(5))
            .unwrap()
            .unwrap();
        assert_eq!(&segment, MERGED_LEVEL0_HEAD5.deref());

        let segment = store
            .find_flat_segment_including_id(nid(1))
            .unwrap()
            .unwrap();
        assert_eq!(&segment, LEVEL0_HEADN2.deref());
    }

    fn test_next_free_id(store: &dyn IdDagStore) {
        assert_eq!(
            store.next_free_id(0 as Level, Group::MASTER).unwrap(),
            Id(14)
        );
        assert_eq!(
            store.next_free_id(0 as Level, Group::NON_MASTER).unwrap(),
            nid(7)
        );
        assert_eq!(
            store.next_free_id(1 as Level, Group::MASTER).unwrap(),
            Id(14)
        );
        assert_eq!(
            store.next_free_id(2 as Level, Group::MASTER).unwrap(),
            Group::MASTER.min_id()
        );
    }

    fn test_next_segments(store: &dyn IdDagStore) {
        let segments = store.next_segments(Id(4), 0 as Level).unwrap();
        let expected = segments_to_owned(&[&MERGED_LEVEL0_HEAD5, &LEVEL0_HEAD9, &LEVEL0_HEAD13]);
        assert_eq!(segments, expected);

        let segments = store.next_segments(Id(14), 0 as Level).unwrap();
        assert!(segments.is_empty());

        let segments = store.next_segments(Id(0), 1 as Level).unwrap();
        let expected = segments_to_owned(&[&LEVEL1_HEAD13]);
        assert_eq!(segments, expected);

        let segments = store.next_segments(Id(0), 2 as Level).unwrap();
        assert!(segments.is_empty());
    }

    fn test_max_level(store: &dyn IdDagStore) {
        assert_eq!(store.max_level().unwrap(), 1);
    }

    fn test_empty_store_max_level(store: &dyn IdDagStore) {
        assert_eq!(store.max_level().unwrap(), 0);
    }

    fn test_iter_segments_descending(store: &dyn IdDagStore) {
        let answer = store
            .iter_segments_descending(Id(12), 0)
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .unwrap();
        let expected = segments_to_owned(&[&LEVEL0_HEAD9, &MERGED_LEVEL0_HEAD5]);
        assert_eq!(answer, expected);

        let mut answer = store.iter_segments_descending(Id(1), 0).unwrap();
        assert!(answer.next().is_none());

        let answer = store
            .iter_segments_descending(Id(13), 1)
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .unwrap();
        let expected = segments_to_owned(&[&LEVEL1_HEAD13]);
        assert_eq!(answer, expected);

        let mut answer = store.iter_segments_descending(Id(5), 2).unwrap();
        assert!(answer.next().is_none());
    }

    fn test_iter_segments_ascending(store: &dyn IdDagStore) {
        let answer = store
            .iter_segments_ascending(Id(12), 0)
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .unwrap();
        let expected = segments_to_owned(&[
            &LEVEL0_HEAD13,
            &LEVEL0_HEADN2,
            &LEVEL0_HEADN4,
            &LEVEL0_HEADN6,
        ]);
        assert_eq!(answer, expected);

        let answer = store
            .iter_segments_ascending(Id(14), 0)
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .unwrap();
        let expected = segments_to_owned(&[&LEVEL0_HEADN2, &LEVEL0_HEADN4, &LEVEL0_HEADN6]);
        assert_eq!(answer, expected);

        let mut answer = store.iter_segments_ascending(nid(7), 0).unwrap();
        assert!(answer.next().is_none());

        let answer = store
            .iter_segments_ascending(nid(3), 1)
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .unwrap();
        let expected = segments_to_owned(&[&LEVEL1_HEADN6]);
        assert_eq!(answer, expected);

        let mut answer = store.iter_segments_ascending(Id(5), 2).unwrap();
        assert!(answer.next().is_none());
    }

    fn test_store_iter_master_flat_segments_with_parent(store: &dyn IdDagStore) {
        let mut answer = store
            .iter_master_flat_segments_with_parent(Id(2))
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .unwrap();
        // LEVEL0_HEAD5 is not in answer because it was merged into MERGED_LEVEL0_HEAD5
        // and MERGED_LEVEL0_HEAD5 no longer has parent 2.
        let expected = segments_to_owned(&[&LEVEL0_HEAD9]);
        answer.sort_by_key(|s| s.head().unwrap());
        assert_eq!(answer, expected);

        let mut answer = store.iter_master_flat_segments_with_parent(Id(13)).unwrap();
        assert!(answer.next().is_none());

        let mut answer = store.iter_master_flat_segments_with_parent(Id(4)).unwrap();
        assert!(answer.next().is_none());

        let mut answer = store.iter_master_flat_segments_with_parent(nid(2)).unwrap();
        assert!(answer.next().is_none());
    }

    fn test_store_iter_flat_segments_with_parent(store: &dyn IdDagStore) {
        let lookup = |id: Id| -> Vec<_> {
            let mut list = store
                .iter_flat_segments_with_parent(id)
                .unwrap()
                .collect::<Result<Vec<_>>>()
                .unwrap();
            list.sort_unstable_by_key(|seg| seg.high().unwrap());
            list
        };

        let answer = lookup(Id(2));
        // LEVEL0_HEAD5 is not in answer because it was merged into MERGED_LEVEL0_HEAD5
        // and MERGED_LEVEL0_HEAD5 no longer has parent 2.
        let expected = segments_to_owned(&[&LEVEL0_HEAD9]);
        assert_eq!(answer, expected);

        let answer = lookup(Id(13));
        let expected = segments_to_owned(&[&LEVEL0_HEADN2]);
        assert_eq!(answer, expected);

        let answer = lookup(Id(4));
        assert!(answer.is_empty());

        let answer = lookup(nid(2));
        let expected = segments_to_owned(&[&LEVEL0_HEADN6]);
        assert_eq!(answer, expected);

        let answer = lookup(Id(9));
        let expected = segments_to_owned(&[&LEVEL0_HEAD13, &LEVEL0_HEADN4]);
        assert_eq!(answer, expected);
    }

    fn test_remove_non_master(store: &mut dyn IdDagStore) {
        store.remove_non_master().unwrap();

        assert!(
            store
                .find_segment_by_head_and_level(nid(2), 0 as Level)
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .find_flat_segment_including_id(nid(1))
                .unwrap()
                .is_none()
        );
        assert_eq!(
            store.next_free_id(0 as Level, Group::NON_MASTER).unwrap(),
            nid(0)
        );
        assert!(
            store
                .iter_master_flat_segments_with_parent(nid(2))
                .unwrap()
                .next()
                .is_none()
        );
    }

    fn for_each_empty_store(f: impl Fn(&mut dyn IdDagStore)) {
        let mut store = InProcessStore::new();
        tracing::debug!("testing InProcessStore");
        f(&mut store);

        #[cfg(feature = "indexedlog-backend")]
        {
            let dir = tempfile::tempdir().unwrap();
            let mut store = IndexedLogStore::open(&dir.path()).unwrap();
            tracing::debug!("testing IndexedLogStore");
            f(&mut store);
        }
    }

    fn for_each_store(f: impl Fn(&mut dyn IdDagStore)) {
        for_each_empty_store(|store| {
            insert_segments(store, get_segments());
            f(store);
        })
    }

    #[test]
    fn test_multi_stores_insert() {
        // `for_each_store` does inserts, we care that nothings panics.
        for_each_store(|_store| ())
    }

    #[test]
    fn test_multi_stores_find_segment_by_head_and_level() {
        for_each_store(|store| test_find_segment_by_head_and_level(store));
    }

    #[test]
    fn test_multi_stores_find_flat_segment_including_id() {
        for_each_store(|store| test_find_flat_segment_including_id(store));
    }

    #[test]
    fn test_multi_stores_next_free_id() {
        for_each_store(|store| test_next_free_id(store));
    }

    #[test]
    fn test_multi_stores_next_segments() {
        for_each_store(|store| test_next_segments(store));
    }

    #[test]
    fn test_multi_stores_max_level() {
        for_each_empty_store(|store| test_empty_store_max_level(store));
    }

    #[test]
    fn test_multi_stores_iter_segments_descending() {
        for_each_store(|store| test_iter_segments_descending(store));
    }

    #[test]
    fn test_multi_stores_iter_segments_ascending() {
        for_each_store(|store| test_iter_segments_ascending(store));
    }

    #[test]
    fn test_multi_stores_iter_master_flat_segments_with_parent() {
        for_each_store(|store| test_store_iter_master_flat_segments_with_parent(store));
    }

    #[test]
    fn test_multi_stores_iter_flat_segments_with_parent() {
        for_each_store(|store| test_store_iter_flat_segments_with_parent(store));
    }

    #[test]
    fn test_multi_stores_remove_non_master() {
        for_each_store(|store| test_remove_non_master(store));
    }
}
