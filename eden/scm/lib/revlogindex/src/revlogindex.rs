/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::fs;
use std::fs::File;
use std::io;
use std::io::Read;
use std::io::Seek;
use std::io::Write;
use std::marker::PhantomData;
use std::mem;
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;
use std::slice;
use std::sync::Arc;

use bit_vec::BitVec;
use byteorder::ReadBytesExt;
use byteorder::BE;
use dag::errors::DagError;
use dag::errors::NotFoundError;
use dag::nameset::hints::Flags;
use dag::nameset::meta::MetaSet;
// Revset is non-lazy. Sync APIs can be used.
use dag::nameset::SyncNameSetQuery;
use dag::ops::CheckIntegrity;
use dag::ops::DagAddHeads;
use dag::ops::DagAlgorithm;
use dag::ops::IdConvert;
use dag::ops::IdMapSnapshot;
use dag::ops::Parents;
use dag::ops::PrefixLookup;
use dag::ops::ToIdSet;
use dag::Group;
use dag::Id;
use dag::IdSet;
use dag::Set;
use dag::VerLink;
use dag::Vertex;
use dag::VertexListWithOptions;
use indexedlog::lock::ScopedDirLock;
use indexedlog::utils::atomic_write_plain;
use indexedlog::utils::mmap_bytes;
use minibytes::Bytes;
use nonblocking::non_blocking_result;
use parking_lot::Mutex;
use parking_lot::RwLock;
use util::path::atomic_write_symlink;
use util::path::remove_file;

use crate::errors::corruption;
use crate::errors::unsupported;
use crate::errors::CorruptionError;
use crate::nodemap;
use crate::Error;
use crate::NodeRevMap;
use crate::Result;

const REVIDX_OCTOPUS_MERGE: u16 = 1 << 12;

impl RevlogIndex {
    /// Calculate `heads(ancestors(revs))`.
    pub fn headsancestors(&self, revs: Vec<u32>) -> dag::Result<Vec<u32>> {
        if revs.is_empty() {
            return Ok(Vec::new());
        }

        #[repr(u8)]
        #[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
        enum State {
            Unspecified,
            PotentialHead,
            NotHead,
        }

        let min_rev = *revs.iter().min().unwrap();
        assert!(self.len() > min_rev as usize);

        let mut states = vec![State::Unspecified; self.len() - min_rev as usize];
        let mut result = Vec::with_capacity(revs.len());
        for rev in revs {
            states[(rev - min_rev) as usize] = State::PotentialHead;
        }

        for rev in (min_rev as usize..self.len()).rev() {
            let state = states[rev - min_rev as usize];
            match state {
                State::Unspecified => {}
                State::PotentialHead | State::NotHead => {
                    if state == State::PotentialHead {
                        result.push(rev as u32);
                    }
                    for &parent_rev in self.parent_revs(rev as u32)?.as_revs() {
                        if parent_rev >= min_rev {
                            states[(parent_rev - min_rev) as usize] = State::NotHead;
                        }
                    }
                }
            }
        }
        Ok(result)
    }

    /// Given public and draft head revision numbers, calculate the "phase sets".
    /// Return (publicset, draftset).
    ///
    /// (only used when narrow-heads is disabled).
    pub fn phasesets(
        &self,
        publicheads: Vec<u32>,
        draftheads: Vec<u32>,
    ) -> dag::Result<(IdSet, IdSet)> {
        let mut draft_set = IdSet::empty();
        let mut public_set = IdSet::empty();

        // Used internally. Different from "phases.py".
        #[repr(u8)]
        #[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
        enum Phase {
            Unspecified,
            Draft,
            Public,
        }
        impl Phase {
            fn max(self, other: Phase) -> Phase {
                if self > other { self } else { other }
            }
        }

        let mut phases = vec![Phase::Unspecified; self.len()];
        for rev in draftheads {
            phases[rev as usize] = Phase::Draft;
        }
        for rev in publicheads {
            phases[rev as usize] = Phase::Public;
        }

        for rev in (0..self.len()).rev() {
            let phase = phases[rev as usize];
            match phase {
                Phase::Public => public_set.push(Id(rev as u64)),
                Phase::Draft => draft_set.push(Id(rev as u64)),
                // Do not track "unknown" explicitly. This is future-proof,
                // since tracking "unknown" explicitly is quite expensive
                // with the new "dag" abstraction.
                Phase::Unspecified => {}
            }
            for &parent_rev in self.parent_revs(rev as u32)?.as_revs() {
                // Propagate phases from this rev to its parents.
                phases[parent_rev as usize] = phases[parent_rev as usize].max(phase);
            }
        }
        Ok((public_set, draft_set))
    }

    /// GCA based on linear scan.
    ///
    /// Ported from Mercurial's C code `find_gca_candidates()`.
    ///
    /// The algorithm was written by Bryan O'Sullivan on 2013-04-16.
    /// He provided both a Python [1] and a C version [2]. Since then, the only real
    /// logic change is removing an unnecessary "if" branch by Mads Kiilerich on
    /// 2014-02-24 [4].
    ///
    /// The C implementation is quite competitive among linear algorithms on
    /// performance. It is cache-efficient, has fast paths to exit early, and
    /// takes up to 62 (if bitmask is u64) revs at once. Other implementations
    /// might just take 2 revs at most. For example, the older implemenation
    /// by Olivia Mackall in 2006 [3] takes 2 revs explicitly.
    ///
    /// Changes in this Rust implementation:
    /// - Change `bitmask` from `u64` to `u8` for smaller memory footage, at the
    ///   cost of losing support for more than 6 revs.
    /// - Adopted to RevlogIndex.
    /// - Run `gca` recursively on large input.
    ///
    /// [1]: https://www.mercurial-scm.org/repo/hg/rev/2f7186400a07
    /// [2]: https://www.mercurial-scm.org/repo/hg/rev/5bae936764bb
    /// [3]: https://www.mercurial-scm.org/repo/hg/rev/b1db258e875c
    /// [4]: https://www.mercurial-scm.org/repo/hg/rev/4add43865a9b
    pub fn gca_revs(&self, revs: &[u32], limit: usize) -> dag::Result<Vec<u32>> {
        type BitMask = u8;
        let revcount = revs.len();
        assert!(revcount < 7);
        if revcount == 0 {
            return Ok(Vec::new());
        }

        let allseen: BitMask = (1 << revcount) - 1;
        let poison: BitMask = 1 << revcount;
        let maxrev = revs.iter().max().cloned().unwrap();
        let mut interesting = revcount;
        let mut gca = Vec::new();
        let mut seen: Vec<BitMask> = vec![0; maxrev as usize + 1];

        for (i, &rev) in revs.iter().enumerate() {
            seen[rev as usize] = 1 << i;
        }

        for v in (0..=maxrev).rev() {
            if interesting == 0 {
                break;
            }
            let mut sv = seen[v as usize];
            if sv == 0 {
                continue;
            }
            if sv < poison {
                interesting -= 1;
                if sv == allseen {
                    gca.push(v);
                    if gca.len() >= limit {
                        return Ok(gca);
                    }
                    sv |= poison;
                    if revs.iter().any(|&r| r == v) {
                        break;
                    }
                }
            }
            for &p in self.parent_revs(v)?.as_revs() {
                let sp = seen[p as usize];
                if sv < poison {
                    if sp == 0 {
                        seen[p as usize] = sv;
                        interesting += 1
                    } else if sp != sv {
                        seen[p as usize] |= sv
                    }
                } else {
                    if sp != 0 && sp < poison {
                        interesting -= 1
                    }
                    seen[p as usize] = sv
                }
            }
        }

        Ok(gca)
    }

    /// Range based on linear scan.
    ///
    /// Ported from Mercurial's C code `reachableroots2()`.
    ///
    /// The C implementation was added by Laurent Charignon on 2015-08-06 [1].
    /// It was based on the a Python implementation added by Bryan O'Sullivan on
    /// 2012-06-01 [2], which is similar to an older implementation by Eric Hopper
    /// on 2005-10-07 [3], but faster and shorter.
    ///
    /// The C code was then revised by others. The most significant change was
    /// switching the "contains" check of "roots" and "reachable" from Python sets
    /// to bits in the pure C "revstates" array for easier error handling and
    /// better performance, by Yuya Nishihara on 2015-08-13 [4] [5].
    ///
    /// Improvements in this Rust implementation:
    /// - Use `VecDeque` for `tovisit` (roughly O(len(result)) -> O(len(heads))).
    /// - Truncate `revstates` (O(len(changelog)) -> O(max_head - min_root)).
    /// - Add `reachable.is_empty()` fast path that existed in the Python code.
    /// - Support octopus merge.
    ///
    /// [1]: https://www.mercurial-scm.org/repo/hg/rev/ff89383a97db
    /// [2]: https://www.mercurial-scm.org/repo/hg/rev/b6efeb27e733
    /// [3]: https://www.mercurial-scm.org/repo/hg/rev/518da3c3b6ce
    /// [4]: https://www.mercurial-scm.org/repo/hg/rev/b68c9d232db6
    /// [5]: https://www.mercurial-scm.org/repo/hg/rev/b3ad349d0e50
    pub fn range_revs(&self, roots: &[u32], heads: &[u32]) -> dag::Result<Vec<u32>> {
        if roots.is_empty() || heads.is_empty() {
            return Ok(Vec::new());
        }
        let min_root = *roots.iter().min().unwrap();
        let max_head = *heads.iter().max().unwrap();
        let len = (max_head.max(min_root) - min_root + 1) as usize;
        let mut reachable = Vec::with_capacity(len);
        let mut tovisit = VecDeque::new();
        let mut revstates = vec![0u8; len];

        const RS_SEEN: u8 = 1;
        const RS_ROOT: u8 = 2;
        const RS_REACHABLE: u8 = 4;

        for &rev in roots {
            if rev <= max_head {
                revstates[(rev - min_root) as usize] |= RS_ROOT;
            }
        }

        for &rev in heads {
            if rev >= min_root && revstates[(rev - min_root) as usize] & RS_SEEN == 0 {
                tovisit.push_back(rev);
                revstates[(rev - min_root) as usize] |= RS_SEEN;
            }
        }

        // Visit the tovisit list and find the reachable roots
        while let Some(rev) = tovisit.pop_front() {
            // Add the node to reachable if it is a root
            if revstates[(rev - min_root) as usize] & RS_ROOT != 0 {
                revstates[(rev - min_root) as usize] |= RS_REACHABLE;
                reachable.push(rev);
            }

            // Add its parents to the list of nodes to visit
            for &p in self.parent_revs(rev)?.as_revs() {
                if p >= min_root && revstates[(p - min_root) as usize] & RS_SEEN == 0 {
                    tovisit.push_back(p);
                    revstates[(p - min_root) as usize] |= RS_SEEN;
                }
            }
        }

        if reachable.is_empty() {
            return Ok(Vec::new());
        }

        // Find all the nodes in between the roots we found and the heads
        // and add them to the reachable set
        for rev in min_root..=max_head {
            if revstates[(rev - min_root) as usize] & RS_SEEN == 0 {
                continue;
            }
            if self
                .parent_revs(rev)?
                .as_revs()
                .iter()
                .any(|&p| p >= min_root && revstates[(p - min_root) as usize] & RS_REACHABLE != 0)
                && revstates[(rev - min_root) as usize] & RS_REACHABLE == 0
            {
                revstates[(rev - min_root) as usize] |= RS_REACHABLE;
                reachable.push(rev);
            }
        }

        Ok(reachable)
    }

    /// Whether "general delta" is enabled for this revlog.
    ///
    /// - If general_delta is true: `RevlogEntry.base` specifies the delta base.
    /// - If general_delta is false: `RevlogEntry.base` specifies the delta chain
    ///   `base..=rev`.
    ///
    /// Revlog written by this crate won't use deltas. This is only useful for
    /// reading ancient revisions in ancient revlogs.
    fn is_general_delta(&self) -> bool {
        match self.changelogi_data.get(0..4) {
            None => {
                // Empty revlog. Does not matter if it's general delta or not.
                true
            }
            Some(mut entry) => {
                // First 4 bytes: Revlog flags.
                let flags = entry.read_u32::<BE>().unwrap();
                const FLAG_GENERALDELTA: u32 = 1 << 17;
                flags & FLAG_GENERALDELTA != 0
            }
        }
    }
}

