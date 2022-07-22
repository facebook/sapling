/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;

use serde::Deserialize;

use crate::Group;
use crate::VertexName;

/// A list of [`VertexName`]s (usually heads) with options attached to each vertex.
#[derive(Default, Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct VertexListWithOptions {
    list: Vec<(VertexName, VertexOptions)>,
}

/// Options attached to a vertex. Usually the vertex is a head. The head and its
/// ancestors are going to be inserted to the graph. The options controls some
/// details about the insertion.
#[derive(Debug, Clone, Deserialize)]
#[non_exhaustive]
pub struct VertexOptions {
    /// How many ids to reserve for this vertex. Suppose this vertex has id `n`,
    /// then `n+1..=n+reserve_size` can only be used when inserting this vertex
    /// and its ancestors in the same batch.
    ///
    /// Note: if any id `j` in the `n+1..=n+reserve_size` range were already
    /// taken, then the reserve range becomes `n+1..j` instead. This avoids
    /// fragmentation.
    #[serde(default = "Default::default")]
    pub reserve_size: u32,

    /// The highest [`Group`] for this vertex. If set to `NON_MASTER` then
    /// this vertex could end up in `MASTER` or `NON_MASTER`. If set to
    /// `MASTER` then this vertex will end up in `MASTER` group.
    #[serde(default = "default_highest_group")]
    pub highest_group: Group,
}

const fn default_highest_group() -> Group {
    Group::NON_MASTER
}

impl Default for VertexOptions {
    fn default() -> Self {
        Self {
            reserve_size: 0,
            highest_group: default_highest_group(),
        }
    }
}

impl<'a> From<&'a [VertexName]> for VertexListWithOptions {
    fn from(list: &'a [VertexName]) -> Self {
        // Just use default options.
        Self {
            list: list
                .iter()
                .map(|v| (v.clone(), VertexOptions::default()))
                .collect(),
        }
    }
}

impl From<Vec<VertexName>> for VertexListWithOptions {
    fn from(list: Vec<VertexName>) -> Self {
        // Just use default options.
        Self {
            list: list
                .into_iter()
                .map(|v| (v, VertexOptions::default()))
                .collect(),
        }
    }
}

impl From<Vec<(VertexName, VertexOptions)>> for VertexListWithOptions {
    fn from(list: Vec<(VertexName, VertexOptions)>) -> Self {
        Self { list }
    }
}

impl VertexListWithOptions {
    /// Get the vertexes and their options.
    pub fn vertex_options(&self) -> Vec<(VertexName, VertexOptions)> {
        self.list.clone()
    }

    /// Get the vertexes.
    pub fn vertexes(&self) -> Vec<VertexName> {
        self.list.iter().map(|i| i.0.clone()).collect()
    }

    /// Get the vertexes, filter by the `highest_group` option.
    pub fn vertexes_by_group(&self, group: Group) -> Vec<VertexName> {
        self.list
            .iter()
            .filter_map(|(v, o)| {
                if o.highest_group == group {
                    Some(v.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Test if this list is empty.
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Add a new item to the list.
    pub fn push(&mut self, head_opts: (VertexName, VertexOptions)) {
        self.list.push(head_opts);
    }

    /// Set the `highest_group` option for all vertexes.
    pub fn with_highest_group(mut self, group: Group) -> Self {
        for (_v, opts) in self.list.iter_mut() {
            opts.highest_group = group;
        }
        self
    }

    /// Chain another list. Vertexes that are already in this list are skipped.
    pub fn chain(mut self, other: impl Into<Self>) -> Self {
        let other = other.into();
        let existed: HashSet<_> = self.vertexes().into_iter().collect();
        for (v, o) in other.list {
            if !existed.contains(&v) {
                self.list.push((v, o))
            }
        }
        self
    }
}
