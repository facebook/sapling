/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! DAG and Id operations (mostly traits)

use crate::default_impl;
use crate::id::Group;
use crate::id::Id;
use crate::id::VertexName;
use crate::namedag::MemNameDag;
use crate::nameset::id_lazy::IdLazySet;
use crate::nameset::id_static::IdStaticSet;
use crate::nameset::NameSet;
use crate::IdSet;
use crate::Result;
use std::sync::Arc;

/// DAG related read-only algorithms.
pub trait DagAlgorithm {
    /// Sort a `NameSet` topologically.
    fn sort(&self, set: &NameSet) -> Result<NameSet>;

    /// Re-create the graph so it looks better when rendered.
    fn beautify(&self, main_branch: Option<NameSet>) -> Result<MemNameDag> {
        default_impl::beautify(self, main_branch)
    }

    /// Get ordered parent vertexes.
    fn parent_names(&self, name: VertexName) -> Result<Vec<VertexName>>;

    /// Returns a [`SpanSet`] that covers all vertexes tracked by this DAG.
    fn all(&self) -> Result<NameSet>;

    /// Calculates all ancestors reachable from any name from the given set.
    fn ancestors(&self, set: NameSet) -> Result<NameSet>;

    /// Calculates parents of the given set.
    ///
    /// Note: Parent order is not preserved. Use [`NameDag::parent_names`]
    /// to preserve order.
    fn parents(&self, set: NameSet) -> Result<NameSet> {
        default_impl::parents(self, set)
    }

    /// Calculates the n-th first ancestor.
    fn first_ancestor_nth(&self, name: VertexName, n: u64) -> Result<VertexName> {
        default_impl::first_ancestor_nth(self, name, n)
    }

    /// Calculates heads of the given set.
    fn heads(&self, set: NameSet) -> Result<NameSet> {
        default_impl::heads(self, set)
    }

    /// Calculates children of the given set.
    fn children(&self, set: NameSet) -> Result<NameSet>;

    /// Calculates roots of the given set.
    fn roots(&self, set: NameSet) -> Result<NameSet> {
        default_impl::roots(self, set)
    }

    /// Calculates one "greatest common ancestor" of the given set.
    ///
    /// If there are no common ancestors, return None.
    /// If there are multiple greatest common ancestors, pick one arbitrarily.
    /// Use `gca_all` to get all of them.
    fn gca_one(&self, set: NameSet) -> Result<Option<VertexName>> {
        default_impl::gca_one(self, set)
    }

    /// Calculates all "greatest common ancestor"s of the given set.
    /// `gca_one` is faster if an arbitrary answer is ok.
    fn gca_all(&self, set: NameSet) -> Result<NameSet> {
        default_impl::gca_all(self, set)
    }

    /// Calculates all common ancestors of the given set.
    fn common_ancestors(&self, set: NameSet) -> Result<NameSet> {
        default_impl::common_ancestors(self, set)
    }

    /// Tests if `ancestor` is an ancestor of `descendant`.
    fn is_ancestor(&self, ancestor: VertexName, descendant: VertexName) -> Result<bool> {
        default_impl::is_ancestor(self, ancestor, descendant)
    }

    /// Calculates "heads" of the ancestors of the given set. That is,
    /// Find Y, which is the smallest subset of set X, where `ancestors(Y)` is
    /// `ancestors(X)`.
    ///
    /// This is faster than calculating `heads(ancestors(set))` in certain
    /// implementations like segmented changelog.
    ///
    /// This is different from `heads`. In case set contains X and Y, and Y is
    /// an ancestor of X, but not the immediate ancestor, `heads` will include
    /// Y while this function won't.
    fn heads_ancestors(&self, set: NameSet) -> Result<NameSet> {
        default_impl::heads_ancestors(self, set)
    }

    /// Calculates the "dag range" - vertexes reachable from both sides.
    fn range(&self, roots: NameSet, heads: NameSet) -> Result<NameSet>;

    /// Calculates `ancestors(reachable) - ancestors(unreachable)`.
    fn only(&self, reachable: NameSet, unreachable: NameSet) -> Result<NameSet> {
        default_impl::only(self, reachable, unreachable)
    }

