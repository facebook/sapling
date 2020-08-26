/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/// Macro rules to delegate trait implementations

#[macro_export]
macro_rules! delegate {
    (IdConvert, $type:ty => self.$($t:tt)*) => {
        impl $crate::ops::IdConvert for $type {
            fn vertex_id(&self, name: $crate::Vertex) -> $crate::Result<$crate::Id> {
                self.$($t)*.vertex_id(name)
            }
            fn vertex_id_with_max_group(&self, name: &$crate::Vertex, max_group: $crate::Group) -> $crate::Result<Option<$crate::Id>> {
                self.$($t)*.vertex_id_with_max_group(name, max_group)
            }
            fn vertex_name(&self, id: $crate::Id) -> $crate::Result<$crate::Vertex> {
                self.$($t)*.vertex_name(id)
            }
            fn contains_vertex_name(&self, name: &$crate::Vertex) -> $crate::Result<bool> {
                self.$($t)*.contains_vertex_name(name)
            }
            fn vertex_id_optional(&self, name: &$crate::Vertex) -> $crate::Result<Option<$crate::Id>> {
                self.$($t)*.vertex_id_with_max_group(name, $crate::Group::NON_MASTER)
            }
        }
    };

    (PrefixLookup, $type:ty => self.$($t:tt)*) => {
        impl $crate::ops::PrefixLookup for $type {
            fn vertexes_by_hex_prefix(&self, hex_prefix: &[u8], limit: usize) -> $crate::Result<Vec<$crate::Vertex>> {
                self.$($t)*.vertexes_by_hex_prefix(hex_prefix, limit)
            }
        }
    };

    (ToIdSet, $type:ty => self.$($t:tt)*) => {
        impl $crate::ops::ToIdSet for $type {
            fn to_id_set(&self, set: &$crate::Set) -> $crate::Result<$crate::IdSet> {
                self.$($t)*.to_id_set(set)
            }
        }
    };

    (ToSet, $type:ty => self.$($t:tt)*) => {
        impl $crate::ops::ToSet for $type {
            fn to_set(&self, set: &$crate::IdSet) -> $crate::Result<$crate::Set> {
                self.$($t)*.to_set(set)
            }
        }
    };

    (DagAlgorithm, $type:ty => self.$($t:tt)*) => {
        impl $crate::DagAlgorithm for $type {
            fn sort(&self, set: &$crate::Set) -> $crate::Result<$crate::Set> {
                self.$($t)*.sort(set)
            }
            fn parent_names(&self, name: $crate::Vertex) -> $crate::Result<Vec<$crate::Vertex>> {
                self.$($t)*.parent_names(name)
            }
            fn all(&self) -> $crate::Result<$crate::Set> {
                self.$($t)*.all()
            }
            fn ancestors(&self, set: $crate::Set) -> $crate::Result<$crate::Set> {
                self.$($t)*.ancestors(set)
            }
            fn parents(&self, set: $crate::Set) -> $crate::Result<$crate::Set> {
                self.$($t)*.parents(set)
            }
            fn first_ancestor_nth(&self, name: $crate::Vertex, n: u64) -> $crate::Result<$crate::Vertex> {
                self.$($t)*.first_ancestor_nth(name, n)
            }
            fn heads(&self, set: $crate::Set) -> $crate::Result<$crate::Set> {
                self.$($t)*.heads(set)
            }
            fn children(&self, set: $crate::Set) -> $crate::Result<$crate::Set> {
                self.$($t)*.children(set)
            }
            fn roots(&self, set: $crate::Set) -> $crate::Result<$crate::Set> {
                self.$($t)*.roots(set)
            }
            fn gca_one(&self, set: $crate::Set) -> $crate::Result<Option<$crate::Vertex>> {
                self.$($t)*.gca_one(set)
            }
            fn gca_all(&self, set: $crate::Set) -> $crate::Result<$crate::Set> {
                self.$($t)*.gca_all(set)
            }
            fn common_ancestors(&self, set: $crate::Set) -> $crate::Result<$crate::Set> {
                self.$($t)*.common_ancestors(set)
            }
            fn is_ancestor(&self, ancestor: $crate::Vertex, descendant: $crate::Vertex) -> $crate::Result<bool> {
                self.$($t)*.is_ancestor(ancestor, descendant)
            }
            fn heads_ancestors(&self, set: $crate::Set) -> $crate::Result<$crate::Set> {
                self.$($t)*.heads_ancestors(set)
            }
            fn range(&self, roots: $crate::Set, heads: $crate::Set) -> $crate::Result<$crate::Set> {
                self.$($t)*.range(roots, heads)
            }
            fn only(&self, reachable: $crate::Set, unreachable: $crate::Set) -> $crate::Result<$crate::Set> {
                self.$($t)*.only(reachable, unreachable)
            }
            fn only_both(&self, reachable: $crate::Set, unreachable: $crate::Set) -> $crate::Result<($crate::Set, $crate::Set)> {
                self.$($t)*.only_both(reachable, unreachable)
            }
            fn descendants(&self, set: $crate::Set) -> $crate::Result<$crate::Set> {
                self.$($t)*.descendants(set)
            }
            fn reachable_roots(&self, roots: $crate::Set, heads: $crate::Set) -> $crate::Result<$crate::Set> {
                self.$($t)*.reachable_roots(roots, heads)
            }
            fn dag_snapshot(&self) -> $crate::Result<std::sync::Arc<dyn $crate::DagAlgorithm + Send + Sync>> {
                self.$($t)*.dag_snapshot()
            }
        }
    };

    ($name:ident | $name2:ident $(| $name3:ident )*, $type:ty => self.$($t:tt)*) => {
        delegate!($name, $type => self.$($t)*);
        delegate!($name2 $(| $name3 )*, $type => self.$($t)*);
    };
}
