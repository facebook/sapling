/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::any::Any;
use std::fs;
use std::fs::File;
use std::io;
use std::io::Cursor;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering::AcqRel;
use std::sync::atomic::Ordering::Acquire;
use std::sync::atomic::Ordering::Release;

use byteorder::BigEndian;
use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use fs2::FileExt;
use indexedlog::log;
use indexedlog::log::Fold;
use minibytes::Bytes;
use parking_lot::Mutex;

use super::IdDagStore;
use crate::errors::bug;
use crate::id::Group;
use crate::id::Id;
use crate::ops::Persist;
use crate::segment::describe_segment_bytes;
use crate::segment::hex;
use crate::segment::Segment;
use crate::segment::SegmentFlags;
use crate::spanset::Span;
use crate::IdSet;
use crate::Level;
use crate::Result;

pub struct IndexedLogStore {
    log: log::Log,
    path: PathBuf,
    cached_max_level: AtomicU8,
}

/// Fold (accumulator) that tracks IdSet covered in groups.
/// The state is stored as part in `log`.
#[derive(Debug, Default)]
struct CoveredIdSetFold {
    inner: Mutex<CoveredIdSetInner>,
}

#[derive(Debug, Default, Clone)]
struct CoveredIdSetInner {
    /// Covered id set, including pending removals.
    /// Use `get_id_set_by_group` to get an up-to-date view without removals.
    id_set_by_group: [IdSet; Group::COUNT],

    /// Pending removals. This avoids O(N*M) when there are N spans in id_set
    /// and M segments are being removed.
    ///
    /// Changing this field from `CoveredIdSetFold` must take `&mut self` to
    /// avoid race conditions.
    id_set_pending_remove_by_group: [IdSet; Group::COUNT],
}

const MAX_LEVEL_UNKNOWN: u8 = 0;
const LEVEL_BYTES: usize = std::mem::size_of::<Level>();

