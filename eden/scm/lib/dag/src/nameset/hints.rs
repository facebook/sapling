/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::cmp;
use std::fmt;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::Acquire;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::Ordering::Release;
use std::sync::Arc;

use bitflags::bitflags;

use crate::ops::DagAlgorithm;
use crate::ops::IdConvert;
use crate::Id;
use crate::VerLink;

bitflags! {
    pub struct Flags: u32 {
        /// Full. A full Set & other set X with compatible Dag results in X.
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
    id_map: IdMapSnapshot,
    dag: DagSnapshot,
}

impl Hints {
    pub fn new_with_idmap_dag(
        id_map: impl Into<IdMapSnapshot>,
        dag: impl Into<DagSnapshot>,
    ) -> Self {
        Self {
            id_map: id_map.into(),
            dag: dag.into(),
            ..Default::default()
        }
    }

    pub fn new_inherit_idmap_dag(hints: &Self) -> Self {
        Self::new_with_idmap_dag(hints, hints)
    }

    /// Attempt to inherit properties (IdMap and Dag snapshots) from a list of
    /// hints. The returned hints have IdMap and Dag set to be compatible with
    /// all other hints in the list (or set to be None if that's not possible).
    pub fn union(hints_list: &[&Self]) -> Self {
        let default = Self::default();
        // Find the id_map that is compatible with all other id_maps.
        debug_assert!(default.id_map().is_none());
        let id_map = hints_list
            .iter()
            .fold(Some(&default), |opt_a, b| {
                opt_a.and_then(
                    |a| match a.id_map_version().partial_cmp(&b.id_map_version()) {
                        None => None, // Incompatible sets
                        Some(cmp::Ordering::Equal) | Some(cmp::Ordering::Greater) => Some(a),
                        Some(cmp::Ordering::Less) => Some(b),
                    },
                )
            })
            .and_then(|a| a.id_map());
        // Find the dag that is compatible with all other dags.
        debug_assert!(default.dag().is_none());
        let dag = hints_list
            .iter()
            .fold(Some(&default), |opt_a, b| {
                opt_a.and_then(|a| match a.dag_version().partial_cmp(&b.dag_version()) {
                    None => None,
                    Some(cmp::Ordering::Equal) | Some(cmp::Ordering::Greater) => Some(a),
                    Some(cmp::Ordering::Less) => Some(b),
                })
            })
            .and_then(|a| a.dag());
        Self {
            id_map: IdMapSnapshot(id_map),
            dag: DagSnapshot(dag),
            ..Self::default()
        }
    }

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

    pub fn inherit_flags_min_max_id(&self, other: &Hints) -> &Self {
        self.update_flags_with(|_| other.flags());
        if let Some(id) = other.min_id() {
            self.set_min_id(id);
        }
        if let Some(id) = other.max_id() {
            self.set_max_id(id);
        }
        self
    }

    pub fn dag(&self) -> Option<Arc<dyn DagAlgorithm + Send + Sync>> {
        self.dag.0.clone()
    }

    pub fn id_map(&self) -> Option<Arc<dyn IdConvert + Send + Sync>> {
        self.id_map.0.clone()
    }

    /// The `VerLink` of the Dag. `None` if there is no Dag associated.
    pub fn dag_version(&self) -> Option<&VerLink> {
        self.dag.0.as_ref().map(|d| d.dag_version())
    }

    /// The `VerLink` of the IdMap. `None` if there is no IdMap associated.
    pub fn id_map_version(&self) -> Option<&VerLink> {
        self.id_map.0.as_ref().map(|d| d.map_version())
    }
}

#[derive(Clone, Default)]
pub struct DagSnapshot(Option<Arc<dyn DagAlgorithm + Send + Sync>>);

impl From<&Hints> for DagSnapshot {
    fn from(hints: &Hints) -> Self {
        hints.dag.clone()
    }
}

impl From<Arc<dyn DagAlgorithm + Send + Sync>> for DagSnapshot {
    fn from(dag: Arc<dyn DagAlgorithm + Send + Sync>) -> Self {
        DagSnapshot(Some(dag))
    }
}

#[derive(Clone, Default)]
pub struct IdMapSnapshot(Option<Arc<dyn IdConvert + Send + Sync>>);

impl From<&Hints> for IdMapSnapshot {
    fn from(hints: &Hints) -> Self {
        hints.id_map.clone()
    }
}

impl From<Arc<dyn IdConvert + Send + Sync>> for IdMapSnapshot {
    fn from(dag: Arc<dyn IdConvert + Send + Sync>) -> Self {
        IdMapSnapshot(Some(dag))
    }
}

#[derive(Clone, Default)]
pub struct DagVersion<'a>(Option<&'a VerLink>);

impl<'a> From<&'a Hints> for DagVersion<'a> {
    fn from(hints: &'a Hints) -> Self {
        Self(match &hints.dag.0 {
            Some(d) => Some(d.dag_version()),
            None => None,
        })
    }
}

impl<'a> From<&'a VerLink> for DagVersion<'a> {
    fn from(version: &'a VerLink) -> Self {
        Self(Some(version))
    }
}

#[derive(Clone, Default)]
pub struct IdMapVersion<'a>(Option<&'a VerLink>);

impl<'a> From<&'a Hints> for IdMapVersion<'a> {
    fn from(hints: &'a Hints) -> Self {
        Self(match &hints.id_map.0 {
            Some(m) => Some(m.map_version()),
            None => None,
        })
    }
}

impl<'a> From<&'a VerLink> for IdMapVersion<'a> {
    fn from(version: &'a VerLink) -> Self {
        Self(Some(version))
    }
}

impl Clone for Hints {
    fn clone(&self) -> Self {
        Self {
            flags: AtomicU32::new(self.flags.load(Acquire)),
            min_id: AtomicU64::new(self.min_id.load(Acquire)),
            max_id: AtomicU64::new(self.max_id.load(Acquire)),
            id_map: self.id_map.clone(),
            dag: self.dag.clone(),
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
            (None, None) => {}
        }
        write!(f, ")")?;
        Ok(())
    }
}

#[cfg(test)]
#[test]
fn test_incompatilbe_union() {
    use crate::tests::dummy_dag::DummyDag;
    let dag1 = DummyDag::new();
    let dag2 = DummyDag::new();

    let mut hints1 = Hints::default();
    hints1.dag = DagSnapshot(Some(dag1.dag_snapshot().unwrap()));

    let mut hints2 = Hints::default();
    hints2.dag = DagSnapshot(Some(dag2.dag_snapshot().unwrap()));

    assert_eq!(
        Hints::union(&[&hints1, &hints1]).dag_version(),
        hints1.dag_version()
    );

    assert_eq!(Hints::union(&[&hints1, &hints2]).dag_version(), None);
    assert_eq!(Hints::union(&[&hints2, &hints1]).dag_version(), None);
    assert_eq!(
        Hints::union(&[&hints2, &hints1, &hints2]).dag_version(),
        None
    );
}
