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

    ($name:ident | $name2:ident $(| $name3:ident )*, $type:ty => self.$($t:tt)*) => {
        delegate!($name, $type => self.$($t)*);
        delegate!($name2 $(| $name3 )*, $type => self.$($t)*);
    };
}