impl Fold for CoveredIdSetFold {
    fn load(&mut self, bytes: &[u8]) -> io::Result<()> {
        let id_sets = mincode::deserialize(bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut locked = self.inner.lock();
        locked.id_set_by_group = id_sets;
        locked.id_set_pending_remove_by_group = Default::default();
        Ok(())
    }

    fn dump(&self) -> io::Result<Vec<u8>> {
        let mut inner = self.inner.lock();
        inner.apply_removals();
        mincode::serialize(&inner.id_set_by_group)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    #[allow(clippy::for_loops_over_fallibles)]
    fn accumulate(&mut self, data: &[u8]) -> indexedlog::Result<()> {
        let mut inner = self.inner.lock();
        if data.starts_with(IndexedLogStore::MAGIC_REMOVE_SEGMENT) {
            let data_start = IndexedLogStore::MAGIC_REMOVE_SEGMENT.len() + LEVEL_BYTES;
            for seg_data in data.get(data_start..) {
                let seg = Segment(Bytes::copy_from_slice(seg_data));
                let span = match seg.span() {
                    Ok(span) => span,
                    Err(e) => return Err(("cannot decode segment", e).into()),
                };
                if let Some(set) = inner
                    .id_set_pending_remove_by_group
                    .get_mut(span.low.group().0)
                {
                    set.push(span);
                } else {
                    let msg = format!(
                        "unsupported group {} in segment {:?}",
                        span.low.group().0,
                        seg
                    );
                    return Err(msg.as_str().into());
                }
            }
            return Ok(());
        }
        inner.apply_removals();

        // See log_open_options for how other index functions read the entry.
        if data == IndexedLogStore::MAGIC_CLEAR_NON_MASTER {
            inner.id_set_by_group[Group::NON_MASTER.0] = IdSet::empty();
            return Ok(());
        }
        let data = if data.starts_with(IndexedLogStore::MAGIC_REWRITE_LAST_FLAT) {
            // See MAGIC_REWRITE_LAST_FLAT for format.
            let data_start = IndexedLogStore::MAGIC_REWRITE_LAST_FLAT.len() + Segment::OFFSET_DELTA
                - Segment::OFFSET_LEVEL;
            &data[data_start..]
        } else {
            data
        };
        let seg = Segment(Bytes::copy_from_slice(data));
        let span = match seg.span() {
            Ok(s) => s,
            Err(e) => return Err(("cannot parse segment in CoveredIdSetFold", e).into()),
        };
        if let Some(set) = inner.id_set_by_group.get_mut(span.low.group().0) {
            set.push(span);
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn clone_boxed(&self) -> Box<dyn Fold> {
        let mut inner = self.inner.lock();
        inner.apply_removals();
        let cloned_inner = inner.clone();
        let cloned = CoveredIdSetFold {
            inner: Mutex::new(cloned_inner),
        };
        Box::new(cloned)
    }
}

impl CoveredIdSetInner {
    /// Apply pending removals.
    fn apply_removals(&mut self) {
        for group in Group::ALL {
            let group = group.0;
            if !self.id_set_pending_remove_by_group[group].is_empty() {
                let new_set = self.id_set_by_group[group]
                    .difference(&self.id_set_pending_remove_by_group[group]);
                self.id_set_by_group[group] = new_set;
                self.id_set_pending_remove_by_group[group] = IdSet::empty();
            }
        }
    }
}

// Required functionality
impl IdDagStore for IndexedLogStore {
    fn max_level(&self) -> Result<Level> {
        let max_level = self.cached_max_level.load(Acquire);
        if max_level != MAX_LEVEL_UNKNOWN {
            return Ok(max_level);
        }
        let max_level = match self
            .log
            .lookup_range(Self::INDEX_LEVEL_HEAD, ..)?
            .rev()
            .nth(0)
        {
            None => 0,
            Some(key) => key?.0.get(0).cloned().unwrap_or(0),
        };
        self.cached_max_level.store(max_level, Release);
        Ok(max_level)
    }

    fn find_segment_by_head_and_level(&self, head: Id, level: u8) -> Result<Option<Segment>> {
        let key = Self::serialize_head_level_lookup_key(head, level);
        match self.log.lookup(Self::INDEX_LEVEL_HEAD, &key)?.nth(0) {
            None => Ok(None),
            Some(bytes) => Ok(Some(self.segment_from_slice(bytes?))),
        }
    }

    fn find_flat_segment_including_id(&self, id: Id) -> Result<Option<Segment>> {
        let level = 0;
        let low = Self::serialize_head_level_lookup_key(id, level);
        let high = [level + 1];
        let iter = self
            .log
            .lookup_range(Self::INDEX_LEVEL_HEAD, &low[..]..&high[..])?;
        for entry in iter {
            let (_, entries) = entry?;
            for entry in entries {
                let entry = entry?;
                let seg = self.segment_from_slice(entry);
                if seg.span()?.low > id {
                    return Ok(None);
                }
                // low <= rev
                debug_assert!(seg.high()? >= id); // by range query
                return Ok(Some(seg));
            }
        }
        Ok(None)
    }

    fn insert_segment(&mut self, segment: Segment) -> Result<()> {
        let level = segment.level()?;
        self.cached_max_level.fetch_max(level, AcqRel);
        // When inserting a new flat segment, consider merging it with the last
        // flat segment on disk.
        //
        // Turn:
        //
        //   [last segment] [(new) segment]
        //
        // Into:
        //
        //   [------------]
        //    (removed)
        //   [(new, merged) segment       ]
        //    (in memory)
        if level == 0 {
            if self.maybe_insert_merged_flat_segment(&segment)? {
                return Ok(());
            }
        }
        self.log.append(&segment.0)?;
        Ok(())
    }

    fn remove_flat_segment_unchecked(&mut self, segment: &Segment) -> Result<()> {
        let max_level = self.max_level()?;
        let mut data =
            Vec::with_capacity(segment.0.len() + Self::MAGIC_REMOVE_SEGMENT.len() + LEVEL_BYTES);
        data.extend_from_slice(Self::MAGIC_REMOVE_SEGMENT);
        data.push(max_level);
        data.extend_from_slice(segment.0.as_ref());
        // The actual remove operation is done by index functions.
        // See log_open_options().
        self.log.append(&data)?;
        Ok(())
    }

    fn all_ids_in_groups(&self, groups: &[Group]) -> Result<IdSet> {
        let fold = self
            .log
            .fold(Self::FOLD_COVERED_ID_SET)?
            .as_any()
            .downcast_ref::<CoveredIdSetFold>()
            .expect("should downcast to CoveredIdSetFold defined by OpenOptions");
        let mut result = IdSet::empty();
        let mut inner = fold.inner.lock();
        inner.apply_removals();
        let id_sets = &inner.id_set_by_group;
        for group in groups {
            result = result.union(&id_sets[group.0]);
        }
        Ok(result)
    }

    fn next_segments(&self, id: Id, level: Level) -> Result<Vec<Segment>> {
        let lower_bound = Self::serialize_head_level_lookup_key(id, level);
        let upper_bound = Self::serialize_head_level_lookup_key(id.group().max_id(), level);
        let mut result = Vec::new();
        for entry in self
            .log
            .lookup_range(Self::INDEX_LEVEL_HEAD, &lower_bound[..]..=&upper_bound)?
        {
            let (_, values) = entry?;
            for value in values {
                result.push(self.segment_from_slice(value?));
            }
        }
        Ok(result)
    }

    fn iter_segments_descending<'a>(
        &'a self,
        max_high_id: Id,
        level: Level,
    ) -> Result<Box<dyn Iterator<Item = Result<Segment>> + 'a>> {
        let lower_bound = Self::serialize_head_level_lookup_key(Id::MIN, level);
        let upper_bound = Self::serialize_head_level_lookup_key(max_high_id, level);
        let iter = self
            .log
            .lookup_range(Self::INDEX_LEVEL_HEAD, &lower_bound[..]..=&upper_bound[..])?
            .rev();
        let iter = iter.flat_map(move |entry| match entry {
            Ok((_key, values)) => values
                .into_iter()
                .map(|value| {
                    let value = value?;
                    Ok(self.segment_from_slice(value))
                })
                .collect(),
            Err(err) => vec![Err(err.into())],
        });
        Ok(Box::new(iter))
    }

    fn iter_segments_ascending<'a>(
        &'a self,
        min_high_id: Id,
        level: Level,
    ) -> Result<Box<dyn Iterator<Item = Result<Segment>> + 'a + Send + Sync>> {
        let lower_bound = Self::serialize_head_level_lookup_key(min_high_id, level);
        let upper_bound = Self::serialize_head_level_lookup_key(Id::MAX, level);
        let iter = self
            .log
            .lookup_range(Self::INDEX_LEVEL_HEAD, &lower_bound[..]..=&upper_bound[..])?;
        let iter = iter.flat_map(move |entry| match entry {
            Ok((_key, values)) => values
                .map(|value| {
                    let value = value?;
                    Ok(self.segment_from_slice(value))
                })
                .collect(),
            Err(err) => vec![Err(err.into())],
        });
        Ok(Box::new(iter))
    }

