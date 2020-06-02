/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::nodemap;
use crate::NodeRevMap;
use anyhow::bail;
use anyhow::Result;
use dag::ops::IdConvert;
use dag::ops::PrefixLookup;
use dag::Group;
use dag::Id;
use dag::IdSet;
use dag::Vertex;
use indexedlog::utils::{atomic_write, mmap_bytes};
use minibytes::Bytes;
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::fs;
use std::io;
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
                    for &parent_rev in self.parents(rev as u32).as_revs() {
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
            for &parent_rev in self.parents(rev as u32).as_revs() {
                // Propagate phases from this rev to its parents.
                phases[parent_rev as usize] = phases[parent_rev as usize].max(phase);
            }
        }
        (public_set, draft_set)
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
        let nodemap_data = read_path(nodemap_path, empty_nodemap_data.clone())?;
        let changelogi_data = read_path(changelogi_path, Bytes::default())?;
        let nodemap =
            NodeRevMap::new(changelogi_data.into(), nodemap_data.into()).or_else(|_| {
                // Attempt to rebuild the index automatically.
                let changelogi_data = read_path(changelogi_path, Bytes::default())?;
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
    pub fn parents(&self, rev: u32) -> ParentRevs {
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
