/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use dag::Id;
use dag::IdSet;
use std::cell::RefCell;

impl<T: AsRef<[RevlogEntry]>> RevlogIndex<T> {
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
pub struct RevlogIndex<T> {
    /// Content of revlog-name.i (ex. 00changelog.i).
    pub data: T,

    /// Inserted entries that are not flushed to disk.
    pub inserted: RefCell<Vec<ParentRevs>>,
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

impl<T> RevlogIndex<T>
where
    T: AsRef<[RevlogEntry]>,
{
    /// Revisions in total.
    pub fn len(&self) -> usize {
        let inserted = self.inserted.borrow();
        self.data_len() + inserted.len()
    }

    /// Revisions stored in the original revlog index.
    pub fn data_len(&self) -> usize {
        self.data.as_ref().len()
    }

    /// Get parent revisions.
    pub fn parents(&self, rev: u32) -> ParentRevs {
        let data_len = self.data_len();
        if rev >= data_len as u32 {
            let inserted = self.inserted.borrow();
            return inserted[rev as usize - data_len].clone();
        }

        let data = self.data.as_ref();
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