    fn iter_flat_segments_with_parent_span<'a>(
        &'a self,
        parent_span: Span,
    ) -> Result<Box<dyn Iterator<Item = Result<(Id, Segment)>> + 'a>> {
        let mut result: Vec<(Id, Segment)> = Vec::new();
        for group in Group::ALL {
            let low = index_parent_key(parent_span.low, group.min_id());
            let high = index_parent_key(parent_span.high, group.max_id());
            let range = &low[..]..=&high[..];
            let range_iter = self.log.lookup_range(Self::INDEX_PARENT, range)?;
            for entry in range_iter {
                let (key, _segments) = entry?;
                let parent_id = {
                    let bytes: [u8; 8] = key[1..9].try_into().unwrap();
                    Id(u64::from_be_bytes(bytes))
                };
                let child_id = {
                    let bytes: [u8; 8] = key[9..].try_into().unwrap();
                    Id(u64::from_be_bytes(bytes))
                };
                if let Some(seg) = self.find_flat_segment_including_id(child_id)? {
                    result.push((parent_id, seg));
                }
            }
        }
        Ok(Box::new(result.into_iter().map(Ok)))
    }

    fn iter_flat_segments_with_parent<'a>(
        &'a self,
        parent: Id,
    ) -> Result<Box<dyn Iterator<Item = Result<Segment>> + 'a>> {
        let iter = self.iter_flat_segments_with_parent_span(parent.into())?;
        Ok(Box::new(iter.map(|item| item.map(|(_, seg)| seg))))
    }

    /// Mark non-master ids as "removed".
    fn remove_non_master(&mut self) -> Result<()> {
        self.log.append(Self::MAGIC_CLEAR_NON_MASTER)?;
        let non_master_ids = self.all_ids_in_groups(&[Group::NON_MASTER])?;
        if !non_master_ids.is_empty() {
            return bug("remove_non_master did not take effect");
        }
        Ok(())
    }
}

impl Persist for IndexedLogStore {
    type Lock = File;

    fn lock(&mut self) -> Result<File> {
        // Take a filesystem lock. The file name 'lock' is taken by indexedlog
        // running on Windows, so we choose another file name here.
        let lock_file = {
            let mut path = self.path.clone();
            path.push("wlock");
            File::open(&path).or_else(|_| {
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
            })?
        };
        lock_file.lock_exclusive()?;
        Ok(lock_file)
    }

    fn reload(&mut self, _lock: &Self::Lock) -> Result<()> {
        self.log.clear_dirty()?;
        self.log.sync()?;
        Ok(())
    }

