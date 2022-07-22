/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::cmp::Ordering;

use crate::errors::bug;
use crate::errors::programming;
use crate::id::Group;
use crate::id::Id;
use crate::segment::Segment;
use crate::segment::SegmentFlags;
use crate::spanset::Span;
use crate::IdSet;
use crate::Level;
use crate::Result;

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

    /// Remove a flat segment. The segment cannot have descendants.
    fn remove_flat_segment(&mut self, segment: &Segment) -> Result<()> {
        self.resize_flat_segment(segment, None)
    }

    /// Remove or truncate a flat segment.
    /// - If `new_high` is None, remove the flat segment. The segment cannot
    ///   have descendants.
    /// - If `new_high` is set, trucnate the flat segment by resetting `high` to
    ///   the given value. If `new_high` is smaller than `high`, then
    ///   `descendants(new_high+1:high)` must be empty.
    fn resize_flat_segment(&mut self, segment: &Segment, new_high: Option<Id>) -> Result<()> {
        // `segment` must be a flat segment.
        if segment.level()? != 0 {
            return programming(format!(
                "resize_flat_segment requires a flat segment, got {:?}",
                segment
            ));
        }

        // Figure out deleted and inserted spans for checks.
        let span = segment.span()?;
        let (deleted_span, inserted_span) = get_deleted_inserted_spans(span, new_high);
        if deleted_span.is_none() && inserted_span.is_none() {
            // No need to do anything.
            return Ok(());
        }

        // `segment` needs to match an existing flat segment?
        if let Some(existing_segment) = self.find_flat_segment_including_id(span.high)? {
            if &existing_segment != segment {
                return programming(format!(
                    "resize_flat_segment got {:?} which does not match the existing segment {:?}",
                    segment, existing_segment
                ));
            }
        } else {
            return programming(format!(
                "resize_flat_segment got a non-existed segment {:?}",
                segment
            ));
        }

        // When inserting a new span, the space cannot be taken.
        if let Some(span) = inserted_span {
            let taken = self.all_ids_in_groups(&[span.high.group()])?;
            let overlap = taken.intersection(&span.into());
            if !overlap.is_empty() {
                return programming(format!(
                    "resize_flat_segment cannot overlap with existing segments (segment: {:?} new_high: {:?}, overlap: {:?})",
                    segment, new_high, overlap,
                ));
            }
        }

        // When deleting a span, it should have no descendants.
        if let Some(span) = deleted_span {
            if let Some(item) = self.iter_flat_segments_with_parent_span(span)?.next() {
                let (_, child) = item?;
                return programming(format!(
                    "resize_flat_segment requires a segment without descendants, got {:?} with child segment {:?}",
                    segment, child
                ));
            }
        }

        // Prepare the resized segment and check `new_high`.
        let new_segment = match new_high {
            Some(high) => Some(segment.with_high(high)?),
            None => None,
        };

        // Check passed, call the low-level function.
        self.remove_flat_segment_unchecked(segment)?;

        // Need to insert a new segment.
        if let Some(seg) = new_segment {
            self.insert_segment(seg)?;
        }

        Ok(())
    }

    /// Actual implementation of the segment removal part of
    /// `resize_flat_segment` without checks.
    /// This should only be called via `resize_flat_segment` for integrity.
    fn remove_flat_segment_unchecked(&mut self, segment: &Segment) -> Result<()>;

    /// Return all ids from given groups. This is useful to implement the
    /// `all()` operation.
    ///
    /// With discontinuous segments, this might return multiple spans for
    /// a single group.
    fn all_ids_in_groups(&self, groups: &[Group]) -> Result<IdSet>;

    /// Find all ids covered by a specific level of segments.
    ///
    /// This function assumes that segments are built in order,
    /// and higher level segments do not cover more than lower
    /// levels.
    ///
    /// That is, if range `Id(x)..Id(y)` is covered by segment
    /// level `n`. Then segment level `n+1` would cover `Id(x)..Id(p)`
    /// and not cover `Id(p)..Id(y)` (x <= p <= y). In other words,
    /// the following cases are forbidden:
    ///
    /// ```plain,ignore
    ///     level n     [-------covered-------]
    ///     level n+1   [covered] gap [covered]
    ///
    ///     level n     [---covered---]
    ///     level n+1   gap [covered]
    ///
    ///     level n     [covered] gap
    ///     level n+1   [---covered---]
    /// ```
    ///
    /// The following cases are okay:
    ///
    /// ```plain,ignore
    ///     level n     [---covered---]
    ///     level n+1   [covered] gap
    ///
    ///     level n     [---covered---]
    ///     level n+1   [---covered---]
    /// ```
    fn all_ids_in_segment_level(&self, level: Level) -> Result<IdSet> {
        let all_ids = self.all_ids_in_groups(&Group::ALL)?;
        if level == 0 {
            return Ok(all_ids);
        }

        let mut result = IdSet::empty();
        for span in all_ids.as_spans() {
            // In this span:
            //
            //      [---------span--------]
            //                 seg-]
            //
            // If we found the right side of a segment, then we can
            // assume the segments cover till the left side without
            // checking the actual segments:
            //
            //      [---------span--------]
            //      [seg][...][seg-]
            let seg = self.iter_segments_descending(span.high, level)?.next();
            if let Some(seg) = seg {
                let seg = seg?;
                let seg_span = seg.span()?;
                if span.contains(seg_span.high) {
                    // sanity check
                    if !span.contains(seg_span.low) {
                        return programming(format!(
                            "span {:?} from all_ids_in_groups should cover all segment {:?}",
                            span, seg
                        ));
                    }
                    result.push(span.low..=seg_span.high);
                }
            }
        }
        Ok(result)
    }

    /// Find segments that covers `id..` range at the given level, within a same group.
    fn next_segments(&self, id: Id, level: Level) -> Result<Vec<Segment>>;

    /// Find segments that fully covers the given range. Return segments in ascending order.
    fn segments_in_span_ascending(&self, span: Span, level: Level) -> Result<Vec<Segment>> {
        let mut iter = self.iter_segments_ascending(span.low, level)?;
        let mut result = Vec::new();
        while let Some(item) = iter.next() {
            let seg = item?;
            let seg_span = seg.span()?;
            if seg_span.low >= span.low && seg_span.high <= span.high {
                result.push(seg);
            }
            if seg_span.low > span.high {
                break;
            }
        }
        Ok(result)
    }

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

    /// Iterate through `(parent_id, segment)` for master flat segments
    /// that have a parent in the given span.
    ///
    /// The order of returned segments is implementation-specific.
    /// Different stores might return different order.
    fn iter_flat_segments_with_parent_span<'a>(
        &'a self,
        parent_span: Span,
    ) -> Result<Box<dyn Iterator<Item = Result<(Id, Segment)>> + 'a>>;

    /// Iterate through flat segments that have the given parent.
    ///
    /// The order of returned segments is implementation-specific.
    /// Different stores might return different order.
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
    /// Return the merged segment if it's mergeable.
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
        let last_segment = match self.iter_segments_descending(span.low, 0)?.next() {
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
        // This is because we intentionally dropped the last (incomplete)
        // high-level segment when building.
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

/// Used by `resize_flat_segment` functions.
pub(crate) fn get_deleted_inserted_spans(
    span: Span,
    new_high: Option<Id>,
) -> (Option<Span>, Option<Span>) {
    match new_high {
        Some(new_high) => match new_high.cmp(&span.high) {
            Ordering::Less => (Some(Span::from(new_high + 1..=span.high)), None),
            Ordering::Equal => (None, None),
            Ordering::Greater => (None, Some(Span::from(span.high + 1..=new_high))),
        },
        None => (Some(span), None),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::fmt;
    use std::ops::Deref;

    use once_cell::sync::Lazy;

    use super::*;

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

    // Helpers
    const ROOT: SegmentFlags = SegmentFlags::HAS_ROOT;
    const EMPTY: SegmentFlags = SegmentFlags::empty();

    const M: Group = Group::MASTER;
    const N: Group = Group::NON_MASTER;

    fn seg(flags: SegmentFlags, group: Group, low: u64, high: u64, parents: &[u64]) -> Segment {
        Segment::new(
            flags,
            0,
            group.min_id() + low,
            group.min_id() + high,
            &parents.iter().copied().map(Id).collect::<Vec<_>>(),
        )
    }

    /// High-level segment.
    fn hseg(
        level: Level,
        flags: SegmentFlags,
        group: Group,
        low: u64,
        high: u64,
        parents: &[u64],
    ) -> Segment {
        Segment::new(
            flags,
            level,
            group.min_id() + low,
            group.min_id() + high,
            &parents.iter().copied().map(Id).collect::<Vec<_>>(),
        )
    }

    fn fmt<T: fmt::Debug>(value: T) -> String {
        format!("{:?}", value)
    }

    fn fmt_iter<T: fmt::Debug>(iter: impl Iterator<Item = Result<T>>) -> Vec<String> {
        iter.map(|i| fmt(i.unwrap())).collect()
    }

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

    fn test_all_ids_in_groups(store: &mut dyn IdDagStore) {
        let all_id_str = |store: &dyn IdDagStore, groups| {
            format!("{:?}", store.all_ids_in_groups(groups).unwrap())
        };

        // Insert some discontinuous segments. Then query all_ids_in_groups.
        store.insert_segment(seg(ROOT, M, 10, 20, &[])).unwrap();
        assert_eq!(all_id_str(store, &[M]), "10..=20");

        store.insert_segment(seg(ROOT, M, 30, 40, &[])).unwrap();
        store.insert_segment(seg(ROOT, M, 50, 60, &[])).unwrap();
        assert_eq!(all_id_str(store, &[M]), "10..=20 30..=40 50..=60");

        // Insert adjacent segments and check that spans are merged.
        store.insert_segment(seg(EMPTY, M, 41, 45, &[40])).unwrap();
        assert_eq!(all_id_str(store, &[M]), "10..=20 30..=45 50..=60");

        store.insert_segment(seg(EMPTY, M, 46, 49, &[45])).unwrap();
        assert_eq!(all_id_str(store, &[M]), "10..=20 30..=60");

        store.insert_segment(seg(EMPTY, M, 61, 70, &[60])).unwrap();
        assert_eq!(all_id_str(store, &[M]), "10..=20 30..=70");

        store.insert_segment(seg(ROOT, M, 21, 29, &[])).unwrap();
        assert_eq!(all_id_str(store, &[M]), "10..=70");

        store.insert_segment(seg(ROOT, M, 0, 5, &[])).unwrap();
        assert_eq!(all_id_str(store, &[M]), "0..=5 10..=70");

        store.insert_segment(seg(ROOT, M, 6, 9, &[])).unwrap();
        assert_eq!(all_id_str(store, &[M]), "0..=70");

        // Spans in the non-master group.
        store.insert_segment(seg(EMPTY, N, 0, 10, &[])).unwrap();
        store.insert_segment(seg(EMPTY, N, 20, 30, &[])).unwrap();
        assert_eq!(all_id_str(store, &[N]), "N0..=N10 N20..=N30");
        store.insert_segment(seg(EMPTY, N, 11, 15, &[])).unwrap();
        assert_eq!(all_id_str(store, &[N]), "N0..=N15 N20..=N30");
        store.insert_segment(seg(EMPTY, N, 17, 19, &[])).unwrap();
        assert_eq!(all_id_str(store, &[N]), "N0..=N15 N17..=N30");
        store.insert_segment(seg(EMPTY, N, 16, 16, &[])).unwrap();
        assert_eq!(all_id_str(store, &[M]), "0..=70");
        assert_eq!(all_id_str(store, &[N]), "N0..=N30");
        assert_eq!(all_id_str(store, &[M, N]), "0..=70 N0..=N30");

        store.remove_non_master().unwrap();
        assert_eq!(all_id_str(store, &[N]), "");
        assert_eq!(all_id_str(store, &[M, N]), "0..=70");
    }

    fn test_all_ids_in_segment_level(store: &mut dyn IdDagStore) {
        let level_id_str = |store: &dyn IdDagStore, level| {
            format!("{:?}", store.all_ids_in_segment_level(level).unwrap())
        };

        // Insert some discontinuous segments. Then query all_ids_in_groups.
        insert_segments(
            store,
            vec![
                &seg(ROOT, M, 0, 10, &[]),
                &seg(EMPTY, M, 11, 20, &[9]),
                &seg(EMPTY, M, 21, 30, &[15]),
                &seg(ROOT, M, 50, 60, &[]),
                &seg(EMPTY, M, 61, 70, &[51]),
                &seg(EMPTY, M, 71, 75, &[51]),
                &seg(EMPTY, M, 76, 80, &[51]),
                &seg(EMPTY, M, 81, 85, &[51]),
                &seg(ROOT, M, 100, 110, &[]),
                &seg(EMPTY, M, 111, 120, &[105]),
                &seg(EMPTY, M, 121, 130, &[115]),
                &seg(ROOT, N, 0, 10, &[]),
                &seg(EMPTY, N, 11, 20, &[9]),
                &seg(EMPTY, N, 21, 30, &[15]),
                &hseg(1, ROOT, M, 0, 10, &[]),
                &hseg(1, EMPTY, M, 11, 20, &[9]),
                &hseg(1, ROOT, M, 50, 70, &[]),
                &hseg(1, EMPTY, M, 71, 80, &[51]),
                &hseg(1, ROOT, M, 100, 120, &[]),
                &hseg(1, ROOT, N, 0, 30, &[]),
                &hseg(2, ROOT, M, 50, 80, &[]),
                &hseg(2, ROOT, M, 100, 120, &[]),
            ],
        );

        assert_eq!(level_id_str(store, 0), "0..=30 50..=85 100..=130 N0..=N30");
        assert_eq!(level_id_str(store, 1), "0..=20 50..=80 100..=120 N0..=N30");
        assert_eq!(level_id_str(store, 2), "50..=80 100..=120");
        assert_eq!(level_id_str(store, 3), "");
    }

    fn test_discontinuous_merges(store: &mut dyn IdDagStore) {
        insert_segments(
            store,
            vec![
                &seg(ROOT, M, 0, 10, &[]),
                &seg(EMPTY, M, 20, 30, &[5]),
                &seg(EMPTY, M, 11, 15, &[10]),
                &seg(EMPTY, M, 31, 35, &[30]),
            ],
        );

        let iter = store.iter_segments_descending(Id(25), 0).unwrap();
        assert_eq!(fmt_iter(iter), ["R0-15[]"]);

        // 0-10 and 11-15 are merged.
        let seg = store.find_segment_by_head_and_level(Id(10), 0).unwrap();
        assert_eq!(fmt(seg), "None");
        let seg = store.find_segment_by_head_and_level(Id(15), 0).unwrap();
        assert_eq!(fmt(seg), "Some(R0-15[])");

        // 20-30 and 31-35 are merged.
        let seg = store.find_segment_by_head_and_level(Id(30), 0).unwrap();
        assert_eq!(fmt(seg), "None");
        let seg = store.find_segment_by_head_and_level(Id(35), 0).unwrap();
        assert_eq!(fmt(seg), "Some(20-35[5])");

        // 0-10 and 11-15 are merged.
        let seg = store.find_flat_segment_including_id(Id(9)).unwrap();
        assert_eq!(fmt(seg), "Some(R0-15[])");
        let seg = store.find_flat_segment_including_id(Id(14)).unwrap();
        assert_eq!(fmt(seg), "Some(R0-15[])");
        let seg = store.find_flat_segment_including_id(Id(16)).unwrap();
        assert_eq!(fmt(seg), "None");

        // 20-30 and 31-35 are merged.
        let seg = store.find_flat_segment_including_id(Id(35)).unwrap();
        assert_eq!(fmt(seg), "Some(20-35[5])");
        let seg = store.find_flat_segment_including_id(Id(36)).unwrap();
        assert_eq!(fmt(seg), "None");

        // Parent lookup.
        let iter = store.iter_flat_segments_with_parent(Id(5)).unwrap();
        assert_eq!(fmt_iter(iter), ["20-35[5]"]);
        let iter = store.iter_flat_segments_with_parent(Id(10)).unwrap();
        assert_eq!(fmt_iter(iter), [] as [String; 0]);
        let iter = store.iter_flat_segments_with_parent(Id(30)).unwrap();
        assert_eq!(fmt_iter(iter), [] as [String; 0]);
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

    fn test_store_iter_flat_segments_with_parent_span(store: &dyn IdDagStore) {
        let query = |span: Span| -> String {
            let iter = store.iter_flat_segments_with_parent_span(span).unwrap();
            let mut answer_str_list: Vec<String> =
                iter.map(|s| format!("{:?}", s.unwrap())).collect();
            answer_str_list.sort_unstable();
            answer_str_list.join(" ")
        };

        assert_eq!(query(Id(2).into()), "(2, 6-9[2])");
        assert_eq!(query((Id(0)..=Id(3)).into()), "(2, 6-9[2])");
        assert_eq!(query(Id(13).into()), "(13, N0-N2[13])");
        assert_eq!(query(Id(4).into()), "");
        assert_eq!(query(nid(2).into()), "(N2, N5-N6[N2, N4])");
    }

    fn test_store_iter_flat_segments_with_parent(store: &dyn IdDagStore) {
        let lookup = |id: Id| -> Vec<Segment> {
            let mut list = store
                .iter_flat_segments_with_parent(id)
                .unwrap()
                .collect::<Result<Vec<_>>>()
                .unwrap();
            list.sort_unstable_by_key(|seg| seg.low().unwrap());
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
        assert!(
            store
                .iter_flat_segments_with_parent_span(nid(2).into())
                .unwrap()
                .next()
                .is_none()
        );
    }

    pub(crate) fn test_remove_segment(store: &mut dyn IdDagStore) {
        // Prepare segments, 3 segments per group.
        let parents_nid_3_4 = [nid(4), nid(3)];
        let parents_id_12_nid_4 = [Id(12), nid(4)];
        let segs: Vec<(Id, Id, &[Id])> = vec![
            (Id(0), Id(5), &[]),
            (Id(6), Id(10), &[Id(4), Id(3)]),
            (Id(11), Id(15), &[Id(4)]),
            (nid(0), nid(5), &[]),
            (nid(6), nid(10), &parents_nid_3_4),
            (nid(11), nid(15), &parents_id_12_nid_4),
        ];
        let mut segs: Vec<Segment> = segs
            .into_iter()
            .map(|(low, high, parents)| {
                let flags = if parents.is_empty() { ROOT } else { EMPTY };
                Segment::new(flags, 0, low, high, parents)
            })
            .collect();
        // Also insert high-level segments.
        segs.push(Segment::new(ROOT, 1, Id(0), Id(10), &[]));
        segs.push(Segment::new(ROOT, 1, nid(0), nid(10), &[]));
        for seg in segs.clone() {
            store.insert_segment(seg).unwrap();
        }

        // Cannot remove segment with descendants, or high-level segments.
        for i in [0, 2, 3, 6, 7] {
            assert!(store.remove_flat_segment(&segs[i]).is_err());
        }

        let all_before_delete = store.all_ids_in_groups(&Group::ALL).unwrap();
        assert_eq!(format!("{:?}", &all_before_delete), "0..=15 N0..=N15");
        assert_eq!(
            dump_store_state(store, &all_before_delete),
            r#"
Lv0: R0-5[], 6-10[4, 3], 11-15[4], RN0-N5[], N6-N10[N4, N3], N11-N15[12, N4]
Lv1: R0-10[], RN0-N10[]
P->C: 3->6, 4->6, 4->11, 12->N11, N3->N6, N4->N6, N4->N11"#
        );

        // Remove the "middle" segment per group.
        for i in [1, 4] {
            store.remove_flat_segment(&segs[i]).unwrap();
        }

        // Check that the removed segments are actually removed.
        let all_after_delete = store.all_ids_in_groups(&Group::ALL).unwrap();
        assert_eq!(
            format!("{:?}", &all_after_delete),
            "0..=5 11..=15 N0..=N5 N11..=N15"
        );
        assert_eq!(
            dump_store_state(store, &all_before_delete),
            r#"
Lv0: R0-5[], 11-15[4], RN0-N5[], N11-N15[12, N4]
P->C: 4->11, 12->N11, N4->N11"#
        );
        let deleted_ids = IdSet::from_spans(vec![Id(6)..=Id(10), nid(6)..=nid(10)]);
        assert_eq!(dump_store_state(store, &deleted_ids), "");
    }

    pub(crate) fn test_resize_segment(store: &mut dyn IdDagStore) {
        // Prepare segments, 3 segments per group.
        let segs: Vec<(Id, Id, &[Id])> = vec![
            (Id(0), Id(100), &[]),
            (Id(200), Id(200), &[]),
            (nid(100), nid(200), &[Id(50)]),
            (nid(300), nid(400), &[Id(50)]),
        ];
        let mut segs: Vec<Segment> = segs
            .into_iter()
            .map(|(low, high, parents)| {
                let flags = if parents.is_empty() { ROOT } else { EMPTY };
                Segment::new(flags, 0, low, high, parents)
            })
            .collect();
        // Also insert high-level segments.
        segs.push(Segment::new(ROOT, 1, Id(0), Id(100), &[]));
        segs.push(Segment::new(ROOT, 1, nid(100), nid(200), &[Id(50)]));
        for seg in segs.clone() {
            store.insert_segment(seg).unwrap();
        }

        let all_before_resize = store.all_ids_in_groups(&Group::ALL).unwrap();
        assert_eq!(
            format!("{:?}", &all_before_resize),
            "0..=100 200 N100..=N200 N300..=N400"
        );
        assert_eq!(
            dump_store_state(store, &all_before_resize),
            r#"
Lv0: R0-100[], R200-200[], N100-N200[50], N300-N400[50]
Lv1: R0-100[], RN100-N200[50]
P->C: 50->N100, 50->N300"#
        );

        // Check error cases.
        let mut e = |i, id| -> String {
            store
                .resize_flat_segment(&segs[i], Some(id))
                .unwrap_err()
                .to_string()
        };

        // Cannot resize because of descendants.
        assert_eq!(
            e(0, Id(49)),
            "ProgrammingError: resize_flat_segment requires a segment without descendants, got R0-100[] with child segment N100-N200[50]"
        );

        // Cannot resize because of overlap.
        assert_eq!(
            e(0, Id(200)),
            "ProgrammingError: resize_flat_segment cannot overlap with existing segments (segment: R0-100[] new_high: Some(200), overlap: 200)"
        );

        // Cannot resize because of high < low.
        assert_eq!(
            e(1, Id(199)),
            "ProgrammingError: with_high got invalid input (segment: R200-200[] new_high: 199)"
        );

        // Cannot resize because of high and new_high are in different groups.
        assert_eq!(
            e(1, nid(0)),
            "ProgrammingError: with_high got invalid input (segment: R200-200[] new_high: N0)"
        );

        // Do resize.
        let mut resize = |i, id| store.resize_flat_segment(&segs[i], Some(id)).unwrap();

        // Shrink.
        resize(0, Id(50));
        resize(3, nid(350));

        // Grow.
        resize(1, Id(250));
        resize(2, nid(250));

        // Check state after resize.
        let all_after_resize = store.all_ids_in_groups(&Group::ALL).unwrap();
        assert_eq!(
            format!("{:?}", &all_after_resize),
            "0..=50 200..=250 N100..=N250 N300..=N350"
        );
        let all = all_after_resize.union(&all_before_resize);
        assert_eq!(
            dump_store_state(store, &all),
            r#"
Lv0: R0-50[], R200-250[], N100-N250[50], N300-N350[50]
P->C: 50->N100, 50->N300"#
        );
    }

    /// Dump the store state in the given `id_set` as a string for testing.
    pub(crate) fn dump_store_state(store: &dyn IdDagStore, id_set: &IdSet) -> String {
        let mut output = Vec::new();
        let max_level = store.max_level().unwrap();
        // Segments per level. Exercises the "head" index.
        for level in 0..=max_level {
            let mut level_segments = Vec::new();
            for &span in id_set.iter_span_asc() {
                let segs = store.segments_in_span_ascending(span, level).unwrap();
                for seg in segs {
                    if seg.level().unwrap() == level {
                        level_segments.push(format!("{:?}", seg));
                    }
                }
            }
            if !level_segments.is_empty() {
                output.push(format!("\nLv{}: {}", level, level_segments.join(", ")));
            }
        }
        // Parent indexes in the id_set. Exercises the "parent->child" index.
        let mut parent_child_relations = Vec::new();
        for &span in id_set.iter_span_asc() {
            let parent_child_segs = store
                .iter_flat_segments_with_parent_span(span)
                .unwrap()
                .collect::<Result<Vec<_>>>()
                .unwrap();
            let mut relations = parent_child_segs
                .into_iter()
                .map(|(parent_id, child_seg)| (parent_id, child_seg.low().unwrap()))
                .collect::<Vec<_>>();
            relations.sort_unstable();
            let mut relations = relations
                .into_iter()
                .map(|(parent, child)| format!("{:?}->{:?}", parent, child))
                .collect::<Vec<_>>();
            parent_child_relations.append(&mut relations);
        }
        if !parent_child_relations.is_empty() {
            output.push(format!("\nP->C: {}", parent_child_relations.join(", ")));
        }
        output.concat()
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
    fn test_multi_stores_all_ids_in_groups() {
        for_each_empty_store(|store| {
            test_all_ids_in_groups(store);
        })
    }

    #[test]
    fn test_multi_stores_all_ids_in_segment_level() {
        for_each_empty_store(|store| {
            test_all_ids_in_segment_level(store);
        })
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
    fn test_multi_stores_iter_flat_segments_with_parent_span() {
        for_each_store(|store| test_store_iter_flat_segments_with_parent_span(store));
    }

    #[test]
    fn test_multi_stores_iter_flat_segments_with_parent() {
        for_each_store(|store| test_store_iter_flat_segments_with_parent(store));
    }

    #[test]
    fn test_multi_stores_remove_non_master() {
        for_each_store(|store| test_remove_non_master(store));
    }

    #[test]
    fn test_multi_stores_discontinuous_merges() {
        for_each_empty_store(|store| test_discontinuous_merges(store));
    }

    #[test]
    fn test_multi_stores_remove_segment() {
        for_each_empty_store(|store| test_remove_segment(store));
    }

    #[test]
    fn test_multi_stores_resize_segment() {
        for_each_empty_store(|store| test_resize_segment(store));
    }
}
