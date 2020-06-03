/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::nodemap;
use crate::utils::read_path;
use crate::NodeRevMap;
use anyhow::bail;
use anyhow::Result;
use bit_vec::BitVec;
use dag::nameset::hints::Flags;
use dag::ops::DagAlgorithm;
use dag::ops::IdConvert;
use dag::ops::IdMapEq;
use dag::ops::PrefixLookup;
use dag::ops::ToIdSet;
use dag::Group;
use dag::Id;
use dag::IdSet;
use dag::Set;
use dag::Vertex;
use indexedlog::utils::atomic_write;
use minibytes::Bytes;
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::mem;
use std::ops::Deref;
use std::path::Path;
use std::slice;
use std::sync::Arc;

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
    /// Inserted entries that are not flushed to disk.
    pub pending_parents: Vec<ParentRevs>,

    /// Inserted node -> index of pending_parents.
    pub pending_nodes: Vec<Vertex>,
    pub pending_nodes_index: BTreeMap<Vertex, usize>,

    /// Snapshot used to construct Set.
    snapshot: RwLock<Option<Arc<RevlogIndex>>>,

    /// Index to convert node to rev.
    pub nodemap: NodeRevMap<BytesSlice<RevlogEntry>, BytesSlice<u32>>,
}

/// "smallvec" optimization
#[derive(Clone, Copy)]
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
#[derive(Copy, Clone)]
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
        let nodemap_data = read_path(nodemap_path, empty_nodemap_data.clone(), false)?;
        let changelogi_data = read_path(changelogi_path, Bytes::default(), true)?;
        let nodemap =
            NodeRevMap::new(changelogi_data.into(), nodemap_data.into()).or_else(|_| {
                // Attempt to rebuild the index automatically.
                let changelogi_data = read_path(changelogi_path, Bytes::default(), true)?;
                NodeRevMap::new(changelogi_data.into(), empty_nodemap_data.into())
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
                let _ = atomic_write(nodemap_path, slice, false);
            }
        }
        let result = Self {
            nodemap,
            pending_parents: Default::default(),
            pending_nodes: Default::default(),
            pending_nodes_index: Default::default(),
            snapshot: Default::default(),
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

    /// Insert a new revision with given parents at the end.
    pub fn insert(&mut self, node: Vertex, parents: Vec<u32>) {
        let p1 = parents.get(0).map(|r| *r as i32).unwrap_or(-1);
        let p2 = parents.get(1).map(|r| *r as i32).unwrap_or(-1);
        self.pending_parents.push(ParentRevs::from_p1p2(p1, p2));
        self.pending_nodes.push(node.clone());

        let idx = self.pending_parents.len();
        self.pending_nodes_index.insert(node, idx);
        *self.snapshot.write() = None;
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
                    snapshot: Default::default(),
                    nodemap: self.nodemap.clone(),
                });
                *snapshot = Some(result.clone());
                result
            }
        }
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
        if let Some(node) = self.nodemap.hex_prefix_to_node(hex_prefix)? {
            result.push(node.to_vec().into());
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