    fn persist(&mut self, _lock: &Self::Lock) -> Result<()> {
        self.log.sync()?;
        Ok(())
    }
}

impl IndexedLogStore {
    /// Attempt to merge the flat `segment` with the last flat segment to reduce
    /// fragmentation. Insert the merged segment.
    ///
    /// Return true if the merged segment was inserted.
    fn maybe_insert_merged_flat_segment(&mut self, segment: &Segment) -> Result<bool> {
        if let Some(merged) = self.maybe_merged_flat_segment(segment)? {
            let mut bytes = Vec::with_capacity(merged.0.len() + 10);
            let span = segment.span()?;
            bytes.extend_from_slice(IndexedLogStore::MAGIC_REWRITE_LAST_FLAT);
            bytes.extend_from_slice(&Self::serialize_head_level_lookup_key(
                span.low - 1, /* = last_segment.high */
                0,            /* level */
            ));
            bytes.extend_from_slice(&merged.0);
            self.log.append(&bytes)?;

            Ok(true)
        } else {
            Ok(false)
        }
    }

    // Used internally to generate the index key for lookup
    fn serialize_head_level_lookup_key(value: Id, level: u8) -> [u8; Self::KEY_LEVEL_HEAD_LEN] {
        let mut buf = [0u8; Self::KEY_LEVEL_HEAD_LEN];
        {
            let mut cur = Cursor::new(&mut buf[..]);
            cur.write_u8(level).unwrap();
            cur.write_u64::<BigEndian>(value.0).unwrap();
            debug_assert_eq!(cur.position(), Self::KEY_LEVEL_HEAD_LEN as u64);
        }
        buf
    }

    fn segment_from_slice(&self, bytes: &[u8]) -> Segment {
        let bytes = if bytes.starts_with(IndexedLogStore::MAGIC_REWRITE_LAST_FLAT) {
            let start = Self::MAGIC_REWRITE_LAST_FLAT.len() + Self::KEY_LEVEL_HEAD_LEN;
            &bytes[start..]
        } else {
            bytes
        };
        Segment(self.log.slice_to_bytes(bytes))
    }
}

/// Describe bytes of an indexedlog entry.
/// This is only for troubleshooting purpose.
pub fn describe_indexedlog_entry(data: &[u8]) -> String {
    let mut message = String::new();
    if data == IndexedLogStore::MAGIC_CLEAR_NON_MASTER {
        message += &format!("# {}: MAGIC_CLEAR_NON_MASTER\n", hex(data),);
    } else if data.starts_with(IndexedLogStore::MAGIC_REMOVE_SEGMENT) {
        message += &format!(
            "# {}: MAGIC_REMOVE_SEGMENT\n",
            hex(IndexedLogStore::MAGIC_REMOVE_SEGMENT)
        );
        if let Some(max_level) = data.get(IndexedLogStore::MAGIC_REMOVE_SEGMENT.len()) {
            message += &format!("# {}: Max Level = {}\n", hex(&[*max_level]), max_level);
            let end = IndexedLogStore::MAGIC_REMOVE_SEGMENT.len() + LEVEL_BYTES;
            message += &describe_indexedlog_entry(&data[end..]);
        }
    } else if data.starts_with(IndexedLogStore::MAGIC_REWRITE_LAST_FLAT) {
        message += &format!(
            "# {}: MAGIC_REWRITE_LAST_FLAT\n",
            hex(IndexedLogStore::MAGIC_REWRITE_LAST_FLAT)
        );
        let start = IndexedLogStore::MAGIC_REWRITE_LAST_FLAT.len();
        let end = start + Segment::OFFSET_DELTA - Segment::OFFSET_LEVEL;
        let previous_index = &data[start..end];
        let previous_level = previous_index[0];
        let previous_head = (&previous_index[1..]).read_u64::<BigEndian>().unwrap_or(0);

        message += &format!(
            "# {}: Previous index Level = {}, Head = {}\n",
            hex(&data[start..end]),
            previous_level,
            Id(previous_head),
        );

        message += &describe_indexedlog_entry(&data[end..]);
    } else {
        message += &describe_segment_bytes(data);
    }
    message
}

// Implementation details
impl IndexedLogStore {
    const INDEX_LEVEL_HEAD: usize = 0;
    const INDEX_PARENT: usize = 1;
    const FOLD_COVERED_ID_SET: usize = 0;
    const KEY_LEVEL_HEAD_LEN: usize = Segment::OFFSET_DELTA - Segment::OFFSET_LEVEL;