    /// Calculates `ancestors(reachable) - ancestors(unreachable)`, and
    /// `ancestors(unreachable)`.
    /// This might be faster in some implementations than calculating `only` and
    /// `ancestors` separately.
    fn only_both(&self, reachable: NameSet, unreachable: NameSet) -> Result<(NameSet, NameSet)> {
        default_impl::only_both(self, reachable, unreachable)
    }

    /// Calculates the descendants of the given set.
    fn descendants(&self, set: NameSet) -> Result<NameSet>;

    /// Calculates `roots` that are reachable from `heads` without going
    /// through other `roots`. For example, given the following graph:
    ///
    /// ```plain,ignore
    ///   F
    ///   |\
    ///   C E
    ///   | |
    ///   B D
    ///   |/
    ///   A
    /// ```
    ///
    /// `reachable_roots(roots=[A, B, C], heads=[F])` returns `[A, C]`.
    /// `B` is not included because it cannot be reached without going
    /// through another root `C` from `F`. `A` is included because it
    /// can be reached via `F -> E -> D -> A` that does not go through
    /// other roots.
    ///
    /// The can be calculated as
    /// `roots & (heads | parents(only(heads, roots & ancestors(heads))))`.
    /// Actual implementation might have faster paths.
    ///
    /// The `roots & ancestors(heads)` portion filters out bogus roots for
    /// compatibility, if the callsite does not provide bogus roots, it
    /// could be simplified to just `roots`.
    fn reachable_roots(&self, roots: NameSet, heads: NameSet) -> Result<NameSet> {
        default_impl::reachable_roots(self, roots, heads)
    }

    /// Get a snapshot of the current graph that can operate separately.
    ///
    /// This makes it easier to fight with borrowck.
    fn dag_snapshot(&self) -> Result<Arc<dyn DagAlgorithm + Send + Sync>>;
}

/// Add vertexes recursively to the DAG.
pub trait DagAddHeads {
    /// Add vertexes and their ancestors to the DAG. This does not persistent
    /// changes to disk.
    fn add_heads<F>(&mut self, parents: F, heads: &[VertexName]) -> Result<()>
    where
        F: Fn(VertexName) -> Result<Vec<VertexName>>;
}

/// Persistent the DAG on disk.
pub trait DagPersistent {
    /// Write in-memory DAG to disk. This might also pick up changes to
    /// the DAG by other processes.
    fn flush(&mut self, master_heads: &[VertexName]) -> Result<()>;

    /// A faster path for add_heads, followed by flush.
    fn add_heads_and_flush<F>(
        &mut self,
        parent_names_func: F,
        master_names: &[VertexName],
        non_master_names: &[VertexName],
    ) -> Result<()>
    where
        F: Fn(VertexName) -> Result<Vec<VertexName>>;

    /// Import from another (potentially large) DAG. Write to disk immediately.
    fn import_and_flush(
        &mut self,
        dag: &(impl DagAlgorithm + ?Sized),
        master_heads: NameSet,
    ) -> Result<()> {
        let heads = dag.heads(dag.all()?)?;
        let non_master_heads = heads - master_heads.clone();
        let master_heads: Vec<VertexName> = master_heads.iter()?.collect::<Result<Vec<_>>>()?;
        let non_master_heads: Vec<VertexName> =
            non_master_heads.iter()?.collect::<Result<Vec<_>>>()?;
        let parent_func = |v| dag.parent_names(v);
        self.add_heads_and_flush(parent_func, &master_heads, &non_master_heads)
    }
}

/// Import ASCII graph to DAG.
pub trait ImportAscii {
    /// Import vertexes described in an ASCII graph.
    /// `heads` optionally specifies the order of heads to insert.
    /// Useful for testing. Panic if the input is invalid.
    fn import_ascii_with_heads(
        &mut self,
        text: &str,
        heads: Option<&[impl AsRef<str>]>,
    ) -> Result<()>;

    /// Import vertexes described in an ASCII graph.
    fn import_ascii(&mut self, text: &str) -> Result<()> {
        self.import_ascii_with_heads(text, <Option<&[&str]>>::None)
    }
}

