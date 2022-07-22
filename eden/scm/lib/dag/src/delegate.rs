/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/// Macro rules to delegate trait implementations

#[macro_export]
macro_rules! delegate {
    {IdConvert { impl $($impl:tt)* } => self.$($t:tt)*} => {
        impl $($impl)* {
            fn vertex_id<'a: 's, 's>(&'a self, name: $crate::Vertex)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<$crate::Id>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.vertex_id(name)
            }
            fn vertex_id_with_max_group<'a: 's, 'b: 's, 's>(&'a self, name: &'b $crate::Vertex, max_group: $crate::Group)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<Option<$crate::Id>>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.vertex_id_with_max_group(name, max_group)
            }
            fn vertex_name<'a: 's, 's>(&'a self, id: $crate::Id)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<$crate::Vertex>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.vertex_name(id)
            }
            fn contains_vertex_name<'a: 's, 'b: 's, 's>(&'a self, name: &'b $crate::Vertex)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<bool>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.contains_vertex_name(name)
            }
            fn vertex_id_optional<'a: 's, 'b: 's, 's>(&'a self, name: &'b $crate::Vertex)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<Option<$crate::Id>>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.vertex_id_with_max_group(name, $crate::Group::NON_MASTER)
            }
            fn contains_vertex_id_locally<'a: 's, 'b: 's, 's>(&'a self, ids: &'b [$crate::Id])
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<Vec<bool>>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.contains_vertex_id_locally(ids)
            }
            fn contains_vertex_name_locally<'a: 's, 'b: 's, 's>(&'a self, names: &'b [$crate::VertexName])
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<Vec<bool>>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.contains_vertex_name_locally(names)
            }
            fn vertex_name_batch<'a: 's, 'b: 's, 's>(&'a self, ids: &'b [$crate::Id])
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<Vec<$crate::Result<$crate::VertexName>>>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.vertex_name_batch(ids)
            }
            fn vertex_id_batch<'a: 's, 'b: 's, 's>(&'a self, names: &'b [$crate::VertexName])
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<Vec<$crate::Result<$crate::Id>>>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.vertex_id_batch(names)
            }
            fn map_id(&self) -> &str {
                self.$($t)*.map_id()
            }
            fn map_version(&self) -> &$crate::VerLink {
                self.$($t)*.map_version()
            }
        }
    };

    (IdConvert, $type:ty => self.$($t:tt)*) => {
        delegate! { IdConvert { impl $crate::ops::IdConvert for $type } => self.$($t)* }
    };

    {PrefixLookup { impl $($impl:tt)* } => self.$($t:tt)*} => {
        impl $($impl)* {
            fn vertexes_by_hex_prefix<'a: 'c, 'b: 'c, 'c>(&'a self, hex_prefix: &'b [u8], limit: usize) -> std::pin::Pin<Box<dyn std::future::Future<Output=$crate::Result<Vec<$crate::Vertex>>> + Send + 'c>> where Self: 'c {
                self.$($t)*.vertexes_by_hex_prefix(hex_prefix, limit)
            }
        }
    };

    (PrefixLookup, $type:ty => self.$($t:tt)*) => {
        delegate! { PrefixLookup { impl $crate::ops::PrefixLookup for $type } => self.$($t)* }
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
            fn sort<'a: 'c, 'b: 'c, 'c>(&'a self, set: &'b $crate::Set)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<$crate::Set>
                    > + Send + 'c>> where Self: 'c
            {
                self.$($t)*.sort(set)
            }
            fn parent_names<'a: 'c, 'c>(&'a self, name: $crate::Vertex)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<Vec<$crate::Vertex>>
                    > + Send + 'c>> where Self: 'c
            {
                self.$($t)*.parent_names(name)
            }
            fn all<'a: 's, 's>(&'a self)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<$crate::Set>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.all()
            }
            fn master_group<'a: 's, 's>(&'a self)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<$crate::Set>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.master_group()
            }
            fn ancestors<'a: 's, 's>(&'a self, set: $crate::Set)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<$crate::Set>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.ancestors(set)
            }
            fn first_ancestors<'a: 's, 's>(&'a self, set: $crate::Set)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<$crate::Set>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.first_ancestors(set)
            }
            fn parents<'a: 's, 's>(&'a self, set: $crate::Set)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<$crate::Set>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.parents(set)
            }
            fn merges<'a: 's, 's>(&'a self, set: $crate::Set)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<$crate::Set>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.merges(set)
            }
            fn first_ancestor_nth<'a: 's, 's>(&'a self, name: $crate::Vertex, n: u64)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<Option<$crate::Vertex>>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.first_ancestor_nth(name, n)
            }
            fn heads<'a: 's, 's>(&'a self, set: $crate::Set)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<$crate::Set>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.heads(set)
            }
            fn children<'a: 's, 's>(&'a self, set: $crate::Set)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<$crate::Set>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.children(set)
            }
            fn roots<'a: 's, 's>(&'a self, set: $crate::Set)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<$crate::Set>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.roots(set)
            }
            fn gca_one<'a: 's, 's>(&'a self, set: $crate::Set)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<Option<$crate::Vertex>>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.gca_one(set)
            }
            fn gca_all<'a: 's, 's>(&'a self, set: $crate::Set)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<$crate::Set>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.gca_all(set)
            }
            fn common_ancestors<'a: 's, 's>(&'a self, set: $crate::Set)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<$crate::Set>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.common_ancestors(set)
            }
            fn is_ancestor<'a: 's, 's>(&'a self, ancestor: $crate::Vertex, descendant: $crate::Vertex)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<bool>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.is_ancestor(ancestor, descendant)
            }
            fn heads_ancestors<'a: 's, 's>(&'a self, set: $crate::Set)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<$crate::Set>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.heads_ancestors(set)
            }
            fn range<'a: 's, 's>(&'a self, roots: $crate::Set, heads: $crate::Set)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<$crate::Set>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.range(roots, heads)
            }
            fn only<'a: 's, 's>(&'a self, reachable: $crate::Set, unreachable: $crate::Set)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<$crate::Set>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.only(reachable, unreachable)
            }
            fn only_both<'a: 's, 's>(&'a self, reachable: $crate::Set, unreachable: $crate::Set)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<($crate::Set, $crate::Set)>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.only_both(reachable, unreachable)
            }
            fn descendants<'a: 's, 's>(&'a self, set: $crate::Set)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<$crate::Set>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.descendants(set)
            }
            fn reachable_roots<'a: 's, 's>(&'a self, roots: $crate::Set, heads: $crate::Set)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<$crate::Set>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.reachable_roots(roots, heads)
            }
            fn dirty<'a: 's, 's>(&'a self)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<$crate::Set>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.dirty()
            }
            fn is_vertex_lazy(&self) -> bool {
                self.$($t)*.is_vertex_lazy()
            }
            fn dag_snapshot(&self)
                -> $crate::Result<std::sync::Arc<dyn $crate::DagAlgorithm + Send + Sync>>
            {
                self.$($t)*.dag_snapshot()
            }
            fn id_dag_snapshot(&self)
                -> $crate::Result<std::sync::Arc<dyn $crate::IdDagAlgorithm + Send + Sync>>
            {
                self.$($t)*.id_dag_snapshot()
            }
            fn dag_id(&self) -> &str {
                self.$($t)*.dag_id()
            }
            fn dag_version(&self) -> &$crate::VerLink {
                self.$($t)*.dag_version()
            }
        }
    };

    (IdMapSnapshot, $type:ty => self.$($t:tt)*) => {
        impl $crate::ops::IdMapSnapshot for $type {
            fn id_map_snapshot(&self) -> $crate::Result<std::sync::Arc<dyn $crate::ops::IdConvert + Send + Sync>> {
                self.$($t)*.id_map_snapshot()
            }
        }
    };

    (CheckIntegrity, $type:ty => self.$($t:tt)*) => {
        impl $crate::ops::CheckIntegrity for $type {
            fn check_universal_ids<'a: 's, 's>(&'a self)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<Vec<$crate::Id>>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.check_universal_ids()
            }
            fn check_segments<'a: 's, 's>(&'a self)
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<Vec<String>>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.check_segments()
            }
            fn check_isomorphic_graph<'a: 's, 'b: 's, 's> (
                &'a self,
                other: &'b dyn $crate::ops::DagAlgorithm,
                heads: $crate::NameSet,
            )
                -> std::pin::Pin<Box<dyn std::future::Future<Output=
                        $crate::Result<Vec<String>>
                    > + Send + 's>> where Self: 's
            {
                self.$($t)*.check_isomorphic_graph(other, heads)
            }
        }
    };

    ($name:ident | $name2:ident $(| $name3:ident )*, $type:ty => self.$($t:tt)*) => {
        delegate!($name, $type => self.$($t)*);
        delegate!($name2 $(| $name3 )*, $type => self.$($t)*);
    };
}

mod impls {
    use std::ops::Deref;
    use std::sync::Arc;

    use crate::ops::DagAlgorithm;
    use crate::ops::IdConvert;

    delegate!(IdConvert | PrefixLookup, Arc<dyn IdConvert + Send + Sync> => self.deref());
    delegate!(DagAlgorithm, Arc<dyn DagAlgorithm + Send + Sync> => self.deref());
    delegate!(DagAlgorithm, &(dyn DagAlgorithm + Send + Sync) => self.deref());
}