    // "Normal" format is just the plain bytes in `Segment`. See `Segment` for details.
    // Basically, FLAG (1B) + LEVEL (1B) + HIGH (8B) + ...

    /// Magic bytes in `Log` that indicates "remove all non-master segments".
    /// A Segment entry has at least KEY_LEVEL_HEAD_LEN (9) bytes so it does
    /// not conflict with this.
    const MAGIC_CLEAR_NON_MASTER: &'static [u8] = b"CLRNM";

    /// Magic bytes in `Log` that indicates this entry replaces a previous flat
    /// segment.
    ///
    /// Format:
    ///
    /// ```plain,ignore
    /// MAGIC_REWRITE_LAST_FLAT + LEVEL (0u8) + PREVIOUS_HEAD (u64) + SEGMENT
    /// ```
    ///
    /// The `LEVEL + PREVIOUS_HEAD` part is used to remove the segment from the
    /// `(level, head)` index.
    const MAGIC_REWRITE_LAST_FLAT: &'static [u8] = &[0xf0];

    /// Magic prefix that indicates removing a segment and its related indexes.
    ///
    /// Format:
    ///
    /// ```plain,ignore
    /// MAGIC_REMOVE_SEGMENT + MAX_LEVEL (u8) + SEGMENT
    /// ```
    const MAGIC_REMOVE_SEGMENT: &'static [u8] = &[0xf1];

    #[allow(clippy::assertions_on_constants)]
    pub fn log_open_options() -> log::OpenOptions {
        assert!(Self::MAGIC_CLEAR_NON_MASTER.len() < Segment::OFFSET_DELTA);
        assert!(Group::BITS == 8);
        for magic in [Self::MAGIC_REWRITE_LAST_FLAT, Self::MAGIC_REMOVE_SEGMENT] {
            assert_ne!(
                SegmentFlags::all().bits() & magic[Segment::OFFSET_FLAGS],
                magic[Segment::OFFSET_FLAGS],
                "magic prefix should not conflict with possible flags (first byte in Segment)"
            )
        }
        log::OpenOptions::new()
            .create(true)
            .index("level-head", |data| {
                // (level, high)
                if data == Self::MAGIC_CLEAR_NON_MASTER {
                    let max_level = 255;
                    (0..=max_level)
                        .map(|level| {
                            log::IndexOutput::RemovePrefix(Box::new([
                                level,
                                Group::NON_MASTER.0 as u8,
                            ]))
                        })
                        .collect()
                } else if data.starts_with(Self::MAGIC_REMOVE_SEGMENT) {
                    // data: 0xf1 + MAX_LEVEL (u8) + SEGMENT
                    let mut index_output = Vec::new();
                    for seg_data in data.get(Self::MAGIC_REMOVE_SEGMENT.len() + LEVEL_BYTES..) {
                        let max_level = data[1];
                        let seg = Segment(Bytes::copy_from_slice(seg_data));
                        // Remove head indexes for all levels.
                        index_output.reserve(max_level as usize + 1);
                        let head = match seg.head() {
                            Ok(id) => id,
                            Err(_) => continue,
                        };
                        for level in 0..=max_level {
                            let index_key = Self::serialize_head_level_lookup_key(head, level);
                            index_output.push(log::IndexOutput::Remove(index_key.into()));
                        }
                    }
                    index_output
                } else if data.starts_with(Self::MAGIC_REWRITE_LAST_FLAT) {
                    // See MAGIC_REWRITE_LAST_FLAT for format.
                    let start = Self::MAGIC_REWRITE_LAST_FLAT.len();
                    let end = start + Segment::OFFSET_DELTA - Segment::OFFSET_LEVEL;
                    let previous_index = &data[start..end];
                    vec![
                        log::IndexOutput::Remove(previous_index.to_vec().into_boxed_slice()),
                        log::IndexOutput::Reference(
                            (end + Segment::OFFSET_LEVEL) as u64
                                ..(end + Segment::OFFSET_DELTA) as u64,
                        ),
                    ]
                } else {
                    vec![log::IndexOutput::Reference(
                        Segment::OFFSET_LEVEL as u64..Segment::OFFSET_DELTA as u64,
                    )]
                }
            })
            .index("group-parent-child", |data| {
                //  child-group parent child  -> child for flat segments
                //  ^^^^^^^^^^^ ^^^^^^ ^^^^^^
                //  u8          u64 BE u64 BE
                //
                //  The "child-group" prefix is used for invalidating index when
                //  non-master Ids get re-assigned.
                if data == Self::MAGIC_CLEAR_NON_MASTER {
                    // Invalidate child-group == 1 entries
                    return vec![log::IndexOutput::RemovePrefix(Box::new([
                        Group::NON_MASTER.0 as u8,
                    ]))];
                }

                if data.starts_with(Self::MAGIC_REMOVE_SEGMENT) {
                    // data: 0xf1 + MAX_LEVEL (u8) + SEGMENT
                    let mut index_output = Vec::new();
                    for data in data.get(2..) {
                        let seg = Segment(Bytes::copy_from_slice(data));
                        let child = match seg.low() {
                            Ok(id) => id,
                            Err(_) => break,
                        };
                        let parents = match seg.parents() {
                            Ok(parents) => parents,
                            Err(_) => break,
                        };
                        // Remove parent->child indexes.
                        index_output.reserve(parents.len());
                        for parent in parents {
                            let index_key = index_parent_key(parent, child);
                            index_output.push(log::IndexOutput::Remove(index_key.into()));
                        }
                    }
                    return index_output;
                }

                if data.starts_with(Self::MAGIC_REWRITE_LAST_FLAT) {
                    // NOTE: Segments this index points to will have wrong `high`.
                    //
                    // But we never use the segments from this index. Instead,
                    // we use the index key to figure out (parent, child) and do
                    // an extra lookup of `child` to figure out the segments.
                    return Vec::new();
                }

                let seg = Segment(Bytes::copy_from_slice(data));
                let mut result = Vec::new();
                if seg.level().ok() == Some(0) {
                    // This should never pass since MAGIC_CLEAR_NON_MASTER[0] != 0.
                    // ([0] stores level: u8).
                    assert_ne!(
                        data,
                        Self::MAGIC_CLEAR_NON_MASTER,
                        "bug: MAGIC_CLEAR_NON_MASTER conflicts with data"
                    );
                    if let (Ok(parents), Ok(span)) = (seg.parents(), seg.span()) {
                        assert_eq!(
                            span.low.group(),
                            span.high.group(),
                            "Cross-group segment is unexpected"
                        );
                        let child_id = span.low;
                        for parent_id in parents {
                            let bytes = index_parent_key(parent_id, child_id);
                            result.push(log::IndexOutput::Owned(bytes.into()));
                        }
                    }
                }
                result
            })
            .fold_def("cover", || Box::new(CoveredIdSetFold::default()))
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let log = Self::log_open_options().open(path.clone())?;
        let iddag = Self {
            log,
            path,
            cached_max_level: AtomicU8::new(MAX_LEVEL_UNKNOWN),
        };
        Ok(iddag)
    }