/// Minimal code to read the DAG (i.e. parents) stored in non-inlined revlog.
pub struct RevlogIndex {
    pub changelogi_data: Bytes,

    /// Inserted entries that are not flushed to disk.
    pub pending_parents: Vec<ParentRevs>,

    /// Inserted node -> index of pending_parents.
    pub pending_nodes: Vec<Vertex>,
    pub pending_nodes_index: BTreeMap<Vertex, usize>,
    pub pending_raw_data: Vec<Bytes>,

    /// Snapshot used to construct Set.
    snapshot: RwLock<Option<Arc<RevlogIndex>>>,

    /// File handler to the revlog data.
    data_handler: Mutex<Option<File>>,

    /// Index to convert node to rev.
    pub nodemap: NodeRevMap<BytesSlice<RevlogEntry>, BytesSlice<u32>>,

    /// Paths to the on-disk files.
    index_path: PathBuf,
    nodemap_path: PathBuf,

    /// Identity of the revlog.
    id: String,

    /// Version of the revlog.
    version: VerLink,
}

/// "smallvec" optimization
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ParentRevs {
    Compact([i32; 2]),
    // Box<Vec<i32>> takes 8 bytes. Box<[i32]> takes 16 bytes.
    // Using smaller struct for better memory efficiency.
    Octopus(Box<Vec<i32>>),
}

impl ParentRevs {
    fn from_p1p2(p1: i32, p2: i32) -> Self {
        Self::Compact([p1, p2])
    }

    fn from_vec(v: Vec<i32>) -> Self {
        Self::Octopus(Box::new(v))
    }

    pub fn as_revs(&self) -> &[u32] {
        let slice: &[i32] = match self {
            ParentRevs::Compact(s) => {
                if s[0] == -1 {
                    &s[0..0]
                } else if s[1] == -1 {
                    &s[0..1]
                } else {
                    &s[..]
                }
            }
            ParentRevs::Octopus(s) => &s[..],
        };
        let ptr = (slice as *const [i32]) as *const [u32];
        unsafe { &*ptr }
    }
}

/// Revlog entry. See "# index ng" in revlog.py.
#[repr(packed)]
#[derive(Copy, Clone, Debug)]
pub struct RevlogEntry {
    offset_flags: u64,
    compressed: i32,
    len: i32,
    base: i32,
    link: i32,
    p1: i32,
    p2: i32,
    pub node: [u8; 20],
    _padding: [u8; 12],
}

/// Bytes that can be converted to &[T].
#[derive(Clone)]
pub struct BytesSlice<T>(Bytes, PhantomData<T>);
pub trait UnsafeSliceCast {}

impl<T> From<Bytes> for BytesSlice<T> {
    fn from(bytes: Bytes) -> Self {
        Self(bytes, PhantomData)
    }
}

impl<T: UnsafeSliceCast> AsRef<[T]> for BytesSlice<T> {
    fn as_ref(&self) -> &[T] {
        let u8_slice: &[u8] = self.0.as_ref();
        // safety: buffer length matches. Bytes is expected to be mmap-ed or
        // static buffer and should be aligned.
        let ptr = u8_slice.as_ptr() as *const T;
        unsafe { slice::from_raw_parts(ptr, u8_slice.len() / mem::size_of::<T>()) }
    }
}
impl UnsafeSliceCast for u32 {}
impl UnsafeSliceCast for RevlogEntry {}

impl RevlogEntry {
    fn flags(&self) -> u16 {
        (self.offset_flags.to_be() & 0xffff) as u16
    }

    fn is_octopus_merge(&self) -> bool {
        (self.flags() & REVIDX_OCTOPUS_MERGE) != 0
    }
}

impl RevlogIndex {
    /// Constructs a RevlogIndex. The NodeRevMap is automatically manage>
    pub fn new(changelogi_path: &Path, nodemap_path: &Path) -> Result<Self> {
        // 20000 is chosen as it takes a few milliseconds to build up nodemap.
        Self::new_advanced(changelogi_path, nodemap_path, 20000)
    }

    /// Constructs a RevlogIndex with customized nodemap lag threshold.
    pub fn new_advanced(
        changelogi_path: &Path,
        nodemap_path: &Path,
        nodemap_lag_threshold: usize,
    ) -> Result<Self> {
        let empty_nodemap_data = Bytes::from(nodemap::empty_index_buffer());
        let nodemap_data = read_path(nodemap_path, None, empty_nodemap_data.clone())?;
        let changelogi_len = read_usize(&changelogi_path.with_extension("len"))?;
        let changelogi_data = read_path(changelogi_path, changelogi_len, Bytes::default())?;
        let nodemap = NodeRevMap::new(changelogi_data.clone().into(), nodemap_data.into())
            .or_else(|_| {
                // Attempt to rebuild the index (in-memory) automatically.
                NodeRevMap::new(changelogi_data.clone().into(), empty_nodemap_data.into())
            })?;
        if nodemap.lag() as usize > nodemap_lag_threshold {
            // The index is lagged, and less efficient. Update it.
            // Building is incremental. Writing to disk is not.
            if let Ok(buf) = nodemap.build_incrementally() {
                // Cast [u32] to [u8] for writing.
                let slice =
                    unsafe { slice::from_raw_parts(buf.as_ptr() as *const u8, buf.len() * 4) };
                // nodemap_path should be a symlink. Older repos might have the non-symlink
                // file. Attempt to delete it.
                // This is useful on Windows to prevent atomic_write_symlink failures when
                // the nodemap file was non-symlink and mmaped.
                if let Ok(meta) = nodemap_path.symlink_metadata() {
                    if !meta.file_type().is_symlink() {
                        remove_file(nodemap_path)?;
                    }
                }
                // Not fatal if we cannot update the on-disk index.
                let _ = atomic_write_symlink(nodemap_path, slice);
            }
        }
        let result = Self {
            nodemap,
            changelogi_data,
            pending_parents: Default::default(),
            pending_nodes: Default::default(),
            pending_nodes_index: Default::default(),
            pending_raw_data: Default::default(),
            snapshot: Default::default(),
            data_handler: Default::default(),
            index_path: changelogi_path.to_path_buf(),
            nodemap_path: nodemap_path.to_path_buf(),
            id: format!("rlog:{}", &nodemap_path.display()),
            version: VerLink::new(),
        };
        Ok(result)
    }

    /// Revisions in total.
    pub fn len(&self) -> usize {
        self.data_len() + self.pending_parents.len()
    }

    /// Revisions stored in the original revlog index.
    pub fn data_len(&self) -> usize {
        self.data().len()
    }

    #[inline]
    fn data(&self) -> &[RevlogEntry] {
        self.nodemap.changelogi.as_ref()
    }

    /// Get parent revisions.
    pub fn parent_revs(&self, rev: u32) -> dag::Result<ParentRevs> {
        let data_len = self.data_len();
        if rev >= data_len as u32 {
            return match self.pending_parents.get(rev as usize - data_len) {
                Some(revs) => Ok(revs.clone()),
                None => Id(rev as _).not_found(),
            };
        }

        let data = self.data();
        let entry = &data[rev as usize];
        let p1 = i32::from_be(entry.p1);
        let p2 = i32::from_be(entry.p2);
        if entry.is_octopus_merge() {
            let mut parents = Vec::with_capacity(3);
            if p1 >= 0 {
                parents.push(p1);
            }
            if p2 >= 0 {
                parents.push(p2);
            }
            // Read from the "stepparents" extra.
            let data = self.raw_data(rev)?;
            let stepparents = self.get_stepparents(&data)?;
            parents.extend(stepparents);
            Ok(ParentRevs::from_vec(parents))
        } else {
            Ok(ParentRevs::from_p1p2(p1, p2))
        }
    }

    /// Get the extra parents from raw data of a revision.
    fn get_stepparents(&self, data: &[u8]) -> Result<Vec<i32>> {
        // `data` format:
        // <manifest sha1> + '\n'
        // author + '\n'
        // date + ' ' + timezone + ' ' + (... + '\0')* + 'stepparents:' + (hexnode + ',')+

        let mut parents = Vec::new();

        #[derive(Copy, Clone)]
        enum State {
            ManifestLine,
            AuthorLine,
            Date,
            Timezone,
            Extras,
        }

        let mut state: State = State::ManifestLine;
        let mut extra_start = 0;
        for (i, b) in data.iter().enumerate() {
            match (state, b) {
                (State::ManifestLine, b'\n') => {
                    state = State::AuthorLine;
                }
                (State::AuthorLine, b'\n') => {
                    state = State::Date;
                }
                (State::Date, b' ') => {
                    state = State::Timezone;
                }
                (State::Timezone, b' ') => {
                    state = State::Extras;
                    extra_start = i + 1;
                }
                (State::Extras, b'\0') | (State::Extras, b'\n') => {
                    let extra = &data[extra_start..i]; // name:value
                    if let Some(value) = extra.strip_prefix(b"stepparents:") {
                        // Parse the stepparents field.
                        if let Ok(value) = std::str::from_utf8(value) {
                            for hex_node in value.split(',') {
                                let node = Vertex::from_hex(hex_node.as_bytes())?;
                                if let Some(id) = self.nodemap.node_to_rev(node.as_ref())? {
                                    parents.push(id as i32);
                                } else {
                                    return Err(crate::errors::CommitNotFound(node).into());
                                }
                            }
                        }
                    }
                    if *b == b'\n' {
                        break;
                    } else {
                        extra_start = i + 1;
                    }
                }
                (_, _) => {}
            }
        }
        Ok(parents)
    }

    /// Get raw content from a revision.
    pub fn raw_data(&self, rev: u32) -> Result<Bytes> {
        if rev as usize >= self.data_len() {
            let result = &self.pending_raw_data[rev as usize - self.data_len()];
            return Ok(result.clone());
        }
        let entry = self.data()[rev as usize];
        let offset = if rev == 0 {
            0
        } else {
            u64::from_be(entry.offset_flags) >> 16
        };

        let compressed_size = i32::from_be(entry.compressed) as usize;
        let mut chunk = vec![0; compressed_size];

        let mut locked = self.data_handler.lock();
        if locked.is_none() {
            let revlog_data_path = self.index_path.with_extension("d");
            let file = fs::OpenOptions::new().read(true).open(revlog_data_path)?;
            *locked = Some(file);
        }
        let file = locked.as_mut().unwrap();

        file.seek(io::SeekFrom::Start(offset))?;
        file.read_exact(&mut chunk)?;
        drop(locked);

        let decompressed = match chunk.get(0) {
            None => chunk,
            Some(b'\0') => chunk,
            Some(b'u') => {
                chunk.drain(0..1);
                chunk
            }
            Some(b'4') => lz4_pyframe::decompress(&chunk[1..])?,
            Some(&c) => return unsupported(format!("unsupported header: {:?}", c as char)),
        };

        let base = i32::from_be(entry.base);
        let result = if base != rev as i32 && base >= 0 {
            // Has a delta base. Load it recursively.
            // PERF: this is very inefficient (no caching, no folding delta chains), but is only
            // used for ancient data.
            let base = if self.is_general_delta() {
                base as u32
            } else {
                rev - 1
            };
            let base_text = self.raw_data(base)?;
            apply_deltas(&base_text, &decompressed)?
        } else {
            decompressed
        };

        Ok(Bytes::from(result))
    }

