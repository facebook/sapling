/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::ops::DagAlgorithm;
use crate::ops::IdConvert;
use crate::Id;
use bitflags::bitflags;
use std::fmt;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize};
use std::sync::Arc;

bitflags! {
    pub struct Flags: u32 {
        /// Full. A full Set & other set X with compatible Dag results in X.
        /// Use `set_dag` to set the Dag pointer to avoid fast paths
        /// intersecting incompatible Dags.
        const FULL = 0x1;

        /// Empty (also implies ID_DESC | ID_ASC | TOPO_DESC).
        const EMPTY = 0x2;

        /// Sorted by Id. Descending (head -> root).
        const ID_DESC = 0x4;

        /// Sorted by Id. Ascending (root -> head).
        const ID_ASC = 0x8;

        /// Sorted topologically. Descending (head -> root).
        const TOPO_DESC = 0x10;

        /// Minimal Id is known.
        const HAS_MIN_ID = 0x20;

        /// Maximum Id is known.
        const HAS_MAX_ID = 0x40;

        /// A "filter" set. It provides "contains" but iteration is inefficient.
        const FILTER = 0x80;

        /// The set contains ancestors. If X in set, any ancestor of X is also in set.
        const ANCESTORS = 0x100;
    }
}

/// Optimation hints.
#[derive(Default)]
pub struct Hints {
    // Atomic is used for interior mutability.
    flags: AtomicU32,
    min_id: AtomicU64,
    max_id: AtomicU64,
    id_map_ptr: AtomicUsize,
    dag_ptr: AtomicUsize,
}

impl Hints {
    pub fn flags(&self) -> Flags {
        let flags = self.flags.load(Relaxed);
        Flags::from_bits_truncate(flags)
    }

    pub fn contains(&self, flags: Flags) -> bool {
        self.flags().contains(flags)
    }

    pub fn min_id(&self) -> Option<Id> {
        if self.contains(Flags::HAS_MIN_ID) {
            Some(Id(self.min_id.load(Acquire)))
        } else {
            None
        }
    }

    pub fn max_id(&self) -> Option<Id> {
        if self.contains(Flags::HAS_MAX_ID) {
            Some(Id(self.max_id.load(Acquire)))
        } else {
            None
        }
    }

    pub fn update_flags_with(&self, func: impl Fn(Flags) -> Flags) -> &Self {
        let mut flags = func(self.flags());
        // Automatically add "derived" flags.
        if flags.contains(Flags::EMPTY) {
            flags.insert(Flags::ID_ASC | Flags::ID_DESC | Flags::TOPO_DESC | Flags::ANCESTORS);
        }
        if flags.contains(Flags::FULL) {
            flags.insert(Flags::ANCESTORS);
        }
        self.flags.store(flags.bits(), Relaxed);
        self
    }

    pub fn add_flags(&self, flags: Flags) -> &Self {
        self.update_flags_with(|f| f | flags);
        self
    }

    pub fn remove_flags(&self, flags: Flags) -> &Self {
        self.update_flags_with(|f| f - flags);
        self
    }

    pub fn set_min_id(&self, min_id: Id) -> &Self {
        self.min_id.store(min_id.0, Release);
        self.add_flags(Flags::HAS_MIN_ID);
        self
    }

    pub fn set_max_id(&self, max_id: Id) -> &Self {
        self.max_id.store(max_id.0, Release);
        self.add_flags(Flags::HAS_MAX_ID);
        self
    }

    pub fn set_id_map(&self, ptr: &Arc<dyn IdConvert + Send + Sync>) -> &Self {
        let map_ref: &dyn IdConvert = ptr.as_ref();
        // std::raw::TraitObject is not stable yet.
        let vptr: [usize; 2] = unsafe { std::mem::transmute(map_ref) };
        self.id_map_ptr.store(vptr[0], Release);
        self
    }

    pub fn set_dag(&self, ptr: &dyn DagAlgorithm) -> &Self {
        // std::raw::TraitObject is not stable yet.
        let ptr: DagPtr = ptr.into();
        self.dag_ptr.store(ptr.0, Release);
        self
    }

    pub fn inherit_id_map(&self, other: &Hints) -> &Self {
        let ptr = other.id_map_ptr.load(Acquire);
        self.id_map_ptr.store(ptr, Release);
        self
    }

    pub fn inherit_dag(&self, other: &Hints) -> &Self {
        let ptr = other.dag_ptr.load(Acquire);
        self.dag_ptr.store(ptr, Release);
        self
    }

    pub fn is_id_map_compatible(&self, other: &Hints) -> bool {
        let ptr1 = self.id_map_ptr.load(Acquire);
        let ptr2 = other.id_map_ptr.load(Acquire);
        ptr1 == ptr2
    }

    pub fn is_dag_compatible(&self, other: impl Into<DagPtr>) -> bool {
        let ptr1 = self.dag_ptr.load(Acquire);
        let ptr2 = other.into().0;
        ptr1 == ptr2
    }
}

pub struct DagPtr(usize);

impl From<&Hints> for DagPtr {
    fn from(hints: &Hints) -> Self {
        DagPtr(hints.dag_ptr.load(Acquire))
    }
}

impl From<&dyn DagAlgorithm> for DagPtr {
    fn from(ptr: &dyn DagAlgorithm) -> Self {
        // std::raw::TraitObject is not stable yet.
        // XXX: This is in theory unsound because pointer address can be reused.
        let vptr: [usize; 2] = unsafe { std::mem::transmute(ptr) };
        DagPtr(vptr[0])
    }
}

impl Clone for Hints {
    fn clone(&self) -> Self {
        Self {
            flags: AtomicU32::new(self.flags.load(Acquire)),
            min_id: AtomicU64::new(self.min_id.load(Acquire)),
            max_id: AtomicU64::new(self.max_id.load(Acquire)),
            id_map_ptr: AtomicUsize::new(self.id_map_ptr.load(Acquire)),
            dag_ptr: AtomicUsize::new(self.dag_ptr.load(Acquire)),
        }
    }
}

impl fmt::Debug for Hints {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Hints({:?}",
            self.flags() - (Flags::HAS_MIN_ID | Flags::HAS_MAX_ID)
        )?;
        match (self.min_id(), self.max_id()) {
            (Some(min), Some(max)) => write!(f, ", {}..={}", min.0, max.0)?,
            (Some(min), None) => write!(f, ", {}..", min.0)?,
            (None, Some(max)) => write!(f, ", ..={}", max.0)?,
            (None, None) => (),
        }
        write!(f, ")")?;
        Ok(())
    }
}