    pub fn open_from_clean_log(log: log::Log) -> Result<Self> {
        let path = log.path().as_opt_path().unwrap().to_path_buf();
        if log.iter_dirty().next().is_some() {
            return bug("open_from_clean_log got a dirty log");
        }
        let iddag = Self {
            log,
            path,
            cached_max_level: AtomicU8::new(MAX_LEVEL_UNKNOWN),
        };
        Ok(iddag)
    }

    pub fn try_clone(&self) -> Result<IndexedLogStore> {
        let log = self.log.try_clone()?;
        let store = IndexedLogStore {
            log,
            path: self.path.clone(),
            cached_max_level: AtomicU8::new(self.cached_max_level.load(Acquire)),
        };
        Ok(store)
    }

    pub fn try_clone_without_dirty(&self) -> Result<IndexedLogStore> {
        let log = self.log.try_clone_without_dirty()?;
        let store = IndexedLogStore {
            log,
            path: self.path.clone(),
            cached_max_level: AtomicU8::new(MAX_LEVEL_UNKNOWN),
        };
        Ok(store)
    }
}

// Build index key for the INDEX_PARENT (group-parent-child) index.
fn index_parent_key(parent_id: Id, child_id: Id) -> [u8; 17] {
    let group = child_id.group();
    let mut result = [0u8; 1 + 8 + 8];
    debug_assert!(group.0 <= 0xff);
    result[0] = group.0 as u8;
    result[1..9].copy_from_slice(&parent_id.0.to_be_bytes());
    result[9..].copy_from_slice(&child_id.0.to_be_bytes());
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iddagstore::tests::dump_store_state;

    #[test]
    fn test_merge_persisted_segments() -> Result<()> {
        // Test that a persisted segment can still be mutable and merged.
        //
        // Persisted                    | Not persisted
        // [0..=5] [6..=10, parents=[3]] [11..=20, parents=[10]]
        //         [6..                       =20, parents=[3] ] <- merged
        let tmp = tempfile::tempdir()?;
        let mut iddag = IndexedLogStore::open(tmp.path())?;
        let seg1 = Segment::new(SegmentFlags::HAS_ROOT, 0, Id(0), Id(5), &[]);
        let seg2 = Segment::new(SegmentFlags::empty(), 0, Id(6), Id(10), &[Id(3)]);
        iddag.insert_segment(seg1)?;
        iddag.insert_segment(seg2)?;
        let locked = iddag.lock()?;
        iddag.persist(&locked)?;

        let seg3 = Segment::new(SegmentFlags::ONLY_HEAD, 0, Id(11), Id(20), &[Id(10)]);
        iddag.insert_segment(seg3)?;
        iddag.persist(&locked)?;

        // Reload.
        let iddag2 = IndexedLogStore::open(tmp.path())?;

        // Check the merged segments.
        assert_eq!(
            dbg_iter(iddag2.iter_segments_descending(Id(20), 0)?),
            "[H6-20[3], R0-5[]]"
        );

        // Check parent -> child index.
        // 10 -> 11 parent index wasn't inserted.
        assert_eq!(
            dbg_iter(iddag2.iter_flat_segments_with_parent(Id(10))?),
            "[]"
        );
        // 3 -> 6 parent index only returns the new segment.
        assert_eq!(
            dbg_iter(iddag2.iter_flat_segments_with_parent(Id(3))?),
            "[H6-20[3]]"
        );

        // Check (level, head) -> segment index.
        // Check lookup by "including_id". Should all return the new merged segment.
        assert_eq!(
            dbg(iddag2.find_flat_segment_including_id(Id(7))?),
            "Some(H6-20[3])"
        );
        assert_eq!(
            dbg(iddag2.find_flat_segment_including_id(Id(13))?),
            "Some(H6-20[3])"
        );
        assert_eq!(
            dbg(iddag2.find_flat_segment_including_id(Id(20))?),
            "Some(H6-20[3])"
        );
        // Check lookup by head.
        // By head 20 returns the new merged segment.
        assert_eq!(
            dbg(iddag2.find_segment_by_head_and_level(Id(20), 0)?),
            "Some(H6-20[3])"
        );
        // By head 10 does not return the old segment.
        assert_eq!(
            dbg(iddag2.find_segment_by_head_and_level(Id(10), 0)?),
            "None"
        );

        Ok(())
    }

    #[test]
    fn test_backwards_compatibility() {
        // Test that data written by older versions of this struct can still
        // be understood by the current version.
        let tmp = tempfile::tempdir().unwrap();
        let mut iddag = IndexedLogStore::open(tmp.path()).unwrap();

        // Code used to print "described_entries". They might base on older APIs
        // and no longer compile.
        // #[cfg(any())] makes compiler skip the code block.
        #[cfg(any())]
        {
            const ROOT: SegmentFlags = SegmentFlags::HAS_ROOT;
            const HEAD: SegmentFlags = SegmentFlags::ONLY_HEAD;
            const EMPTY: SegmentFlags = SegmentFlags::empty();
            let segs = vec![
                Segment::new(ROOT | HEAD, 0, Id(0), Id(10), &[]),
                Segment::new(HEAD, 0, Id(11), Id(20), &[Id(10)]), // merge with previous
                Segment::new(ROOT, 0, Id(21), Id(30), &[]),
                Segment::new(EMPTY, 0, nid(0), nid(10), &[Id(20)]),
                Segment::new(EMPTY, 0, nid(11), nid(20), &[nid(10)]),
                Segment::new(EMPTY, 0, nid(5), nid(15), &[Id(10)]),
            ];
            for seg in &segs[..5] {
                iddag.insert_segment(seg.clone()).unwrap();
            }
            iddag.remove_non_master().unwrap();
            iddag.insert_segment(segs[5].clone()).unwrap();
            iddag.remove_flat_segment(&segs[2]).unwrap();
            for item in iddag.log.iter() {
                let data = item.unwrap();
                let s = describe_indexedlog_entry(data);
                eprintln!("{}", s);
            }
        }

        let described_entries = r#"
# 03: Flags = HAS_ROOT | ONLY_HEAD
# 00: Level = 0
# 00 00 00 00 00 00 00 0a: High = 10
# 0a: Delta = 10 (Low = 0)
# 00: Parent count = 0

# f0: MAGIC_REWRITE_LAST_FLAT
# 00 00 00 00 00 00 00 00 0a: Previous index Level = 0, Head = 10
# 03: Flags = HAS_ROOT | ONLY_HEAD
# 00: Level = 0
# 00 00 00 00 00 00 00 14: High = 20
# 14: Delta = 20 (Low = 0)
# 00: Parent count = 0

# 01: Flags = HAS_ROOT
# 00: Level = 0
# 00 00 00 00 00 00 00 1e: High = 30
# 09: Delta = 9 (Low = 21)
# 00: Parent count = 0

# 00: Flags = (empty)
# 00: Level = 0
# 01 00 00 00 00 00 00 0a: High = N10
# 0a: Delta = 10 (Low = N0)
# 01: Parent count = 1
# 14: Parents[0] = 20

# 00: Flags = (empty)
# 00: Level = 0
# 01 00 00 00 00 00 00 14: High = N20
# 09: Delta = 9 (Low = N11)
# 01: Parent count = 1
# 8a 80 80 80 80 80 80 80 01: Parents[0] = N10

# 43 4c 52 4e 4d: MAGIC_CLEAR_NON_MASTER

# 00: Flags = (empty)
# 00: Level = 0
# 01 00 00 00 00 00 00 0f: High = N15
# 0a: Delta = 10 (Low = N5)
# 01: Parent count = 1
# 0a: Parents[0] = 10

# f1: MAGIC_REMOVE_SEGMENT
# 00: Max Level = 0
# 01: Flags = HAS_ROOT
# 00: Level = 0
# 00 00 00 00 00 00 00 1e: High = 30
# 09: Delta = 9 (Low = 21)
# 00: Parent count = 0
"#;

        for described_entry in described_entries.split("\n\n") {
            let data = undescribe(described_entry);
            iddag.log.append(data).unwrap();
        }

        let all = iddag.all_ids_in_groups(&Group::ALL).unwrap();
        assert_eq!(format!("{:?}", &all), "0..=20 N5..=N15");

        let state = dump_store_state(&iddag, &all);
        assert_eq!(state, "\nLv0: RH0-20[], N5-N15[10]\nP->C: 10->N5");
    }

    /// Turn a string generated by "describe_indexedlog_entry" back to bytes.
    fn undescribe(s: &str) -> Vec<u8> {
        let mut data = Vec::new();
        for line in s.lines() {
            // line looks like: "# 00 00: Something".
            if let Some(line) = line.get(2..) {
                if let Some((hex_bytes, _)) = line.split_once(':') {
                    for hex_byte in hex_bytes.split(' ') {
                        let byte = u8::from_str_radix(hex_byte, 16).unwrap();
                        data.push(byte);
                    }
                }
            }
        }
        data
    }

    fn nid(id: u64) -> Id {
        Group::NON_MASTER.min_id() + id
    }

    #[test]
    fn test_describe() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut iddag = IndexedLogStore::open(tmp.path())?;
        let seg1 = Segment::new(SegmentFlags::HAS_ROOT, 0, Id(0), Id(5), &[]);
        let seg2 = Segment::new(SegmentFlags::empty(), 0, Id(6), Id(10), &[Id(5)]);
        iddag.insert_segment(seg1)?;
        iddag.insert_segment(seg2)?;
        let bytes = iddag.log.iter_dirty().nth(1).unwrap()?;
        assert_eq!(
            describe_indexedlog_entry(&bytes),
            r#"# f0: MAGIC_REWRITE_LAST_FLAT
# 00 00 00 00 00 00 00 00 05: Previous index Level = 0, Head = 5
# 01: Flags = HAS_ROOT
# 00: Level = 0
# 00 00 00 00 00 00 00 0a: High = 10
# 0a: Delta = 10 (Low = 0)
# 00: Parent count = 0
"#
        );
        let seg = iddag.find_flat_segment_including_id(Id(10))?.unwrap();
        iddag.remove_flat_segment(&seg)?;
        let bytes = iddag.log.iter_dirty().nth(2).unwrap()?;
        assert_eq!(
            describe_indexedlog_entry(bytes),
            r#"# f1: MAGIC_REMOVE_SEGMENT
# 00: Max Level = 0
# 01: Flags = HAS_ROOT
# 00: Level = 0
# 00 00 00 00 00 00 00 0a: High = 10
# 0a: Delta = 10 (Low = 0)
# 00: Parent count = 0
"#
        );

        Ok(())
    }

    fn dbg_iter<'a, T: std::fmt::Debug>(iter: Box<dyn Iterator<Item = Result<T>> + 'a>) -> String {
        let v = iter.map(|s| s.unwrap()).collect::<Vec<_>>();
        dbg(v)
    }

    fn dbg<T: std::fmt::Debug>(t: T) -> String {
        format!("{:?}", t)
    }
}