    /// Insert a new revision with given parents at the end.
    pub fn insert(&mut self, node: Vertex, parents: Vec<u32>, raw_data: Bytes) {
        if non_blocking_result(self.contains_vertex_name(&node)).unwrap_or(false) {
            return;
        }
        let parent_revs = if parents.len() <= 2 {
            let p1 = parents.get(0).map(|r| *r as i32).unwrap_or(-1);
            let p2 = parents.get(1).map(|r| *r as i32).unwrap_or(-1);
            ParentRevs::from_p1p2(p1, p2)
        } else {
            ParentRevs::from_vec(parents.into_iter().map(|i| i as i32).collect())
        };
        let idx = self.pending_parents.len();
        self.pending_parents.push(parent_revs);
        self.pending_nodes.push(node.clone());

        self.pending_nodes_index.insert(node, idx);
        *self.snapshot.write() = None;

        self.pending_raw_data.push(raw_data);

        self.version.bump();
    }

    fn pending_parent_map(&self) -> dag::Result<HashMap<Vec<u8>, Vec<Vec<u8>>>> {
        let mut result = HashMap::new();
        for i in 0..self.pending_nodes.len() {
            let parent_revs = &self.pending_parents[i];
            let parent_nodes = parent_revs
                .as_revs()
                .iter()
                .map(|&rev| {
                    non_blocking_result(self.vertex_name(Id(rev as _)))
                        .unwrap()
                        .as_ref()
                        .to_vec()
                })
                .collect::<Vec<_>>();
            let rev = self.data_len() + i;
            let node = non_blocking_result(self.vertex_name(Id(rev as _))).unwrap();
            result.insert(node.as_ref().to_vec(), parent_nodes);
        }
        Ok(result)
    }

    /// Write pending commits to disk.
    pub fn flush(&mut self) -> Result<()> {
        // Convert parent revs to parent nodes. This is because revs are
        // easy to get wrong: ex. some nodes already exist in updated revlog.
        let parent_map = self.pending_parent_map().map_err(|e| {
            CorruptionError::Generic(format!("cannot calculate pending graph: {}", e))
        })?;

        let _lock = ScopedDirLock::new(
            self.index_path
                .parent()
                .expect("index_path should not be root dir"),
        )?;

        let revlog_data_path = self.index_path.with_extension("d");
        let mut revlog_data = fs::OpenOptions::new()
            .read(true)
            .create(true)
            .append(true)
            .write(true)
            .open(&revlog_data_path)?;
        let mut revlog_index = fs::OpenOptions::new()
            .read(true)
            .create(true)
            .write(true)
            .open(&self.index_path)?;

        let meta_len_path = self.index_path.with_extension("len");
        let meta_len = read_usize(&meta_len_path)?;
        let revlog_index_size = match meta_len {
            // Use the logic length.
            Some(len) => revlog_index.seek(io::SeekFrom::Start(len as _))? as usize,
            // No explicit length. Use the physical length of the file.
            None => revlog_index.seek(io::SeekFrom::End(0))? as usize,
        };

        let old_rev_len = revlog_index_size / mem::size_of::<RevlogEntry>();
        let old_offset = revlog_data.seek(io::SeekFrom::End(0))?;

        if revlog_index_size % mem::size_of::<RevlogEntry>() != 0 {
            return corruption("changelog index length is not a multiple of 64");
        }

        if old_rev_len < self.data_len() {
            return corruption("changelog was truncated unexpectedly");
        }

        // Read from disk about new nodes. Do not write them again.
        // References to them will use new revs.
        // `existing_nodes` contain rev -> node relationship not known
        // to the current revlog snapshot.
        let mut existing_nodes = HashMap::new();
        if old_rev_len > self.data_len() {
            let data = BytesSlice::<RevlogEntry>::from(read_path(
                &self.index_path,
                Some(revlog_index_size),
                Bytes::new(),
            )?);
            for (i, entry) in data.as_ref()[self.data_len()..].iter().enumerate() {
                let rev = (self.data_len() + i) as u32;
                existing_nodes.insert(entry.node.as_ref().to_vec(), rev);
            }
        }

        let get_rev = |existing_nodes: &HashMap<Vec<u8>, u32>, node: &[u8]| -> Result<u32> {
            match existing_nodes.get(node) {
                Some(&rev) => Ok(rev),
                None => Ok(self.nodemap.node_to_rev(node)?.unwrap()),
            }
        };

        let mut new_data = Vec::new();
        let mut new_index = Vec::new();
        let mut i = 0;

        for (raw, node) in self.pending_raw_data.iter().zip(self.pending_nodes.iter()) {
            if existing_nodes.contains_key(node.as_ref()) {
                continue;
            }
            let raw_len = raw.len();
            let compressed = lz4_pyframe::compress(&raw)?;
            let chunk = if compressed.len() < raw.len() {
                // Use LZ4 compression ('4' header).
                let mut chunk = vec![b'4'];
                chunk.extend(compressed);
                chunk
            } else {
                // Do not use compression ('u' header).
                let mut chunk = vec![b'u'];
                chunk.extend(raw.as_ref().to_vec());
                chunk
            };

            let offset = old_offset + new_data.len() as u64;
            let rev = old_rev_len + i;
            i += 1;
            existing_nodes.insert(node.as_ref().to_vec(), rev as u32);

            let mut parent_revs: [i32; 2] = [-1, -1];
            let parents = &parent_map[node.as_ref()];
            for (p_id, p_node) in parents.iter().enumerate() {
                parent_revs[p_id] = get_rev(&existing_nodes, p_node)? as i32;
            }

            let mut flags = 0;
            if parents.len() > 2 || find_bytes_in_bytes(&raw, b"stepparents:").is_some() {
                flags |= REVIDX_OCTOPUS_MERGE;
            };
            let entry = RevlogEntry {
                offset_flags: u64::to_be((offset << 16) | (flags as u64)),
                compressed: i32::to_be(chunk.len() as i32),
                len: i32::to_be(raw_len as i32),
                base: i32::to_be(rev as i32),
                link: i32::to_be(rev as i32),
                p1: i32::to_be(parent_revs[0]),
                p2: i32::to_be(parent_revs[1]),
                node: <[u8; 20]>::try_from(node.as_ref()).map_err(|_| {
                    crate::Error::Unsupported(format!("node is not 20-char long: {:?}", &node))
                })?,
                _padding: [0u8; 12],
            };

            let entry_bytes: [u8; 64] = unsafe { std::mem::transmute(entry) };

            new_index.write_all(&entry_bytes)?;
            new_data.write_all(&chunk)?;
        }

        // Special case. First 6 bytes of the index file are not the offset but
        // specify the version and flags.
        if old_rev_len == 0 && new_index.len() > 4 {
            new_index[3] = 1; // revlog v1
        }

        // Write revlog data before revlog index.
        revlog_data.write_all(&new_data)?;
        drop(revlog_data);

        // Write revlog index.
        revlog_index.write_all(&new_index)?;
        drop(revlog_index);

        // Update meta file with the new logical length.
        let new_len = revlog_index_size + new_index.len();
        atomic_write_plain(&meta_len_path, format!("{}", new_len).as_bytes(), false)?;

        // Reload.
        *self = Self::new(&self.index_path, &self.nodemap_path)?;

        Ok(())
    }

    /// Create a Arc snapshot of IdConvert trait object on demand.
    pub fn get_snapshot(&self) -> Arc<Self> {
        let mut snapshot = self.snapshot.write();
        // Create snapshot on demand.
        match snapshot.deref() {
            Some(s) => s.clone(),
            None => {
                let result = Arc::new(self.clone());
                *snapshot = Some(result.clone());
                result
            }
        }
    }
}

impl Clone for RevlogIndex {
    fn clone(&self) -> Self {
        Self {
            pending_parents: self.pending_parents.clone(),
            pending_nodes: self.pending_nodes.clone(),
            pending_nodes_index: self.pending_nodes_index.clone(),
            pending_raw_data: self.pending_raw_data.clone(),
            snapshot: Default::default(),
            data_handler: Default::default(),
            nodemap: self.nodemap.clone(),
            changelogi_data: self.changelogi_data.clone(),
            index_path: self.index_path.clone(),
            nodemap_path: self.nodemap_path.clone(),
            id: self.id.clone(),
            version: self.version.clone(),
        }
    }
}

fn apply_deltas(base_text: &[u8], delta_text: &[u8]) -> Result<Vec<u8>> {
    struct Delta {
        start: usize,
        end: usize,
        data: Vec<u8>,
    }
    let mut deltas = Vec::new();
    let mut cursor = &delta_text[..];
    while !cursor.is_empty() {
        let start = cursor.read_u32::<BE>()?;
        let end = cursor.read_u32::<BE>()?;
        let len = cursor.read_u32::<BE>()?;
        let mut data = vec![0u8; len as usize];
        cursor.read_exact(&mut data)?;
        deltas.push(Delta {
            start: start as _,
            end: end as _,
            data,
        });
    }
    let mut result: Vec<u8> = Vec::new();
    let mut base_text_pos = 0;
    for delta in deltas {
        if base_text_pos < delta.start {
            result.extend_from_slice(&base_text[base_text_pos..delta.start]);
        }
        if !delta.data.is_empty() {
            result.extend(delta.data);
        }
        base_text_pos = delta.end;
    }
    if base_text_pos < base_text.len() {
        result.extend_from_slice(&base_text[base_text_pos..]);
    }
    Ok(result)
}

/// Read an integer from a `path`. If the file does not exist, or is empty,
/// return None.
fn read_usize(path: &Path) -> io::Result<Option<usize>> {
    match fs::OpenOptions::new().read(true).open(path) {
        Err(err) => {
            if err.kind() == io::ErrorKind::NotFound {
                Ok(None)
            } else {
                Err(err)
            }
        }
        Ok(mut file) => {
            let mut s = String::new();
            file.read_to_string(&mut s)?;
            if s.is_empty() {
                // This might happen with some file systems.
                // Just treat it as if the file does not exist.
                Ok(None)
            } else {
                s.parse::<usize>()
                    .map(Some)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
            }
        }
    }
}

fn read_path(path: &Path, length: Option<usize>, fallback: Bytes) -> io::Result<Bytes> {
    match fs::OpenOptions::new().read(true).open(path) {
        Err(err) => {
            if err.kind() == io::ErrorKind::NotFound {
                Ok(fallback)
            } else {
                Err(err)
            }
        }
        Ok(file) => mmap_bytes(&file, length.map(|size| size as u64)),
    }
}

#[async_trait::async_trait]
impl PrefixLookup for RevlogIndex {
    async fn vertexes_by_hex_prefix(
        &self,
        hex_prefix: &[u8],
        limit: usize,
    ) -> dag::Result<Vec<Vertex>> {
        // Search through the BTreeMap
        let start = Vertex::from_hex(hex_prefix)?;
        let mut result = Vec::new();
        for (vertex, _) in self.pending_nodes_index.range(start..) {
            if !vertex.to_hex().as_bytes().starts_with(hex_prefix) {
                break;
            }
            result.push(vertex.clone());
            if result.len() >= limit {
                break;
            }
        }
        // Search through the NodeRevMap
        match self.nodemap.hex_prefix_to_node(hex_prefix) {
            Ok(Some(node)) => {
                if limit > 0 {
                    result.push(node.to_vec().into());
                }
            }
            Ok(None) => {}
            Err(crate::Error::AmbiguousPrefix) => {
                // Convert AmbiguousPrefix to a non-error with multiple vertex pushed to
                // result.  That's what the Python code base expects.
                while result.len() < limit {
                    result.push(Vertex::from(Bytes::from_static(b"")));
                }
                return Ok(result);
            }
            Err(e) => return Err(e.into()),
        }
        Ok(result)
    }
}