/// Lookup vertexes by prefixes.
pub trait PrefixLookup {
    /// Lookup vertexes by hex prefix.
    fn vertexes_by_hex_prefix(&self, hex_prefix: &[u8], limit: usize) -> Result<Vec<VertexName>>;
}

/// Convert between `Vertex` and `Id`.
pub trait IdConvert {
    fn vertex_id(&self, name: VertexName) -> Result<Id>;
    fn vertex_id_with_max_group(&self, name: &VertexName, max_group: Group) -> Result<Option<Id>>;
    fn vertex_name(&self, id: Id) -> Result<VertexName>;
    fn contains_vertex_name(&self, name: &VertexName) -> Result<bool>;
    fn vertex_id_optional(&self, name: &VertexName) -> Result<Option<Id>> {
        self.vertex_id_with_max_group(name, Group::NON_MASTER)
    }
}

impl<T> ImportAscii for T
where
    T: DagAddHeads,
{
    fn import_ascii_with_heads(
        &mut self,
        text: &str,
        heads: Option<&[impl AsRef<str>]>,
    ) -> Result<()> {
        let parents = drawdag::parse(&text);
        let heads: Vec<_> = match heads {
            Some(heads) => heads
                .iter()
                .map(|s| VertexName::copy_from(s.as_ref().as_bytes()))
                .collect(),
            None => {
                let mut heads: Vec<_> = parents
                    .keys()
                    .map(|s| VertexName::copy_from(s.as_bytes()))
                    .collect();
                heads.sort();
                heads
            }
        };

        let parents_func = move |name: VertexName| -> Result<Vec<VertexName>> {
            Ok(parents[&String::from_utf8(name.as_ref().to_vec()).unwrap()]
                .iter()
                .map(|p| VertexName::copy_from(p.as_bytes()))
                .collect())
        };
        self.add_heads(&parents_func, &heads[..])?;
        Ok(())
    }
}

pub trait ToIdSet {
    /// Converts [`NameSet`] to [`SpanSet`].
    fn to_id_set(&self, set: &NameSet) -> Result<IdSet>;
}

pub trait ToSet {
    /// Converts [`SpanSet`] to [`NameSet`].
    fn to_set(&self, set: &IdSet) -> Result<NameSet>;
}

pub trait IdMapEq {
    /// (Cheaply) test if the map is compatible (same).
    fn is_map_compatible(&self, other: &Arc<dyn IdConvert + Send + Sync>) -> bool;
}

pub trait IdMapSnapshot {
    /// Get a snapshot of IdMap.
    fn id_map_snapshot(&self) -> Result<Arc<dyn IdConvert + Send + Sync>>;
}

impl<T: IdConvert + IdMapEq> ToIdSet for T {
    /// Converts [`NameSet`] to [`IdSet`].
    fn to_id_set(&self, set: &NameSet) -> Result<IdSet> {
        // Fast path: extract IdSet from IdStaticSet.
        if let Some(set) = set.as_any().downcast_ref::<IdStaticSet>() {
            if self.is_map_compatible(&set.map) {
                return Ok(set.spans.clone());
            }
        }

        // Convert IdLazySet to IdStaticSet. Bypass hash lookups.
        if let Some(set) = set.as_any().downcast_ref::<IdLazySet>() {
            if self.is_map_compatible(&set.map) {
                let set: IdStaticSet = set.to_static()?;
                return Ok(set.spans);
            }
        }

        // Slow path: iterate through the set and convert it to a non-lazy
        // IdSet. Does not bypass hash lookups.
        let mut spans = IdSet::empty();
        for name in set.iter()? {
            let name = name?;
            let id = self.vertex_id(name)?;
            spans.push(id);
        }
        Ok(spans)
    }
}

impl<T: IdMapSnapshot + DagAlgorithm> ToSet for T {
    /// Converts [`SpanSet`] to [`NameSet`].
    fn to_set(&self, set: &IdSet) -> Result<NameSet> {
        NameSet::from_spans_dag(set.clone(), self)
    }
}
