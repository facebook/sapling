/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::nodemap;
use crate::NodeRevMap;
use anyhow::bail;
use anyhow::ensure;
use anyhow::Result;
use bit_vec::BitVec;
use dag::nameset::hints::Flags;
use dag::ops::DagAddHeads;
use dag::ops::DagAlgorithm;
use dag::ops::IdConvert;
use dag::ops::IdMapEq;
use dag::ops::IdMapSnapshot;
use dag::ops::PrefixLookup;
use dag::ops::ToIdSet;
use dag::Group;
use dag::Id;
use dag::IdSet;
use dag::Set;
use dag::Vertex;
use indexedlog::lock::ScopedDirLock;
use indexedlog::utils::mmap_bytes;
use minibytes::Bytes;
use parking_lot::Mutex;
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::convert::TryFrom;
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
use util::path::atomic_write_symlink;

impl RevlogIndex {
    /// Calculate `heads(ancestors(revs))`.
    pub fn headsancestors(&self, revs: Vec<u32>) -> Vec<u32> {
        if revs.is_empty() {
            return Vec::new();
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
                State::Unspecified => (),
                State::PotentialHead | State::NotHead => {
                    if state == State::PotentialHead {
                        result.push(rev as u32);
                    }
                    for &parent_rev in self.parent_revs(rev as u32).as_revs() {
                        if parent_rev >= min_rev {
                            states[(parent_rev - min_rev) as usize] = State::NotHead;
                        }
                    }
                }
            }
        }
        result
    }

    /// Given public and draft head revision numbers, calculate the "phase sets".
    /// Return (publicset, draftset).
    ///
    /// (only used when narrow-heads is disabled).
    pub fn phasesets(&self, publicheads: Vec<u32>, draftheads: Vec<u32>) -> (IdSet, IdSet) {
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
                if self > other {
                    self
                } else {
                    other
                }
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
                Phase::Unspecified => (),
            }
            for &parent_rev in self.parent_revs(rev as u32).as_revs() {
                // Propagate phases from this rev to its parents.
                phases[parent_rev as usize] = phases[parent_rev as usize].max(phase);
            }
        }
        (public_set, draft_set)
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
    /// by Matt Mackall in 2006 [3] takes 2 revs explicitly.
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
    pub fn gca_revs(&self, revs: &[u32], limit: usize) -> Vec<u32> {
        type BitMask = u8;
        let revcount = revs.len();
        assert!(revcount < 7);
        if revcount == 0 {
            return Vec::new();
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
                        return gca;
                    }
                    sv |= poison;
                    if revs.iter().any(|&r| r == v) {
                        break;
                    }
                }
            }
            for &p in self.parent_revs(v).as_revs() {
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

        gca
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
    pub fn range_revs(&self, roots: &[u32], heads: &[u32]) -> Vec<u32> {
        if roots.is_empty() || heads.is_empty() {
            return Vec::new();
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
            for &p in self.parent_revs(rev).as_revs() {
                if p >= min_root && revstates[(p - min_root) as usize] & RS_SEEN == 0 {
                    tovisit.push_back(p);
                    revstates[(p - min_root) as usize] |= RS_SEEN;
                }
            }
        }

        if reachable.is_empty() {
            return Vec::new();
        }

        // Find all the nodes in between the roots we found and the heads
        // and add them to the reachable set
        for rev in min_root..=max_head {
            if revstates[(rev - min_root) as usize] & RS_SEEN == 0 {
                continue;
            }
            if self
                .parent_revs(rev)
                .as_revs()
                .iter()
                .any(|&p| p >= min_root && revstates[(p - min_root) as usize] & RS_REACHABLE != 0)
                && revstates[(rev - min_root) as usize] & RS_REACHABLE == 0
            {
                revstates[(rev - min_root) as usize] |= RS_REACHABLE;
                reachable.push(rev);
            }
        }

        reachable
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
}

/// "smallvec" optimization
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ParentRevs([i32; 2]);

impl ParentRevs {
    fn from_p1p2(p1: i32, p2: i32) -> Self {
        Self([p1, p2])
    }

    pub fn as_revs(&self) -> &[u32] {
        let slice: &[i32] = if self.0[0] == -1 {
            &self.0[0..0]
        } else if self.0[1] == -1 {
            &self.0[0..1]
        } else {
            &self.0[..]
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

impl RevlogIndex {
    /// Constructs a RevlogIndex. The NodeRevMap is automatically manage>
    pub fn new(changelogi_path: &Path, nodemap_path: &Path) -> Result<Self> {
        let empty_nodemap_data = Bytes::from(nodemap::empty_index_buffer());
        let nodemap_data = read_path(nodemap_path, empty_nodemap_data.clone())?;
        let changelogi_data = read_path(changelogi_path, Bytes::default())?;
        let nodemap = NodeRevMap::new(changelogi_data.clone().into(), nodemap_data.into())
            .or_else(|_| {
                // Attempt to rebuild the index (in-memory) automatically.
                NodeRevMap::new(changelogi_data.clone().into(), empty_nodemap_data.into())
            })?;
        // 20000 is chosen as it takes a few milliseconds to build up.
        if nodemap.lag() > 20000 {
            // The index is lagged, and less efficient. Update it.
            // Building is incremental. Writing to disk is not.
            if let Ok(buf) = nodemap.build_incrementally() {
                // Cast [u32] to [u8] for writing.
                let slice =
                    unsafe { slice::from_raw_parts(buf.as_ptr() as *const u8, buf.len() * 4) };
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
    pub fn parent_revs(&self, rev: u32) -> ParentRevs {
        let data_len = self.data_len();
        if rev >= data_len as u32 {
            return self.pending_parents[rev as usize - data_len].clone();
        }

        let data = self.data();
        let p1 = i32::from_be(data[rev as usize].p1);
        let p2 = i32::from_be(data[rev as usize].p2);
        ParentRevs::from_p1p2(p1, p2)
    }

    /// Get raw content from a revision.
    pub fn raw_data(&self, rev: u32) -> Result<Bytes> {
        if rev as usize >= self.data_len() {
            let result = &self.pending_raw_data[rev as usize - self.data_len()];
            return Ok(result.clone());
        }
        let entry = self.data()[rev as usize];
        let base = i32::from_be(entry.base);
        ensure!(
            base == rev as i32 || base == -1,
            "delta-chain (base {}, rev {}) is unsupported when reading changelog",
            base,
            rev,
        );

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
        let decompressed = match chunk.get(0) {
            None => Bytes::new(),
            Some(b'u') | Some(b'\0') => Bytes::copy_from_slice(&chunk[1..]),
            Some(b'4') => Bytes::from(lz4_pyframe::decompress(&chunk[1..])?),
            Some(&c) => bail!("unsupported header: {:?}", c as char),
        };
        Ok(decompressed)
    }

    /// Insert a new revision with given parents at the end.
    pub fn insert(&mut self, node: Vertex, parents: Vec<u32>, raw_data: Bytes) {
        if self.contains_vertex_name(&node).unwrap_or(false) {
            return;
        }
        let p1 = parents.get(0).map(|r| *r as i32).unwrap_or(-1);
        let p2 = parents.get(1).map(|r| *r as i32).unwrap_or(-1);
        let idx = self.pending_parents.len();
        self.pending_parents.push(ParentRevs::from_p1p2(p1, p2));
        self.pending_nodes.push(node.clone());

        self.pending_nodes_index.insert(node, idx);
        *self.snapshot.write() = None;

        self.pending_raw_data.push(raw_data);
    }

    /// Write pending commits to disk.
    pub fn flush(&mut self) -> Result<()> {
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
            .append(true)
            .write(true)
            .open(&self.index_path)?;

        let old_rev_len =
            revlog_index.seek(io::SeekFrom::End(0))? as usize / mem::size_of::<RevlogEntry>();
        let old_offset = revlog_data.seek(io::SeekFrom::End(0))?;

        ensure!(
            old_rev_len >= self.data_len(),
            "changelog was truncated unexpectedly"
        );

        // Adjust `rev` to take possible changes on-disk into consideration.
        // For example,
        // - On-disk revlog.i has rev 0, rev 1.
        // - On-disk revlog.i loaded as RevlogIndex r1.
        // - r1.insert(...), got rev 2.
        // - On-disk revlog.i got appended to have rev 2.
        // - r1.flush(), rev 2 was taken by the on-disk revlog, it needs to be adjusted to rev 3.
        let adjust_rev = |rev: i32| -> i32 {
            if rev >= self.data_len() as _ {
                // need fixup.
                rev + old_rev_len as i32 - self.data_len() as i32
            } else {
                // rev is already on-disk - no need to fix.
                rev
            }
        };

        let mut new_data = Vec::new();
        let mut new_index = Vec::new();

        for (i, ((raw, node), parents)) in self
            .pending_raw_data
            .iter()
            .zip(self.pending_nodes.iter())
            .zip(self.pending_parents.iter())
            .enumerate()
        {
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

            let entry = RevlogEntry {
                offset_flags: u64::to_be(offset << 16),
                compressed: i32::to_be(chunk.len() as i32),
                len: i32::to_be(raw_len as i32),
                base: i32::to_be(rev as i32),
                link: i32::to_be(rev as i32),
                p1: i32::to_be(adjust_rev(parents.0[0])),
                p2: i32::to_be(adjust_rev(parents.0[1])),
                node: <[u8; 20]>::try_from(node.as_ref())?,
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
                let result = Arc::new(RevlogIndex {
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
                });
                *snapshot = Some(result.clone());
                result
            }
        }
    }
}

fn read_path(path: &Path, fallback: Bytes) -> io::Result<Bytes> {
    match fs::OpenOptions::new().read(true).open(path) {
        Err(err) => {
            if err.kind() == io::ErrorKind::NotFound {
                Ok(fallback)
            } else {
                Err(err)
            }
        }
        Ok(file) => mmap_bytes(&file, None),
    }
}

impl PrefixLookup for RevlogIndex {
    fn vertexes_by_hex_prefix(&self, hex_prefix: &[u8], limit: usize) -> Result<Vec<Vertex>> {
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
            Ok(None) => (),
            Err(e) => {
                if let Some(e) = e.downcast_ref::<radixbuf::errors::ErrorKind>() {
                    if e == &radixbuf::errors::ErrorKind::AmbiguousPrefix {
                        // Convert AmbiguousPrefix to a non-error with multiple vertex pushed to
                        // result.  That's what the Python code base expects.
                        while result.len() < limit {
                            result.push(Vertex::from(Bytes::from_static(b"")));
                        }
                        return Ok(result);
                    }
                }
                return Err(e);
            }
        }
        Ok(result)
    }
}

impl IdConvert for RevlogIndex {
    fn vertex_id(&self, vertex: Vertex) -> Result<Id> {
        if let Some(pending_id) = self.pending_nodes_index.get(&vertex) {
            Ok(Id((pending_id + self.data_len()) as _))
        } else if let Some(id) = self.nodemap.node_to_rev(vertex.as_ref())? {
            Ok(Id(id as _))
        } else {
            bail!("not found in revlog: {:?}", &vertex)
        }
    }
    fn vertex_id_with_max_group(&self, vertex: &Vertex, _max_group: Group) -> Result<Option<Id>> {
        // RevlogIndex stores everything in the master group. So max_gorup is ignored.
        if let Some(pending_id) = self.pending_nodes_index.get(vertex) {
            Ok(Some(Id((pending_id + self.data_len()) as _)))
        } else if let Some(id) = self.nodemap.node_to_rev(vertex.as_ref())? {
            Ok(Some(Id(id as _)))
        } else {
            Ok(None)
        }
    }
    fn vertex_name(&self, id: Id) -> Result<Vertex> {
        let id = id.0 as usize;
        if id < self.data_len() {
            Ok(Vertex::from(self.data()[id].node.as_ref().to_vec()))
        } else {
            match self.pending_nodes.get(id - self.data_len()) {
                Some(node) => Ok(node.clone()),
                None => bail!("rev {} not found", id),
            }
        }
    }
    fn contains_vertex_name(&self, vertex: &Vertex) -> Result<bool> {
        if let Some(_pending_id) = self.pending_nodes_index.get(vertex) {
            Ok(true)
        } else if let Some(_id) = self.nodemap.node_to_rev(vertex.as_ref())? {
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

impl DagAlgorithm for RevlogIndex {
    /// Sort a `Set` topologically.
    fn sort(&self, set: &Set) -> Result<Set> {
        if set.hints().contains(Flags::TOPO_DESC) {
            Ok(set.clone())
        } else {
            let mut spans = IdSet::empty();
            for name in set.iter()? {
                let id = self.vertex_id(name?)?;
                spans.push(id);
            }
            Ok(Set::from_spans_idmap(spans, self.get_snapshot()))
        }
    }

    /// Get ordered parent vertexes.
    fn parent_names(&self, name: Vertex) -> Result<Vec<Vertex>> {
        let rev = self.vertex_id(name)?.0 as u32;
        let parent_revs = self.parent_revs(rev);
        let parent_revs = parent_revs.as_revs();
        let mut result = Vec::with_capacity(parent_revs.len());
        for &rev in parent_revs {
            result.push(self.vertex_name(Id(rev as _))?);
        }
        Ok(result)
    }

    /// Returns a [`SpanSet`] that covers all vertexes tracked by this DAG.
    fn all(&self) -> Result<Set> {
        let id_set = if self.len() == 0 {
            IdSet::empty()
        } else {
            IdSet::from(Id(0)..=Id(self.len() as u64 - 1))
        };
        Ok(Set::from_spans_idmap(id_set, self.get_snapshot()))
    }

    /// Calculates all ancestors reachable from any name from the given set.
    fn ancestors(&self, set: Set) -> Result<Set> {
        let id_set = self.to_id_set(&set)?;
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

        let iter = (0..=max_rev)
            .rev()
            .filter(move |&rev| {
                let should_include = included[rev as usize];
                if should_include {
                    for &p in dag.parent_revs(rev).as_revs() {
                        included.set(p as usize, true);
                    }
                }
                should_include
            })
            .map(|rev| Ok(Id(rev as _)));

        let map = self.get_snapshot() as Arc<dyn IdConvert + Send + Sync>;
        let set = Set::from_iter_idmap(iter, map);
        set.hints().add_flags(Flags::ID_DESC | Flags::TOPO_DESC);
        set.hints().set_max_id(max_id);

        Ok(set)
    }

    /// Calculates children of the given set.
    fn children(&self, set: Set) -> Result<Set> {
        let id_set = self.to_id_set(&set)?;
        if id_set.is_empty() {
            return Ok(Set::empty());
        }

        let min_id = id_set.min().unwrap();
        let dag = self.get_snapshot();
        // Children: scan to the highest Id. Check parents.
        let iter = ((min_id.0 as u32)..(dag.len() as u32))
            .filter(move |&rev| {
                dag.parent_revs(rev)
                    .as_revs()
                    .iter()
                    .any(|&p| id_set.contains(Id(p as _)))
            })
            .map(|rev| Ok(Id(rev as _)));

        let map = self.get_snapshot() as Arc<dyn IdConvert + Send + Sync>;
        let set = Set::from_iter_idmap(iter, map);
        set.hints().add_flags(Flags::ID_ASC);
        set.hints().set_min_id(min_id);
        Ok(set)
    }

    /// Calculates roots of the given set.
    fn roots(&self, set: Set) -> Result<Set> {
        let id_set = self.to_id_set(&set)?;
        if id_set.is_empty() {
            return Ok(Set::empty());
        }
        let min_id = id_set.min().unwrap();
        let max_id = id_set.max().unwrap();
        let dag = self.get_snapshot();
        // Roots: [x for x in set if (parents(x) & set) is empty]
        let iter = id_set
            .clone()
            .into_iter()
            .filter(move |i| {
                dag.parent_revs(i.0 as _)
                    .as_revs()
                    .iter()
                    .all(|&p| !id_set.contains(Id(p as _)))
            })
            .map(Ok);
        let set = Set::from_iter_idmap(iter, self.get_snapshot());
        set.hints().add_flags(Flags::ID_DESC | Flags::TOPO_DESC);
        set.hints().set_min_id(min_id);
        set.hints().set_max_id(max_id);
        Ok(set)
    }

    /// Calculates one "greatest common ancestor" of the given set.
    ///
    /// If there are no common ancestors, return None.
    /// If there are multiple greatest common ancestors, pick one arbitrarily.
    /// Use `gca_all` to get all of them.
    fn gca_one(&self, set: Set) -> Result<Option<Vertex>> {
        let id_set = self.to_id_set(&set)?;
        let mut revs: Vec<u32> = id_set.iter().map(|id| id.0 as u32).collect();
        while revs.len() > 1 {
            let mut new_revs = Vec::new();
            // gca_revs takes at most 6 revs at one.
            for revs in revs.chunks(6) {
                let gcas = self.gca_revs(revs, 1);
                if gcas.is_empty() {
                    return Ok(None);
                } else {
                    new_revs.extend(gcas);
                }
            }
            revs = new_revs;
        }
        if revs.is_empty() {
            Ok(None)
        } else {
            Ok(Some(self.vertex_name(Id(revs[0] as _))?))
        }
    }

    /// Calculates all "greatest common ancestor"s of the given set.
    /// `gca_one` is faster if an arbitrary answer is ok.
    fn gca_all(&self, set: Set) -> Result<Set> {
        let id_set = self.to_id_set(&set)?;
        // XXX: Limited by gca_revs implementation detail.
        if id_set.count() > 6 {
            bail!("RevlogIndex::gca_all does not support set with > 6 items");
        }
        let revs: Vec<u32> = id_set.iter().map(|id| id.0 as u32).collect();
        let gcas = self.gca_revs(&revs, usize::max_value());
        let spans = IdSet::from_spans(gcas.into_iter().map(|r| Id(r as _)));
        Ok(Set::from_spans_idmap(spans, self.get_snapshot()))
    }

    /// Tests if `ancestor` is an ancestor of `descendant`.
    fn is_ancestor(&self, ancestor: Vertex, descendant: Vertex) -> Result<bool> {
        let ancestor_rev = self.vertex_id(ancestor)?.0 as u32;
        let descendant_rev = self.vertex_id(descendant)?.0 as u32;
        if ancestor_rev == descendant_rev {
            return Ok(true);
        }
        Ok(self.gca_revs(&[ancestor_rev, descendant_rev], 1).get(0) == Some(&ancestor_rev))
    }

    /// Calculates "heads" of the ancestors of the given set. That is,
    /// Find Y, which is the smallest subset of set X, where `ancestors(Y)` is
    /// `ancestors(X)`.
    fn heads_ancestors(&self, set: Set) -> Result<Set> {
        let id_set = self.to_id_set(&set)?;
        let revs: Vec<u32> = id_set.iter().map(|id| id.0 as u32).collect();
        let result_revs = self.headsancestors(revs);
        let result_id_set = IdSet::from_spans(result_revs.into_iter().map(|r| Id(r as _)));
        Ok(Set::from_spans_idmap(result_id_set, self.get_snapshot()))
    }

    /// Calculates the "dag range" - vertexes reachable from both sides.
    fn range(&self, roots: Set, heads: Set) -> Result<Set> {
        let root_ids = self.to_id_set(&roots)?;
        let head_ids = self.to_id_set(&heads)?;
        let root_revs: Vec<u32> = root_ids.into_iter().map(|i| i.0 as u32).collect();
        let head_revs: Vec<u32> = head_ids.into_iter().map(|i| i.0 as u32).collect();
        let result_revs = self.range_revs(&root_revs, &head_revs);
        let result_id_set = IdSet::from_spans(result_revs.into_iter().map(|r| Id(r as _)));
        Ok(Set::from_spans_idmap(result_id_set, self.get_snapshot()))
    }

    /// Calculate `::reachable - ::unreachable`.
    fn only(&self, reachable: Set, unreachable: Set) -> Result<Set> {
        let reachable_ids = self.to_id_set(&reachable)?;
        let unreachable_ids = self.to_id_set(&unreachable)?;

        if reachable_ids.is_empty() {
            return Ok(Set::empty());
        } else if unreachable_ids.is_empty() {
            return self.ancestors(reachable);
        }

        let max_id = reachable_ids
            .max()
            .unwrap()
            .max(unreachable_ids.max().unwrap());

        // bits[i*2]: true if i is reachable from "reachable".
        // bits[i*2+1]: true if i is reachable from "unreachable".
        let mut bits = BitVec::from_elem(((max_id.0 + 1) * 2) as usize, false);

        // set "unreachable" heads.
        for id in unreachable_ids.iter() {
            bits.set((id.0 * 2 + 1) as usize, true);
        }

        // alive: count of "id"s that might belong to the result set but haven't
        // been added to the result set yet.  alive == 0 indicates there is no
        // need to check more ids.
        let mut alive = 0;
        for id in reachable_ids.iter() {
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
                for p in self.parent_revs(rev as u32).as_revs() {
                    bits.set((p * 2 + 1) as usize, true);
                }
            } else if is_reachable {
                // Push to result - only reachable from 'reachable'.
                result.push(Id(rev as _));
                // Parents might belong to the result set.
                for p in self.parent_revs(rev as u32).as_revs() {
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

        let result = Set::from_spans_idmap(result, self.get_snapshot());
        Ok(result)
    }

    /// Calculate `::reachable - ::unreachable` and `::unreachable`.
    fn only_both(&self, reachable: Set, unreachable: Set) -> Result<(Set, Set)> {
        let reachable_ids = self.to_id_set(&reachable)?;
        let unreachable_ids = self.to_id_set(&unreachable)?;
        let reachable_revs: Vec<u32> = reachable_ids.into_iter().map(|i| i.0 as u32).collect();
        let unreachable_revs: Vec<u32> = unreachable_ids.into_iter().map(|i| i.0 as u32).collect();
        // This is a same problem to head-based public/draft phase calculation.
        let (result_unreachable_id_set, result_reachable_id_set) =
            self.phasesets(unreachable_revs, reachable_revs);
        Ok((
            Set::from_spans_idmap(result_reachable_id_set, self.get_snapshot()),
            Set::from_spans_idmap(result_unreachable_id_set, self.get_snapshot()),
        ))
    }

    /// Calculates the descendants of the given set.
    fn descendants(&self, set: Set) -> Result<Set> {
        let id_set = self.to_id_set(&set)?;
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

        let iter = (min_rev..(dag.len() as u32))
            .filter(move |&rev| {
                let should_include = included[(rev - min_rev) as usize]
                    || dag
                        .parent_revs(rev)
                        .as_revs()
                        .iter()
                        .any(|&prev| prev >= min_rev && included[(prev - min_rev) as usize]);
                if should_include {
                    included.set((rev - min_rev) as usize, true);
                }
                should_include
            })
            .map(|rev| Ok(Id(rev as _)));

        let map = self.get_snapshot() as Arc<dyn IdConvert + Send + Sync>;
        let set = Set::from_iter_idmap(iter, map);
        set.hints().add_flags(Flags::ID_ASC);
        set.hints().set_min_id(min_id);
        Ok(set)
    }
}

impl IdMapEq for RevlogIndex {
    fn is_map_compatible(&self, other: &Arc<dyn IdConvert + Send + Sync>) -> bool {
        Arc::ptr_eq(
            other,
            &(self.get_snapshot() as Arc<dyn IdConvert + Send + Sync>),
        )
    }
}

impl IdMapSnapshot for RevlogIndex {
    fn id_map_snapshot(&self) -> Result<Arc<dyn IdConvert + Send + Sync>> {
        Ok(self.get_snapshot())
    }
}

impl RevlogIndex {
    fn add_heads_for_testing<F>(&mut self, parents_func: &F, heads: &[Vertex]) -> Result<()>
    where
        F: Fn(Vertex) -> Result<Vec<Vertex>>,
    {
        if !cfg!(test) {
            panic!(
                "add_heads should only works for testing \
                   because it uses dummy commit message and \
                   revlog does not support separating commit \
                   messages from the graph"
            );
        }

        // Update IdMap. Keep track of what heads are added.
        for head in heads.iter() {
            if !self.contains_vertex_name(&head)? {
                let parents = parents_func(head.clone())?;
                for parent in parents.iter() {
                    self.add_heads_for_testing(parents_func, &[parent.clone()])?;
                }
                if !self.contains_vertex_name(&head)? {
                    let parent_revs: Vec<u32> = parents
                        .iter()
                        .map(|p| self.vertex_id(p.clone()).unwrap().0 as u32)
                        .collect();
                    if parent_revs.len() > 2 {
                        bail!(
                            "revlog does not support > 2 parents (when inserting {:?})",
                            &head
                        );
                    }
                    let text = Bytes::from_static(b"DUMMY COMMIT MESSAGE FOR TESTING");
                    self.insert(head.clone(), parent_revs, text);
                }
            }
        }

        Ok(())
    }
}

impl DagAddHeads for RevlogIndex {
    fn add_heads<F>(&mut self, parents_func: F, heads: &[Vertex]) -> Result<()>
    where
        F: Fn(Vertex) -> Result<Vec<Vertex>>,
    {
        self.add_heads_for_testing(&parents_func, heads)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering::SeqCst;
    use tempfile::tempdir;

    #[test]
    fn test_simple_3_commits() -> Result<()> {
        // 00changelog.i and 00changelog.d are created by real hg commands.
        let changelog_i = from_xxd(
            r#"
00000000: 0000 0001 0000 0000 0000 003d 0000 0059  ...........=...Y
00000010: 0000 0000 0000 0000 ffff ffff ffff ffff  ................
00000020: 5d97 babb c8ff 796c c63e 3d1b d85d 303c  ].....yl.>=..]0<
00000030: 6741 d574 0000 0000 0000 0000 0000 0000  gA.t............
00000040: 0000 0000 003d 0000 0000 0046 0000 00c4  .....=.....F....
00000050: 0000 0001 0000 0001 0000 0000 ffff ffff  ................
00000060: 116d 9f6f fa04 e925 47bd 7a4c e3d2 6bb2  .m.o...%G.zL..k.
00000070: fe3e aa71 0000 0000 0000 0000 0000 0000  .>.q............
00000080: 0000 0000 0083 0000 0000 005c 0000 005b  ...........\...[
00000090: 0000 0002 0000 0002 0000 0001 ffff ffff  ................
000000a0: d9ab b672 e662 0779 042a 5309 f250 60f1  ...r.b.y.*S..P`.
000000b0: 0f6a bf0d 0000 0000 0000 0000 0000 0000  .j..............
"#,
        );
        let changelog_d = from_xxd(
            r#"
00000000: 3459 0000 001f 3001 0014 f022 0a4a 756e  4Y....0....".Jun
00000010: 2057 7520 3c71 7561 726b 4066 622e 636f   Wu <quark@fb.co
00000020: 6d3e 0a31 3539 3131 3336 3439 3420 3235  m>.1591136494 25
00000030: 3230 300a 0a63 6f6d 6d69 7420 3134 c400  200..commit 14..
00000040: 0000 1f30 0100 14ff 220a 4a75 6e20 5775  ...0....".Jun Wu
00000050: 203c 7175 6172 6b40 6662 2e63 6f6d 3e0a   <quark@fb.com>.
00000060: 3135 3931 3133 3635 3036 2032 3532 3030  1591136506 25200
00000070: 0a0a 636f 6d6d 6974 2032 0100 5350 3232  ..commit 2..SP22
00000080: 3232 3275 3835 3135 6434 6266 6461 3736  222u8515d4bfda76
00000090: 3865 3034 6166 3463 3133 6136 3961 3732  8e04af4c13a69a72
000000a0: 6532 3863 3765 6666 6265 6137 0a4a 756e  e28c7effbea7.Jun
000000b0: 2057 7520 3c71 7561 726b 4066 622e 636f   Wu <quark@fb.co
000000c0: 6d3e 0a31 3539 3131 3339 3531 3620 3235  m>.1591139516 25
000000d0: 3230 300a 610a 0a63 6f6d 6d69 7420 33    200.a..commit 3
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
        assert_eq!(index.parent_revs(0), ParentRevs([-1, -1]));
        assert_eq!(index.parent_revs(1), ParentRevs([0, -1]));
        assert_eq!(index.parent_revs(2), ParentRevs([1, -1]));

        // Read commit data.
        let read = |rev: u32| -> Result<String> {
            Ok(std::str::from_utf8(&index.raw_data(rev)?)?.to_string())
        };
        assert_eq!(
            read(0)?,
            r#"0000000000000000000000000000000000000000
Jun Wu <quark@fb.com>
1591136494 25200

commit 1"#
        );
        assert_eq!(
            read(1)?,
            r#"0000000000000000000000000000000000000000
Jun Wu <quark@fb.com>
1591136506 25200

commit 222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222222"#
        );
        // commit 3 is not lz4 compressed.
        assert_eq!(
            read(2)?,
            r#"8515d4bfda768e04af4c13a69a72e28c7effbea7
Jun Wu <quark@fb.com>
1591139516 25200
a

commit 3"#
        );

        Ok(())
    }

    #[test]
    fn test_flush() -> Result<()> {
        let dir = tempdir()?;
        let dir = dir.path();
        let changelog_i_path = dir.join("00changelog.i");
        let nodemap_path = dir.join("00changelog.nodemap");

        let mut revlog1 = RevlogIndex::new(&changelog_i_path, &nodemap_path)?;
        let mut revlog2 = RevlogIndex::new(&changelog_i_path, &nodemap_path)?;

        revlog1.insert(v(1), vec![], b"commit 1".to_vec().into());

        // commit 2 is lz4-friendly.
        let text = b"commit 2 (............................................)";
        revlog1.insert(v(2), vec![0], text.to_vec().into());

        revlog2.insert(v(3), vec![], b"commit 3".to_vec().into());
        revlog2.insert(v(4), vec![0], b"commit 4".to_vec().into());
        revlog2.insert(v(5), vec![1, 0], b"commit 5".to_vec().into());

        // Inserting an existing node is ignored.
        let old_len = revlog1.len();
        revlog1.insert(v(1), vec![], b"commit 1".to_vec().into());
        revlog1.insert(v(2), vec![0], text.to_vec().into());
        assert_eq!(revlog1.len(), old_len);

        revlog1.flush()?;
        revlog2.flush()?;

        // The second flush reloads data, without writing new data.
        revlog1.flush()?;
        revlog2.flush()?;

        // Read the flushed data into revlog3.
        let revlog3 = RevlogIndex::new(&changelog_i_path, &nodemap_path)?;

        // Read parents.
        assert_eq!(revlog3.parent_revs(0), ParentRevs([-1, -1]));
        assert_eq!(revlog3.parent_revs(1), ParentRevs([0, -1]));
        assert_eq!(revlog3.parent_revs(2), ParentRevs([-1, -1]));
        assert_eq!(revlog3.parent_revs(3), ParentRevs([2, -1]));
        assert_eq!(revlog3.parent_revs(4), ParentRevs([3, 2]));

        // Prefix lookup.
        assert_eq!(revlog3.vertexes_by_hex_prefix(b"0303", 2)?, vec![v(3)]);

        // Id - Vertex.
        assert_eq!(revlog3.vertex_name(Id(2))?, v(3));
        assert_eq!(revlog3.vertex_id(v(3))?, Id(2));

        // Read commit data.
        let read = |rev: u32| -> Result<String> {
            let raw = revlog3.raw_data(rev)?;
            for index in vec![&revlog1, &revlog2] {
                ensure!(index.raw_data(rev)? == raw, "index read mismatch");
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
        dag::tests::test_generic_dag(new_dag);
    }
}