#[async_trait::async_trait]
impl IdConvert for RevlogIndex {
    async fn vertex_id(&self, vertex: Vertex) -> dag::Result<Id> {
        if let Some(pending_id) = self.pending_nodes_index.get(&vertex) {
            Ok(Id((pending_id + self.data_len()) as _))
        } else if let Some(id) = self.nodemap.node_to_rev(vertex.as_ref())? {
            Ok(Id(id as _))
        } else {
            vertex.not_found()
        }
    }
    async fn vertex_id_with_max_group(
        &self,
        vertex: &Vertex,
        _max_group: Group,
    ) -> dag::Result<Option<Id>> {
        // RevlogIndex stores everything in the master group. So max_gorup is ignored.
        if let Some(pending_id) = self.pending_nodes_index.get(vertex) {
            Ok(Some(Id((pending_id + self.data_len()) as _)))
        } else if let Some(id) = self.nodemap.node_to_rev(vertex.as_ref())? {
            Ok(Some(Id(id as _)))
        } else {
            Ok(None)
        }
    }
    async fn vertex_name(&self, id: Id) -> dag::Result<Vertex> {
        let rev = id.0 as usize;
        if rev < self.data_len() {
            Ok(Vertex::from(self.data()[rev].node.as_ref().to_vec()))
        } else {
            match self.pending_nodes.get(rev - self.data_len()) {
                Some(node) => Ok(node.clone()),
                None => id.not_found(),
            }
        }
    }
    async fn contains_vertex_name(&self, vertex: &Vertex) -> dag::Result<bool> {
        if let Some(_pending_id) = self.pending_nodes_index.get(vertex) {
            Ok(true)
        } else if let Some(_id) = self.nodemap.node_to_rev(vertex.as_ref())? {
            Ok(true)
        } else {
            Ok(false)
        }
    }
    async fn contains_vertex_id_locally(&self, ids: &[Id]) -> dag::Result<Vec<bool>> {
        let mut list = Vec::with_capacity(ids.len());
        for id in ids {
            let rev = id.0 as usize;
            list.push(
                rev < self.data_len() || self.pending_nodes.get(rev - self.data_len()).is_some(),
            );
        }
        Ok(list)
    }
    async fn contains_vertex_name_locally(&self, names: &[Vertex]) -> dag::Result<Vec<bool>> {
        let mut list = Vec::with_capacity(names.len());
        for name in names {
            list.push(
                self.pending_nodes_index.contains_key(name)
                    || self.nodemap.node_to_rev(name.as_ref())?.is_some(),
            )
        }
        Ok(list)
    }
    fn map_id(&self) -> &str {
        &self.id
    }
    fn map_version(&self) -> &VerLink {
        &self.version
    }
}

#[async_trait::async_trait]
impl DagAlgorithm for RevlogIndex {
    /// Sort a `Set` topologically.
    async fn sort(&self, set: &Set) -> dag::Result<Set> {
        let hints = set.hints();
        if hints.contains(Flags::TOPO_DESC)
            && matches!(hints.dag_version(), Some(v) if v <= self.dag_version())
            && matches!(hints.id_map_version(), Some(v) if v <= self.map_version())
        {
            Ok(set.clone())
        } else {
            let mut spans = IdSet::empty();
            for name in set.iter()? {
                let id = self.vertex_id(name?).await?;
                spans.push(id);
            }
            let result = Set::from_spans_dag(spans, self)?;
            Ok(result)
        }
    }

    /// Get ordered parent vertexes.
    async fn parent_names(&self, name: Vertex) -> dag::Result<Vec<Vertex>> {
        let rev = self.vertex_id(name).await?.0 as u32;
        let parent_revs = self.parent_revs(rev)?;
        let parent_revs = parent_revs.as_revs();
        let mut result = Vec::with_capacity(parent_revs.len());
        for &rev in parent_revs {
            result.push(self.vertex_name(Id(rev as _)).await?);
        }
        Ok(result)
    }

    /// Returns a set that covers all vertexes tracked by this DAG.
    async fn all(&self) -> dag::Result<Set> {
        let id_set = if self.len() == 0 {
            IdSet::empty()
        } else {
            IdSet::from(Id(0)..=Id(self.len() as u64 - 1))
        };
        let result = Set::from_spans_dag(id_set, self)?;
        result.hints().add_flags(Flags::FULL);
        Ok(result)
    }

    /// Returns a set that covers all vertexes in the master group.
    async fn master_group(&self) -> dag::Result<Set> {
        self.all().await
    }

    /// Vertexes buffered, not persisted.
    async fn dirty(&self) -> dag::Result<Set> {
        let low = Id(self.data_len() as _);
        let next = Id(self.len() as _);
        let mut id_set = IdSet::empty();
        if next > low {
            id_set.push(low..=(next - 1));
        }
        let set = Set::from_spans_dag(id_set, self)?;
        Ok(set)
    }

    /// Calculates all ancestors reachable from any name from the given set.
    async fn ancestors(&self, set: Set) -> dag::Result<Set> {
        if set.hints().contains(Flags::ANCESTORS) {
            return Ok(set);
        }
        let id_set = self.to_id_set(&set).await?;
        if id_set.is_empty() {
            return Ok(Set::empty());
        }

        let dag = self.get_snapshot();
        let max_id = id_set.max().unwrap();
        let max_rev = max_id.0 as u32;

        let mut included = BitVec::from_elem(max_rev as usize + 1, false);
        for id in id_set.into_iter() {
            included.set(id.0 as usize, true);
        }

        let iter = (0..=max_rev).rev().filter_map(move |rev| {
            let should_include = included[rev as usize];
            if should_include {
                let parent_revs = match dag.parent_revs(rev) {
                    Ok(revs) => revs,
                    Err(e) => return Some(Err(e)),
                };
                for &p in parent_revs.as_revs() {
                    included.set(p as usize, true);
                }
            }
            if should_include {
                Some(Ok(Id(rev as _)))
            } else {
                None
            }
        });

        let set = Set::from_id_iter_dag(iter, self)?;
        set.hints()
            .add_flags(Flags::ID_DESC | Flags::TOPO_DESC | Flags::ANCESTORS)
            .set_max_id(max_id);
        Ok(set)
    }

    /// Calculates parents.
    async fn parents(&self, set: Set) -> dag::Result<Set> {
        let id_set = self.to_id_set(&set).await?;
        if id_set.is_empty() {
            return Ok(Set::empty());
        }

        let max_id = id_set.max().unwrap();
        let dag = self.get_snapshot();
        let min_parent = id_set.iter_desc().fold(max_id.0 as u32, |min, id| {
            let rev = id.0 as u32;
            if let Ok(parent_revs) = dag.parent_revs(rev) {
                parent_revs.as_revs().iter().fold(min, |min, &p| p.min(min))
            } else {
                min
            }
        }) as usize;

        let mut included = BitVec::from_elem(max_id.0 as usize - min_parent + 1, false);
        for id in id_set {
            let rev = id.0 as u32;
            for &p in dag.parent_revs(rev)?.as_revs() {
                included.set(p as usize - min_parent, true);
            }
        }

        // IdSet::push is O(1) if pushed in DESC order, otherwise it's O(N).
        let mut id_spans: IdSet = IdSet::empty();
        for rev in (min_parent..=max_id.0 as usize).rev() {
            if included[rev - min_parent] {
                id_spans.push(Id(rev as _));
            }
        }

        let idmap = dag.clone();
        let result = Set::from_spans_idmap_dag(id_spans, idmap, dag);
        Ok(result)
    }

    /// Calculates children of the given set.
    async fn children(&self, set: Set) -> dag::Result<Set> {
        let id_set = self.to_id_set(&set).await?;
        if id_set.is_empty() {
            return Ok(Set::empty());
        }

        let min_id = id_set.min().unwrap();
        let dag = self.get_snapshot();
        // Children: scan to the highest Id. Check parents.
        let iter = ((min_id.0 as u32)..(dag.len() as u32)).filter_map(move |rev| {
            let parent_revs = match dag.parent_revs(rev) {
                Ok(revs) => revs,
                Err(err) => return Some(Err(err)),
            };
            let should_include = parent_revs
                .as_revs()
                .iter()
                .any(|&p| id_set.contains(Id(p as _)));
            if should_include {
                Some(Ok(Id(rev as _)))
            } else {
                None
            }
        });

        let set = Set::from_id_iter_dag(iter, self)?;
        set.hints().add_flags(Flags::ID_ASC).set_min_id(min_id);
        Ok(set)
    }

    /// Calculates roots of the given set.
    async fn roots(&self, set: Set) -> dag::Result<Set> {
        let id_set = self.to_id_set(&set).await?;
        if id_set.is_empty() {
            return Ok(Set::empty());
        }
        let min_id = id_set.min().unwrap();
        let max_id = id_set.max().unwrap();
        let dag = self.get_snapshot();
        // Roots: [x for x in set if (parents(x) & set) is empty]
        let iter = id_set.clone().into_iter().filter_map(move |i| {
            let parent_revs = match dag.parent_revs(i.0 as _) {
                Ok(revs) => revs,
                Err(err) => return Some(Err(err)),
            };
            let should_include = parent_revs
                .as_revs()
                .iter()
                .all(|&p| !id_set.contains(Id(p as _)));
            if should_include { Some(Ok(i)) } else { None }
        });
        let set = Set::from_id_iter_dag(iter, self)?;
        set.hints()
            .add_flags(Flags::ID_DESC | Flags::TOPO_DESC)
            .set_min_id(min_id)
            .set_max_id(max_id);
        Ok(set)
    }

    /// Calculates one "greatest common ancestor" of the given set.
    ///
    /// If there are no common ancestors, return None.
    /// If there are multiple greatest common ancestors, pick one arbitrarily.
    /// Use `gca_all` to get all of them.
    async fn gca_one(&self, set: Set) -> dag::Result<Option<Vertex>> {
        let id_set = self.to_id_set(&set).await?;
        let mut revs: Vec<u32> = id_set.iter_desc().map(|id| id.0 as u32).collect();
        while revs.len() > 1 {
            let mut new_revs = Vec::new();
            // gca_revs takes at most 6 revs at one.
            for revs in revs.chunks(6) {
                let gcas = self.gca_revs(revs, 1)?;
                if gcas.is_empty() {
                    return Ok(None);
                } else {
                    new_revs.extend(gcas);
                }
            }
            // gca_revs needs de-duplicated revs to work.
            new_revs.sort_unstable();
            new_revs.dedup();
            revs = new_revs;
        }
        if revs.is_empty() {
            Ok(None)
        } else {
            Ok(Some(self.vertex_name(Id(revs[0] as _)).await?))
        }
    }

    /// Calculates all "greatest common ancestor"s of the given set.
    /// `gca_one` is faster if an arbitrary answer is ok.
    async fn gca_all(&self, set: Set) -> dag::Result<Set> {
        let id_set = self.to_id_set(&set).await?;
        // XXX: Limited by gca_revs implementation detail.
        if id_set.count() > 6 {
            return Err(Error::Unsupported(format!(
                "RevlogIndex::gca_all does not support set with > 6 items, got {} items",
                id_set.count()
            ))
            .into());
        }
        let revs: Vec<u32> = id_set.iter_desc().map(|id| id.0 as u32).collect();
        let gcas = self.gca_revs(&revs, usize::max_value())?;
        let spans = IdSet::from_spans(gcas.into_iter().map(|r| Id(r as _)));
        let result = Set::from_spans_dag(spans, self)?;
        Ok(result)
    }

    /// Tests if `ancestor` is an ancestor of `descendant`.
    async fn is_ancestor(&self, ancestor: Vertex, descendant: Vertex) -> dag::Result<bool> {
        let ancestor_rev = self.vertex_id(ancestor).await?.0 as u32;
        let descendant_rev = self.vertex_id(descendant).await?.0 as u32;
        if ancestor_rev == descendant_rev {
            return Ok(true);
        }
        Ok(self.gca_revs(&[ancestor_rev, descendant_rev], 1)?.get(0) == Some(&ancestor_rev))
    }

