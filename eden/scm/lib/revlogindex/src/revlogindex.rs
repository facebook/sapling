/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::nodemap;
use crate::NodeRevMap;
use anyhow::Result;
use dag::Id;
use dag::IdSet;
use indexedlog::utils::{atomic_write, mmap_bytes};
use minibytes::Bytes;
use std::cell::RefCell;
use std::fs;
use std::io;
use std::marker::PhantomData;
use std::mem;
use std::path::Path;
use std::slice;

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
    pub inserted: RefCell<Vec<ParentRevs>>,

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
    pub node: [u8; 32],
}

/// Bytes that can be converted to &[T].
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
            inserted: Default::default(),
        };
        Ok(result)
    }

    /// Revisions in total.
    pub fn len(&self) -> usize {
        let inserted = self.inserted.borrow();
        self.data_len() + inserted.len()
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
            let inserted = self.inserted.borrow();
            return inserted[rev as usize - data_len].clone();
        }

        let data = self.data();
        let p1 = i32::from_be(data[rev as usize].p1);
        let p2 = i32::from_be(data[rev as usize].p2);
        ParentRevs::from_p1p2(p1, p2)
    }

    /// Insert a new revision with given parents at the end.
    pub fn insert(&self, parents: Vec<u32>) {
        let mut inserted = self.inserted.borrow_mut();
        let p1 = parents.get(0).map(|r| *r as i32).unwrap_or(-1);
        let p2 = parents.get(1).map(|r| *r as i32).unwrap_or(-1);
        inserted.push(ParentRevs::from_p1p2(p1, p2));
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