    /// Calculates "heads" of the ancestors of the given set. That is,
    /// Find Y, which is the smallest subset of set X, where `ancestors(Y)` is
    /// `ancestors(X)`.
    async fn heads_ancestors(&self, set: Set) -> dag::Result<Set> {
        let id_set = self.to_id_set(&set).await?;
        if id_set.is_empty() {
            return Ok(Set::empty());
        }

        let min_rev = id_set.min().unwrap().0 as usize;
        let max_rev = id_set.max().unwrap().0 as usize;
        assert!(self.len() > min_rev);
        assert!(self.len() > max_rev);
        let state_len = max_rev - min_rev + 1;

        #[repr(u8)]
        #[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
        enum State {
            Unspecified,
            PotentialHead,
            NotHead,
        }
        let mut states = if id_set.count() as usize == state_len {
            // Fast path: entire range of states are "Unspecified".
            vec![State::PotentialHead; state_len]
        } else {
            let mut states = vec![State::Unspecified; state_len];
            for id in id_set {
                states[id.0 as usize - min_rev] = State::PotentialHead;
            }
            states
        };

        let mut result_id_set = IdSet::empty();
        for i in (0..states.len()).rev() {
            let state = states[i];
            match state {
                State::Unspecified => {}
                State::PotentialHead | State::NotHead => {
                    let rev = i + min_rev;
                    if state == State::PotentialHead {
                        result_id_set.push(Id(rev as _));
                    }
                    for &parent_rev in self.parent_revs(rev as u32)?.as_revs() {
                        if parent_rev as usize >= min_rev {
                            states[parent_rev as usize - min_rev] = State::NotHead;
                        }
                    }
                }
            }
        }
        let result = Set::from_spans_dag(result_id_set, self)?;
        Ok(result)
    }

    /// Calculate the heads of the set.
    async fn heads(&self, set: Set) -> dag::Result<Set> {
        if set.hints().contains(Flags::ANCESTORS) {
            self.heads_ancestors(set).await
        } else {
            Ok(set.clone() - self.parents(set).await?)
        }
    }

    /// Calculates the "dag range" - vertexes reachable from both sides.
    async fn range(&self, roots: Set, heads: Set) -> dag::Result<Set> {
        let root_ids = self.to_id_set(&roots).await?;
        let head_ids = self.to_id_set(&heads).await?;
        let root_revs: Vec<u32> = root_ids.into_iter().map(|i| i.0 as u32).collect();
        let head_revs: Vec<u32> = head_ids.into_iter().map(|i| i.0 as u32).collect();
        let result_revs = self.range_revs(&root_revs, &head_revs)?;
        let result_id_set = IdSet::from_spans(result_revs.into_iter().map(|r| Id(r as _)));
        let result = Set::from_spans_dag(result_id_set, self)?;
        Ok(result)
    }

    /// Calculate `::reachable - ::unreachable`.
    async fn only(&self, reachable: Set, unreachable: Set) -> dag::Result<Set> {
        let reachable_ids = self.to_id_set(&reachable).await?;
        let unreachable_ids = self.to_id_set(&unreachable).await?;

        if reachable_ids.is_empty() {
            return Ok(Set::empty());
        } else if unreachable_ids.is_empty() {
            return self.ancestors(reachable).await;
        }

        let max_id = reachable_ids
            .max()
            .unwrap()
            .max(unreachable_ids.max().unwrap());

        // bits[i*2]: true if i is reachable from "reachable".
        // bits[i*2+1]: true if i is reachable from "unreachable".
        let mut bits = BitVec::from_elem(((max_id.0 + 1) * 2) as usize, false);

        // set "unreachable" heads.
        for id in unreachable_ids.iter_desc() {
            bits.set((id.0 * 2 + 1) as usize, true);
        }

        // alive: count of "id"s that might belong to the result set but haven't
        // been added to the result set yet.  alive == 0 indicates there is no
        // need to check more ids.
        let mut alive = 0;
        for id in reachable_ids.iter_desc() {
            if !bits[(id.0 * 2 + 1) as _] {
                bits.set((id.0 * 2) as usize, true);
                alive += 1;
            }
        }

        let mut result = IdSet::empty();
        for rev in (0..=max_id.0 as usize).rev() {
            let is_reachable = bits[rev * 2];
            let is_unreachable = bits[rev * 2 + 1];
            if is_unreachable {
                // Update unreachable's parents to unreachable.
                for p in self.parent_revs(rev as u32)?.as_revs() {
                    bits.set((p * 2 + 1) as usize, true);
                }
            } else if is_reachable {
                // Push to result - only reachable from 'reachable'.
                result.push(Id(rev as _));
                // Parents might belong to the result set.
                for p in self.parent_revs(rev as u32)?.as_revs() {
                    let i = (p * 2) as usize;
                    if !bits[i] && !bits[i + 1] {
                        bits.set(i, true);
                        alive += 1;
                    }
                }
            }
            if is_reachable {
                // This rev is processed. It's no longer a "potential".
                alive -= 1;
            }
            if alive == 0 {
                break;
            }
        }

        let result = Set::from_spans_dag(result, self)?;
        Ok(result)
    }

    /// Calculate `::reachable - ::unreachable` and `::unreachable`.
    async fn only_both(&self, reachable: Set, unreachable: Set) -> dag::Result<(Set, Set)> {
        let hints_ancestors_of_unreachable = unreachable.hints().clone();
        hints_ancestors_of_unreachable.update_flags_with(|f| f | Flags::ANCESTORS | Flags::ID_DESC);

        let reachable_ids = self.to_id_set(&reachable).await?;
        let unreachable_ids = self.to_id_set(&unreachable).await?;
        let reachable_revs = reachable_ids.into_iter().map(|i| i.0 as u32);
        let unreachable_revs = unreachable_ids.into_iter().map(|i| i.0 as u32);

        // Used internally. Different from "phases.py".
        #[repr(u8)]
        #[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
        enum Phase {
            Unspecified,
            Draft,
            Public,
        }
        impl Phase {
            fn max(self, other: Phase) -> Phase {
                if self > other { self } else { other }
            }
        }

        struct OnlyBothState {
            // Track the last_rev so we can resume from that location.
            last_rev: u64,
            // Track how many draft commits are remaining so we can stop processing after we've
            // seen them. (Unless the limit rev indicates we should go farther).
            remaining_draft: u64,
            reachable_set: IdSet,
            unreachable_set: IdSet,
            phases: Vec<Phase>,
            dag: Arc<RevlogIndex>,
        }
        let mut state = OnlyBothState {
            last_rev: self.len() as u64,
            remaining_draft: 0,
            reachable_set: IdSet::empty(),
            unreachable_set: IdSet::empty(),
            phases: vec![Phase::Unspecified; self.len()],
            dag: self.get_snapshot(),
        };

        for rev in reachable_revs {
            state.phases[rev as usize] = Phase::Draft;
            state.remaining_draft += 1;
        }
        for rev in unreachable_revs {
            if state.phases[rev as usize] == Phase::Draft {
                state.remaining_draft -= 1;
            }
            state.phases[rev as usize] = Phase::Public;
        }

        let arc_state = Arc::new(Mutex::new(state));

        let evaluate = Arc::new(
            move |limit_rev: u64| -> dag::Result<Arc<Mutex<OnlyBothState>>> {
                let mut guard = arc_state.lock();
                let state = &mut guard;

                // If we've already processed the requested rev, and we don't have any draft
                // commits remaining, exit early.
                if limit_rev >= state.last_rev && state.remaining_draft == 0 {
                    return Ok(arc_state.clone());
                }

                // Track the spans of public commits manually, instead of relying on constantly adding them
                // to a span. This is a hotpath and this optimization can save 100+ms.
                let mut start_public: i64 = -1;
                for rev in (0..state.last_rev).rev() {
                    state.last_rev = rev;

                    let phase = state.phases[rev as usize];
                    match phase {
                        Phase::Public => {
                            // Record the start of a public span.
                            if start_public == -1 {
                                start_public = rev as i64;
                            }
                        }
                        _ => {
                            // Record the end of a public span.
                            if start_public != -1 {
                                // Start is the end of the range because we're iterating in reverse order. So
                                // start is the later revision.
                                let span = Id(rev + 1)..=Id(start_public as u64);
                                state.unreachable_set.push(span);
                                start_public = -1;
                            }
                            if phase == Phase::Draft {
                                state.remaining_draft -= 1;
                                state.reachable_set.push(Id(rev));
                            }
                        }
                    }
                    for &parent_rev in state.dag.parent_revs(rev as u32)?.as_revs() {
                        // Propagate phases from this rev to its parents, tracking changes to the number of
                        // remaining drafts as we go.
                        let old_parent_phase = state.phases[parent_rev as usize];
                        let new_parent_phase = old_parent_phase.max(phase);
                        if new_parent_phase == Phase::Draft && old_parent_phase != Phase::Draft {
                            state.remaining_draft += 1;
                        }
                        if new_parent_phase == Phase::Public && old_parent_phase == Phase::Draft {
                            state.remaining_draft -= 1;
                        }
                        state.phases[parent_rev as usize] = new_parent_phase;
                    }

                    if rev <= limit_rev && state.remaining_draft == 0 {
                        break;
                    }
                }

                // Record any final public spans
                if start_public != -1 {
                    let last_rev = state.last_rev;
                    state
                        .unreachable_set
                        .push(Id(last_rev)..=Id(start_public as u64));
                }

                Ok(arc_state.clone())
            },
        );

        let dag = self.get_snapshot();

        // Kick off an initial evaluate to process all the draft commits.
        let state = (evaluate)(self.len() as u64)?;
        let reachable_set =
            Set::from_spans_idmap_dag(state.lock().reachable_set.clone(), dag.clone(), dag.clone());

        let eval_contains = evaluate.clone();
        let is_public = move |_: &MetaSet, v: &Vertex| -> dag::Result<bool> {
            let id = match non_blocking_result(dag.vertex_id(v.clone())) {
                Ok(id) => id,
                Err(DagError::VertexNotFound(_)) => return Ok(false),
                Err(e) => {
                    return Err(e);
                }
            };
            Ok((eval_contains)(id.0)?.lock().phases[id.0 as usize] == Phase::Public)
        };
        let unreachable_set = Set::from_evaluate_contains(
            move || {
                let state = (evaluate)(0)?;
                let guard = state.lock();

                Ok(Set::from_spans_idmap_dag(
                    guard.unreachable_set.clone(),
                    guard.dag.clone(),
                    guard.dag.clone(),
                ))
            },
            is_public,
            hints_ancestors_of_unreachable,
        );
        Ok((reachable_set, unreachable_set))
    }

    /// Calculates the descendants of the given set.
    async fn descendants(&self, set: Set) -> dag::Result<Set> {
        let id_set = self.to_id_set(&set).await?;
        if id_set.is_empty() {
            return Ok(Set::empty());
        }

        let min_id = id_set.min().unwrap();
        let min_rev = min_id.0 as u32;
        let dag = self.get_snapshot();
        let mut included = BitVec::from_elem(dag.len() - min_id.0 as usize, false);
        for id in id_set.into_iter() {
            included.set(id.0 as usize - min_id.0 as usize, true);
        }

        let iter = (min_rev..(dag.len() as u32)).filter_map(move |rev| {
            let should_include = included[(rev - min_rev) as usize] || {
                let parent_revs = match dag.parent_revs(rev) {
                    Ok(revs) => revs,
                    Err(err) => return Some(Err(err)),
                };
                parent_revs
                    .as_revs()
                    .iter()
                    .any(|&prev| prev >= min_rev && included[(prev - min_rev) as usize])
            };
            if should_include {
                included.set((rev - min_rev) as usize, true);
                Some(Ok(Id(rev as _)))
            } else {
                None
            }
        });

        let set = Set::from_id_iter_dag(iter, self)?;
        set.hints().add_flags(Flags::ID_ASC).set_min_id(min_id);
        Ok(set)
    }

    async fn reachable_roots(&self, roots: Set, heads: Set) -> dag::Result<Set> {
        let id_roots = self.to_id_set(&roots).await?;
        if id_roots.is_empty() {
            return Ok(Set::empty());
        }

        let id_heads = self.to_id_set(&heads).await?;
        if id_heads.is_empty() {
            return Ok(Set::empty());
        }

        let max_rev = id_heads.max().unwrap().0;
        let min_rev = id_roots.min().unwrap().0;
        if max_rev < min_rev {
            return Ok(Set::empty());
        }
        let mut reachable = BitVec::from_elem((max_rev + 1 - min_rev) as usize, false);
        let mut is_root = BitVec::from_elem((max_rev + 1 - min_rev) as usize, false);
        let mut result = IdSet::empty();

        // alive: count of "id"s that have unexplored parents.
        // alive == 0 indicates all parents are checked and iteration can be stopped.
        let mut alive = 0;
        for rev in id_heads.iter_desc() {
            let rev = rev.0;
            if rev <= max_rev && rev >= min_rev {
                reachable.set((rev - min_rev) as _, true);
                alive += 1;
            }
        }
        for rev in id_roots.iter_desc() {
            let rev = rev.0;
            if rev <= max_rev && rev >= min_rev {
                is_root.set((rev - min_rev) as _, true);
            }
        }

        for rev in (min_rev..=max_rev).rev() {
            if alive == 0 {
                break;
            }
            if !reachable[(rev - min_rev) as _] {
                continue;
            }
            alive -= 1;
            if is_root[(rev - min_rev) as _] {
                result.push(Id(rev as _));
                continue;
            }
            let parent_revs = self.parent_revs(rev as _)?;
            for parent_rev in parent_revs.as_revs() {
                let parent_rev = *parent_rev as u64;
                if parent_rev >= min_rev as _ && parent_rev <= max_rev as _ {
                    let idx = (parent_rev - min_rev) as usize;
                    if !reachable[idx] {
                        reachable.set(idx, true);
                        alive += 1;
                    }
                }
            }
        }

        let result = Set::from_spans_dag(result, self)?;
        Ok(result)
    }

    fn is_vertex_lazy(&self) -> bool {
        false
    }

    fn dag_snapshot(&self) -> dag::Result<Arc<dyn DagAlgorithm + Send + Sync>> {
        Ok(self.get_snapshot())
    }

    fn dag_id(&self) -> &str {
        &self.id
    }

    fn dag_version(&self) -> &VerLink {
        &self.version
    }
}

impl IdMapSnapshot for RevlogIndex {
    fn id_map_snapshot(&self) -> dag::Result<Arc<dyn IdConvert + Send + Sync>> {
        Ok(self.get_snapshot())
    }
}

impl RevlogIndex {
    fn add_heads_for_testing(
        &mut self,
        parents_func: &dyn Parents,
        heads: &VertexListWithOptions,
    ) -> dag::Result<bool> {
        if !cfg!(test) {
            panic!(
                "add_heads should only works for testing \
                   because it uses dummy commit message and \
                   revlog does not support separating commit \
                   messages from the graph"
            );
        }

        let mut updated = false;
        // Update IdMap. Keep track of what heads are added.
        for head in heads.vertexes() {
            if !non_blocking_result(self.contains_vertex_name(&head))? {
                let parents = non_blocking_result(parents_func.parent_names(head.clone()))?;
                for parent in parents.iter() {
                    self.add_heads_for_testing(parents_func, &vec![parent.clone()].into())?;
                }
                if !non_blocking_result(self.contains_vertex_name(&head))? {
                    let parent_revs: Vec<u32> = parents
                        .iter()
                        .map(|p| non_blocking_result(self.vertex_id(p.clone())).unwrap().0 as u32)
                        .collect();
                    if parent_revs.len() > 2 {
                        return Err(Error::Unsupported(format!(
                            "revlog does not support > 2 parents (when inserting {:?})",
                            &head
                        ))
                        .into());
                    }
                    let text = Bytes::from_static(b"DUMMY COMMIT MESSAGE FOR TESTING");
                    self.insert(head.clone(), parent_revs, text);
                    updated = true;
                }
            }
        }

        Ok(updated)
    }
}

#[async_trait::async_trait]
impl DagAddHeads for RevlogIndex {
    async fn add_heads(
        &mut self,
        parents_func: &dyn Parents,
        heads: &VertexListWithOptions,
    ) -> dag::Result<bool> {
        self.add_heads_for_testing(parents_func, heads)
    }
}

#[async_trait::async_trait]
impl CheckIntegrity for RevlogIndex {
    async fn check_universal_ids(&self) -> dag::Result<Vec<Id>> {
        unsupported_dag_error()
    }

    async fn check_segments(&self) -> dag::Result<Vec<String>> {
        unsupported_dag_error()
    }

    async fn check_isomorphic_graph(
        &self,
        other: &dyn DagAlgorithm,
        heads: dag::NameSet,
    ) -> dag::Result<Vec<String>> {
        let _ = (other, heads);
        unsupported_dag_error()
    }
}

fn unsupported_dag_error<T>() -> dag::Result<T> {
    Err(dag::errors::BackendError::Generic("unsupported by revlog index".to_string()).into())
}

fn find_bytes_in_bytes(bytes: &[u8], to_find: &[u8]) -> Option<usize> {
    bytes.windows(to_find.len()).position(|b| b == to_find)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering::SeqCst;

    use anyhow::Result;
    use nonblocking::non_blocking_result as r;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_simple_3_commits() -> Result<()> {
        // 00changelog.i and 00changelog.d are created by real hg commands.
        let changelog_i = from_xxd(
            r#"
0000000: 0000 0001 0000 0000 0000 002c 0000 0048  ...........,...H
0000010: 0000 0000 0000 0000 ffff ffff ffff ffff  ................
0000020: 785b 4df8 9f24 f67f fdd3 eec2 2cc4 c51f  x[M..$......,...
0000030: ca90 cc44 0000 0000 0000 0000 0000 0000  ...D............
0000040: 0000 0000 002c 0000 0000 0044 0000 0083  .....,.....D....
0000050: 0000 0001 0000 0001 0000 0000 ffff ffff  ................
0000060: 4028 e6d9 355e 415f ef6b 7a2b f8cf 4b53  @(..5^A_.kz+..KS
0000070: 0d66 2906 0000 0000 0000 0000 0000 0000  .f).............
0000080: 0000 0000 0070 0000 0000 004b 0000 004a  .....p.....K...J
0000090: 0000 0002 0000 0002 0000 0001 ffff ffff  ................
00000a0: ab7a efa7 ec2d 5e85 0295 6b2c 8fb4 2590  .z...-^...k,..%.
00000b0: 9c57 9822 0000 0000 0000 0000 0000 0000  .W."............
"#,
        );
        let changelog_d = from_xxd(
            r#"
0000000: 3448 0000 001f 3001 0014 f011 0a74 6573  4H....0......tes
0000010: 740a 3135 3936 3038 3335 3534 2032 3532  t.1596083554 252
0000020: 3030 0a0a 636f 6d6d 6974 2031 3483 0000  00..commit 14...
0000030: 001f 3001 0014 ff14 0a74 6573 740a 3135  ..0......test.15
0000040: 3936 3038 3335 3834 2032 3532 3030 0a0a  96083584 25200..
0000050: 636f 6d6d 6974 2032 3a20 3201 0015 f001  commit 2: 2.....
0000060: 2069 7320 636f 6d70 7265 7373 6962 6c65   is compressible
0000070: 7538 3531 3564 3462 6664 6137 3638 6530  u8515d4bfda768e0
0000080: 3461 6634 6331 3361 3639 6137 3265 3238  4af4c13a69a72e28
0000090: 6337 6566 6662 6561 370a 7465 7374 0a31  c7effbea7.test.1
00000a0: 3539 3630 3833 3631 3420 3235 3230 300a  596083614 25200.
00000b0: 610a 0a63 6f6d 6d69 7420 33              a..commit 3
"#,
        );

        let dir = tempdir()?;
        let dir = dir.path();
        let changelog_i_path = dir.join("00changelog.i");
        fs::write(&changelog_i_path, changelog_i)?;
        let changelog_d_path = dir.join("00changelog.d");
        fs::write(&changelog_d_path, changelog_d)?;
        let nodemap_path = dir.join("00changelog.nodemap");
        let index = RevlogIndex::new(&changelog_i_path, &nodemap_path)?;

        // Read parents.
        assert_eq!(index.parent_revs(0)?, ParentRevs::Compact([-1, -1]));
        assert_eq!(index.parent_revs(1)?, ParentRevs::Compact([0, -1]));
        assert_eq!(index.parent_revs(2)?, ParentRevs::Compact([1, -1]));

        // Read commit data.
        let read = |rev: u32| -> Result<String> {
            Ok(std::str::from_utf8(&index.raw_data(rev)?)?.to_string())
        };
        assert_eq!(
            read(0)?,
            "0000000000000000000000000000000000000000\ntest\n1596083554 25200\n\ncommit 1"
        );
        assert_eq!(
            read(1)?,
            r#"0000000000000000000000000000000000000000
test
1596083584 25200

commit 2: 22222222222222222222222222222222222222222 is compressible"#
        );
        // commit 3 is not lz4 compressed.
        assert_eq!(
            read(2)?,
            r#"8515d4bfda768e04af4c13a69a72e28c7effbea7
test
1596083614 25200
a

commit 3"#
        );

        Ok(())
    }

    #[test]
    fn test_delta_application() -> Result<()> {
        // util.py.i and util.py.d come from mercurial/util.py cloned from
        // https://www.mercurial-scm.org/repo/hg with lz4revlog enabled.
        // Truncated to first 3 revs.
        let util_py_i = from_xxd(
            r#"
0000000: 0002 0001 0000 0000 0000 016c 0000 0199  ...........l....
0000010: 0000 0000 0000 01a3 ffff ffff ffff ffff  ................
0000020: 83c9 892d 2313 c88d c0ae d198 5930 91ea  ...-#.......Y0..
0000030: 4c33 4f5f 0000 0000 0000 0000 0000 0000  L3O_............
0000040: 0000 0000 016c 0000 0000 0071 0000 0230  .....l.....q...0
0000050: 0000 0000 0000 01a5 0000 0000 ffff ffff  ................
0000060: fa42 82a9 8aaf 93c9 0c4c 89d0 e835 336f  .B.......L...53o
0000070: 875e f671 0000 0000 0000 0000 0000 0000  .^.q............
0000080: 0000 0000 01dd 0000 0000 00d4 0000 0393  ................
0000090: 0000 0001 0000 01a6 0000 0001 ffff ffff  ................
00000a0: 3326 e39d 9781 4b34 7c5b 4542 fc55 65c9  3&....K4|[EB.Ue.
00000b0: 7e6f b53d 0000 0000 0000 0000 0000 0000  ~o.=............
00000c0: 0000 0000 02b1 0000 0000 013e 0000 05c5  ...........>....
"#,
        );
        let util_py_d = from_xxd(
            r#"
0000000: 3499 0100 00b1 2320 7574 696c 2e70 7920  4.....# util.py 
0000010: 2d0a 00f1 1e69 7479 2066 756e 6374 696f  -....ity functio
0000020: 6e73 2061 6e64 2070 6c61 7466 6f72 6d20  ns and platform 
0000030: 7370 6563 6669 6320 696d 706c 656d 656e  specfic implemen
0000040: 7461 2500 f217 0a23 0a23 2043 6f70 7972  ta%....#.# Copyr
0000050: 6967 6874 2032 3030 3520 4b2e 2054 6861  ight 2005 K. Tha
0000060: 6e61 6e63 6861 7961 6e20 3c74 0e00 c16b  nanchayan <t...k
0000070: 4079 6168 6f6f 2e63 6f6d 3e38 00f1 0a54  @yahoo.com>8...T
0000080: 6869 7320 736f 6674 7761 7265 206d 6179  his software may
0000090: 2062 6520 7573 6564 7b00 9064 6973 7472   be used{..distr
00000a0: 6962 7574 1000 f10b 6363 6f72 6469 6e67  ibut....ccording
00000b0: 2074 6f20 7468 6520 7465 726d 730a 2320   to the terms.# 
00000c0: 6f66 0f00 f016 474e 5520 4765 6e65 7261  of....GNU Genera
00000d0: 6c20 5075 626c 6963 204c 6963 656e 7365  l Public License
00000e0: 2c20 696e 636f 7270 6f72 6149 00f2 3c68  , incorporaI..<h
00000f0: 6572 6569 6e20 6279 2072 6566 6572 656e  erein by referen
0000100: 6365 2e0a 0a69 6d70 6f72 7420 6f73 0a0a  ce...import os..
0000110: 6966 206f 732e 6e61 6d65 203d 3d20 276e  if os.name == 'n
0000120: 7427 3a0a 2020 2020 6465 6620 7063 6f6e  t':.    def pcon
0000130: 7665 7274 2870 6174 6829 1800 0001 0070  vert(path).....p
0000140: 7265 7475 726e 2016 00ff 092e 7265 706c  return .....repl
0000150: 6163 6528 225c 5c22 2c20 222f 2229 0a65  ace("\\", "/").e
0000160: 6c73 6545 0017 5061 7468 0a0a 34a3 0000  lseE..Path..4...
0000170: 0042 0000 0113 0400 f210 0097 6465 6620  .B..........def 
0000180: 7265 6e61 6d65 2873 7263 2c20 6473 7429  rename(src, dst)
0000190: 3a0a 2020 2020 7472 7909 0000 0100 3c6f  :.    try.....<o
00001a0: 732e 2600 011c 0069 6578 6365 7074 2800  s.&....iexcept(.
00001b0: 7575 6e6c 696e 6b28 2300 0f3f 0005 f00e  uunlink(#..?....
00001c0: 0a23 2050 6c61 7466 6f72 2073 7065 6369  .# Platfor speci
00001d0: 6669 6320 7661 7269 656e 7473 0a34 7b01  fic varients.4{.
00001e0: 0000 4200 0001 fd04 00f3 1700 da0a 2020  ..B...........  
00001f0: 2020 6465 6620 6d61 6b65 6c6f 636b 2869    def makelock(i
0000200: 6e66 6f2c 2070 6174 686e 616d 6529 3a0a  nfo, pathname):.
0000210: 2001 00d4 6c64 203d 206f 732e 6f70 656e   ...ld = os.open
0000220: 2820 0010 2c12 0092 4f5f 4352 4541 5420  ( ..,...O_CREAT 
0000230: 7c0d 0064 5752 4f4e 4c59 0e00 5545 5843  |..dWRONLY..UEXC
0000240: 4c29 4500 d06f 732e 7772 6974 6528 6c64  L)E..os.write(ld
0000250: 2c20 6b00 091b 0040 636c 6f73 1b00 2529  , k....@clos..%)
0000260: 0a98 0041 7265 6164 9800 0f92 0000 b672  ...Aread.......r
0000270: 6574 7572 6e20 6669 6c65 1f00 102e 3200  eturn file....2.
0000280: 7228 290a 0000 0230 0400 2f00 89e5 0017  r()....0../.....
0000290: 9d6f 732e 7379 6d6c 696e 2400 0f93 001a  .os.symlin$.....
00002a0: 216f 7387 0001 4800 0326 0050 6529 0a0a  !os...H..&.Pe)..
00002b0: 0a34 4a02 0000 4200 0001 be04 00f3 0e00  .4J...B.........
00002c0: 5c20 2020 2064 6566 2069 735f 6578 6563  \    def is_exec
"#,
        );

        let dir = tempdir()?;
        let dir = dir.path();
        let i_path = dir.join("util.py.i");
        fs::write(&i_path, util_py_i)?;
        let d_path = dir.join("util.py.d");
        fs::write(&d_path, util_py_d)?;
        let nodemap_path = dir.join("util.py.nodemap");
        let index = RevlogIndex::new(&i_path, &nodemap_path)?;
        let read = |rev: u32| index.raw_data(rev);

        assert_eq!(
            to_xxd(&read(0)?),
            r#"
00000000: 2320 7574 696c 2e70 7920 2d20 7574 696c  # util.py - util
00000010: 6974 7920 6675 6e63 7469 6f6e 7320 616e  ity functions an
00000020: 6420 706c 6174 666f 726d 2073 7065 6366  d platform specf
00000030: 6963 2069 6d70 6c65 6d65 6e74 6174 696f  ic implementatio
00000040: 6e73 0a23 0a23 2043 6f70 7972 6967 6874  ns.#.# Copyright
00000050: 2032 3030 3520 4b2e 2054 6861 6e61 6e63   2005 K. Thananc
00000060: 6861 7961 6e20 3c74 6861 6e61 6e63 6b40  hayan <thananck@
00000070: 7961 686f 6f2e 636f 6d3e 0a23 0a23 2054  yahoo.com>.#.# T
00000080: 6869 7320 736f 6674 7761 7265 206d 6179  his software may
00000090: 2062 6520 7573 6564 2061 6e64 2064 6973   be used and dis
000000a0: 7472 6962 7574 6564 2061 6363 6f72 6469  tributed accordi
000000b0: 6e67 2074 6f20 7468 6520 7465 726d 730a  ng to the terms.
000000c0: 2320 6f66 2074 6865 2047 4e55 2047 656e  # of the GNU Gen
000000d0: 6572 616c 2050 7562 6c69 6320 4c69 6365  eral Public Lice
000000e0: 6e73 652c 2069 6e63 6f72 706f 7261 7465  nse, incorporate
000000f0: 6420 6865 7265 696e 2062 7920 7265 6665  d herein by refe
00000100: 7265 6e63 652e 0a0a 696d 706f 7274 206f  rence...import o
00000110: 730a 0a69 6620 6f73 2e6e 616d 6520 3d3d  s..if os.name ==
00000120: 2027 6e74 273a 0a20 2020 2064 6566 2070   'nt':.    def p
00000130: 636f 6e76 6572 7428 7061 7468 293a 0a20  convert(path):. 
00000140: 2020 2020 2020 2072 6574 7572 6e20 7061         return pa
00000150: 7468 2e72 6570 6c61 6365 2822 5c5c 222c  th.replace("\\",
00000160: 2022 2f22 290a 656c 7365 3a0a 2020 2020   "/").else:.    
00000170: 6465 6620 7063 6f6e 7665 7274 2870 6174  def pconvert(pat
00000180: 6829 3a0a 2020 2020 2020 2020 7265 7475  h):.        retu
00000190: 726e 2070 6174 680a 0a                   rn path..
"#
        );
        assert_eq!(
            to_xxd(&read(1)?),
            r#"
00000000: 2320 7574 696c 2e70 7920 2d20 7574 696c  # util.py - util
00000010: 6974 7920 6675 6e63 7469 6f6e 7320 616e  ity functions an
00000020: 6420 706c 6174 666f 726d 2073 7065 6366  d platform specf
00000030: 6963 2069 6d70 6c65 6d65 6e74 6174 696f  ic implementatio
00000040: 6e73 0a23 0a23 2043 6f70 7972 6967 6874  ns.#.# Copyright
00000050: 2032 3030 3520 4b2e 2054 6861 6e61 6e63   2005 K. Thananc
00000060: 6861 7961 6e20 3c74 6861 6e61 6e63 6b40  hayan <thananck@
00000070: 7961 686f 6f2e 636f 6d3e 0a23 0a23 2054  yahoo.com>.#.# T
00000080: 6869 7320 736f 6674 7761 7265 206d 6179  his software may
00000090: 2062 6520 7573 6564 2061 6e64 2064 6973   be used and dis
000000a0: 7472 6962 7574 6564 2061 6363 6f72 6469  tributed accordi
000000b0: 6e67 2074 6f20 7468 6520 7465 726d 730a  ng to the terms.
000000c0: 2320 6f66 2074 6865 2047 4e55 2047 656e  # of the GNU Gen
000000d0: 6572 616c 2050 7562 6c69 6320 4c69 6365  eral Public Lice
000000e0: 6e73 652c 2069 6e63 6f72 706f 7261 7465  nse, incorporate
000000f0: 6420 6865 7265 696e 2062 7920 7265 6665  d herein by refe
00000100: 7265 6e63 652e 0a0a 696d 706f 7274 206f  rence...import o
00000110: 730a 0a64 6566 2072 656e 616d 6528 7372  s..def rename(sr
00000120: 632c 2064 7374 293a 0a20 2020 2074 7279  c, dst):.    try
00000130: 3a0a 2020 2020 2020 2020 6f73 2e72 656e  :.        os.ren
00000140: 616d 6528 7372 632c 2064 7374 290a 2020  ame(src, dst).  
00000150: 2020 6578 6365 7074 3a0a 2020 2020 2020    except:.      
00000160: 2020 6f73 2e75 6e6c 696e 6b28 6473 7429    os.unlink(dst)
00000170: 0a20 2020 2020 2020 206f 732e 7265 6e61  .        os.rena
00000180: 6d65 2873 7263 2c20 6473 7429 0a0a 2320  me(src, dst)..# 
00000190: 506c 6174 666f 7220 7370 6563 6966 6963  Platfor specific
000001a0: 2076 6172 6965 6e74 730a 6966 206f 732e   varients.if os.
000001b0: 6e61 6d65 203d 3d20 276e 7427 3a0a 2020  name == 'nt':.  
000001c0: 2020 6465 6620 7063 6f6e 7665 7274 2870    def pconvert(p
000001d0: 6174 6829 3a0a 2020 2020 2020 2020 7265  ath):.        re
000001e0: 7475 726e 2070 6174 682e 7265 706c 6163  turn path.replac
000001f0: 6528 225c 5c22 2c20 222f 2229 0a65 6c73  e("\\", "/").els
00000200: 653a 0a20 2020 2064 6566 2070 636f 6e76  e:.    def pconv
00000210: 6572 7428 7061 7468 293a 0a20 2020 2020  ert(path):.     
00000220: 2020 2072 6574 7572 6e20 7061 7468 0a0a     return path..
"#
        );
        assert_eq!(
            to_xxd(&read(2)?),
            r#"
00000000: 2320 7574 696c 2e70 7920 2d20 7574 696c  # util.py - util
00000010: 6974 7920 6675 6e63 7469 6f6e 7320 616e  ity functions an
00000020: 6420 706c 6174 666f 726d 2073 7065 6366  d platform specf
00000030: 6963 2069 6d70 6c65 6d65 6e74 6174 696f  ic implementatio
00000040: 6e73 0a23 0a23 2043 6f70 7972 6967 6874  ns.#.# Copyright
00000050: 2032 3030 3520 4b2e 2054 6861 6e61 6e63   2005 K. Thananc
00000060: 6861 7961 6e20 3c74 6861 6e61 6e63 6b40  hayan <thananck@
00000070: 7961 686f 6f2e 636f 6d3e 0a23 0a23 2054  yahoo.com>.#.# T
00000080: 6869 7320 736f 6674 7761 7265 206d 6179  his software may
00000090: 2062 6520 7573 6564 2061 6e64 2064 6973   be used and dis
000000a0: 7472 6962 7574 6564 2061 6363 6f72 6469  tributed accordi
000000b0: 6e67 2074 6f20 7468 6520 7465 726d 730a  ng to the terms.
000000c0: 2320 6f66 2074 6865 2047 4e55 2047 656e  # of the GNU Gen
000000d0: 6572 616c 2050 7562 6c69 6320 4c69 6365  eral Public Lice
000000e0: 6e73 652c 2069 6e63 6f72 706f 7261 7465  nse, incorporate
000000f0: 6420 6865 7265 696e 2062 7920 7265 6665  d herein by refe
00000100: 7265 6e63 652e 0a0a 696d 706f 7274 206f  rence...import o
00000110: 730a 0a64 6566 2072 656e 616d 6528 7372  s..def rename(sr
00000120: 632c 2064 7374 293a 0a20 2020 2074 7279  c, dst):.    try
00000130: 3a0a 2020 2020 2020 2020 6f73 2e72 656e  :.        os.ren
00000140: 616d 6528 7372 632c 2064 7374 290a 2020  ame(src, dst).  
00000150: 2020 6578 6365 7074 3a0a 2020 2020 2020    except:.      
00000160: 2020 6f73 2e75 6e6c 696e 6b28 6473 7429    os.unlink(dst)
00000170: 0a20 2020 2020 2020 206f 732e 7265 6e61  .        os.rena
00000180: 6d65 2873 7263 2c20 6473 7429 0a0a 2320  me(src, dst)..# 
00000190: 506c 6174 666f 7220 7370 6563 6966 6963  Platfor specific
000001a0: 2076 6172 6965 6e74 730a 6966 206f 732e   varients.if os.
000001b0: 6e61 6d65 203d 3d20 276e 7427 3a0a 2020  name == 'nt':.  
000001c0: 2020 6465 6620 7063 6f6e 7665 7274 2870    def pconvert(p
000001d0: 6174 6829 3a0a 2020 2020 2020 2020 7265  ath):.        re
000001e0: 7475 726e 2070 6174 682e 7265 706c 6163  turn path.replac
000001f0: 6528 225c 5c22 2c20 222f 2229 0a0a 2020  e("\\", "/")..  
00000200: 2020 6465 6620 6d61 6b65 6c6f 636b 2869    def makelock(i
00000210: 6e66 6f2c 2070 6174 686e 616d 6529 3a0a  nfo, pathname):.
00000220: 2020 2020 2020 2020 6c64 203d 206f 732e          ld = os.
00000230: 6f70 656e 2870 6174 686e 616d 652c 206f  open(pathname, o
00000240: 732e 4f5f 4352 4541 5420 7c20 6f73 2e4f  s.O_CREAT | os.O
00000250: 5f57 524f 4e4c 5920 7c20 6f73 2e4f 5f45  _WRONLY | os.O_E
00000260: 5843 4c29 0a20 2020 2020 2020 206f 732e  XCL).        os.
00000270: 7772 6974 6528 6c64 2c20 696e 666f 290a  write(ld, info).
00000280: 2020 2020 2020 2020 6f73 2e63 6c6f 7365          os.close
00000290: 286c 6429 0a0a 2020 2020 6465 6620 7265  (ld)..    def re
000002a0: 6164 6c6f 636b 2870 6174 686e 616d 6529  adlock(pathname)
000002b0: 3a0a 2020 2020 2020 2020 7265 7475 726e  :.        return
000002c0: 2066 696c 6528 7061 7468 6e61 6d65 292e   file(pathname).
000002d0: 7265 6164 2829 0a65 6c73 653a 0a20 2020  read().else:.   
000002e0: 2064 6566 2070 636f 6e76 6572 7428 7061   def pconvert(pa
000002f0: 7468 293a 0a20 2020 2020 2020 2072 6574  th):.        ret
00000300: 7572 6e20 7061 7468 0a0a 2020 2020 6465  urn path..    de
00000310: 6620 6d61 6b65 6c6f 636b 2869 6e66 6f2c  f makelock(info,
00000320: 2070 6174 686e 616d 6529 3a0a 2020 2020   pathname):.    
00000330: 2020 2020 6f73 2e73 796d 6c69 6e6b 2869      os.symlink(i
00000340: 6e66 6f2c 2070 6174 686e 616d 6529 0a0a  nfo, pathname)..
00000350: 2020 2020 6465 6620 7265 6164 6c6f 636b      def readlock
00000360: 2870 6174 686e 616d 6529 3a0a 2020 2020  (pathname):.    
00000370: 2020 2020 7265 7475 726e 206f 732e 7265      return os.re
00000380: 6164 6c69 6e6b 2870 6174 686e 616d 6529  adlink(pathname)
00000390: 0a0a 0a                                  ...
"#
        );

        Ok(())
    }

    #[test]
    fn test_octopus_merge() {
        let dir = tempdir().unwrap();
        let dir = dir.path();
        let changelog_i_path = dir.join("00changelog.i");
        let nodemap_path = dir.join("00changelog.nodemap");

        let mut rlog = RevlogIndex::new(&changelog_i_path, &nodemap_path).unwrap();
        rlog.insert(v(0), vec![], b"A".to_vec().into()); // rev 0
        rlog.insert(v(1), vec![], b"B".to_vec().into()); // rev 1
        rlog.insert(v(2), vec![], b"C".to_vec().into()); // rev 2
        rlog.insert(v(3), vec![], b"D".to_vec().into()); // rev 3
        rlog.insert(
            v(4),
            vec![0, 3],
            format!(
                concat!(
                    "deadbeef000000000000\n",
                    "test <test@example.com>\n",
                    "100 300 bar:foo\0stepparents:{},{}\0foo:bar\n",
                    "foobar"
                ),
                v(2).to_hex(),
                v(1).to_hex()
            )
            .as_bytes()
            .to_vec()
            .into(),
        ); // rev 4
        rlog.flush().unwrap();

        let rlog = RevlogIndex::new(&changelog_i_path, &nodemap_path).unwrap();
        assert_eq!(rlog.parent_revs(4).unwrap().as_revs(), [0, 3, 2, 1]);
    }

    #[test]
    fn test_flush() -> Result<()> {
        let dir = tempdir()?;
        let dir = dir.path();
        let changelog_i_path = dir.join("00changelog.i");
        let nodemap_path = dir.join("00changelog.nodemap");

        let mut revlog1 = RevlogIndex::new(&changelog_i_path, &nodemap_path)?;
        let mut revlog2 = RevlogIndex::new(&changelog_i_path, &nodemap_path)?;

        revlog1.insert(v(1), vec![], b"commit 1".to_vec().into()); // rev 0

        // commit 2 is lz4-friendly.
        let text = b"commit 2 (............................................)";
        revlog1.insert(v(2), vec![0], text.to_vec().into()); // rev 1, parent rev 0

        revlog2.insert(v(3), vec![], b"commit 3".to_vec().into()); // rev 2, local 0
        revlog2.insert(v(1), vec![], b"commit 1".to_vec().into()); // duplicate with revlog1, local 1, rev 0
        revlog2.insert(v(4), vec![0], b"commit 4".to_vec().into()); // rev 3, local 2
        revlog2.insert(v(5), vec![1, 0], b"commit 5".to_vec().into()); // rev 4, local 3

        // Inserting an existing node is ignored.
        let old_len = revlog1.len();
        revlog1.insert(v(1), vec![], b"commit 1".to_vec().into()); // rev 0
        revlog1.insert(v(2), vec![0], text.to_vec().into()); // rev 1
        assert_eq!(revlog1.len(), old_len);

        revlog1.flush()?;
        revlog2.flush()?;

        // The second flush reloads data, without writing new data.
        revlog1.flush()?;
        revlog2.flush()?;

        // Read the flushed data into revlog3.
        let revlog3 = RevlogIndex::new(&changelog_i_path, &nodemap_path)?;

        // Read parents.
        assert_eq!(revlog3.parent_revs(0)?, ParentRevs::Compact([-1, -1]));
        assert_eq!(revlog3.parent_revs(1)?, ParentRevs::Compact([0, -1]));
        assert_eq!(revlog3.parent_revs(2)?, ParentRevs::Compact([-1, -1]));
        assert_eq!(revlog3.parent_revs(3)?, ParentRevs::Compact([2, -1]));
        assert_eq!(revlog3.parent_revs(4)?, ParentRevs::Compact([0, 2])); // 0: "commit 1"

        // Prefix lookup.
        assert_eq!(r(revlog3.vertexes_by_hex_prefix(b"0303", 2))?, vec![v(3)]);

        // Id - Vertex.
        assert_eq!(r(revlog3.vertex_name(Id(2)))?, v(3));
        assert_eq!(r(revlog3.vertex_id(v(3)))?, Id(2));

        // Read commit data.
        let read = |rev: u32| -> Result<String> {
            let raw = revlog3.raw_data(rev)?;
            for index in vec![&revlog1, &revlog2] {
                assert!(index.raw_data(rev)? == raw, "index read mismatch");
            }
            Ok(std::str::from_utf8(&raw)?.to_string())
        };

        assert_eq!(read(0)?, "commit 1");
        assert_eq!(
            read(1)?,
            "commit 2 (............................................)"
        );
        assert_eq!(read(2)?, "commit 3");
        assert_eq!(read(3)?, "commit 4");
        assert_eq!(read(4)?, "commit 5");

        Ok(())
    }

    /// Quickly construct a Vertex from a byte.
    fn v(byte: u8) -> Vertex {
        Vertex::from(vec![byte; 20])
    }

    fn from_xxd(s: &str) -> Vec<u8> {
        s.lines()
            .flat_map(|line| {
                line.split("  ")
                    .nth(0)
                    .unwrap_or("")
                    .split(": ")
                    .nth(1)
                    .unwrap_or("")
                    .split(" ")
                    .flat_map(|s| {
                        (0..(s.len() / 2))
                            .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap())
                            .collect::<Vec<_>>()
                    })
            })
            .collect()
    }

    fn to_xxd(data: &[u8]) -> String {
        let mut result = String::with_capacity((data.len() + 15) / 16 * 67 + 1);
        result.push('\n');
        for (i, chunk) in data.chunks(16).enumerate() {
            result += &format!("{:08x}: ", i * 16);
            for slice in chunk.chunks(2) {
                match slice {
                    [b1, b2] => result += &format!("{:02x}{:02x} ", b1, b2),
                    [b1] => result += &format!("{:02x}   ", b1),
                    _ => {}
                }
            }
            result += &"     ".repeat(8 - (chunk.len() + 1) / 2);
            result += &" ";
            for &byte in chunk {
                let ch = match byte {
                    0x20..=0x7e => byte as char,
                    _ => '.',
                };
                result.push(ch);
            }
            result += "\n";
        }
        result
    }

    #[test]
    fn test_xxd() {
        let xxd = "00000000: 3132 3334 0a                             1234.";
        let bytes = from_xxd(xxd);
        assert_eq!(bytes, b"1234\n");
    }

    #[test]
    fn test_generic_dag() {
        let dir = tempdir().unwrap();
        let id = AtomicUsize::new(0);
        let new_dag = {
            let dir = dir.path();
            move || -> RevlogIndex {
                let id = id.fetch_add(1, SeqCst);
                let revlog_path = dir.join(format!("{}.i", id));
                let nodemap_path = dir.join(format!("{}.nodemap", id));
                RevlogIndex::new(&revlog_path, &nodemap_path).unwrap()
            }
        };
        dag::tests::test_generic_dag(&new_dag);
    }

    #[test]
    fn test_replace_non_symlink_mmap() -> Result<()> {
        // Test that while nodemap is 1. not a symlink (legacy setup); 2. being mmaped.
        // It can still be updated to a new version that is a symlink.
        // This test is more relevant on Windows.

        let dir = tempdir()?;
        let dir = dir.path();
        let changelog_i_path = dir.join("00changelog.i");
        let nodemap_path = dir.join("00changelog.nodemap");

        let mut revlog1 = RevlogIndex::new(&changelog_i_path, &nodemap_path)?;
        revlog1.insert(v(1), vec![], b"commit 1".to_vec().into());
        revlog1.flush()?;

        // Trigger nodemap build.
        let build_nodemap = || -> Result<()> {
            RevlogIndex::new_advanced(&changelog_i_path, &nodemap_path, 0)?;
            Ok(())
        };
        build_nodemap()?;

        // Convert nodemap to a non-symlink file.
        let nodemap_bytes = fs::read(&nodemap_path)?;
        assert!(nodemap_bytes.len() > 0);
        fs::remove_file(&nodemap_path)?;
        assert_eq!(
            nodemap_path.symlink_metadata().unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        fs::write(&nodemap_path, &nodemap_bytes)?;
        assert!(!nodemap_path.symlink_metadata()?.file_type().is_symlink());

        // Keep mmap on nodemap.
        let revlog2 = RevlogIndex::new(&changelog_i_path, &nodemap_path)?;

        // Make nodemap lagged.
        revlog1.insert(v(0xff), vec![], b"commit 1".to_vec().into());
        revlog1.flush()?;

        // Trigger nodemap build while keeping the mmap.
        build_nodemap()?;
        drop(revlog2);

        // The nodemap should be replaced to a symlink with more data.
        assert_ne!(
            &fs::read(&nodemap_path)?,
            &nodemap_bytes,
            "nodemap should be updated"
        );
        assert!(nodemap_path.symlink_metadata()?.file_type().is_symlink());

        Ok(())
    }
}
