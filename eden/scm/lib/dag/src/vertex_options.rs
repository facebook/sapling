/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;

use serde::Deserialize;

use crate::Group;
use crate::Vertex;

/// A list of [`Vertex`]s (usually heads) with options attached to each vertex.
#[derive(Default, Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct VertexListWithOptions {
    list: Vec<(Vertex, VertexOptions)>,
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

    /// The desired [`Group`] for this vertex. Note a vertex's group can be
    /// moved down (ex. `NON_MASTER` to `MASTER`) but not moved up
    /// (ex. `MASTER` to `NON_MASTER`).
    /// - If the vertex does not exist, it will be inserted into the
    ///   `desired_group`.
    /// - If the vertex is already in a lower group than `desired_group`,
    ///   the vertex will stay in that lower group unchanged.
    /// - If the vertex is in a higher group than `desired_group`,
    ///   the implementation (ex. `add_heads_and_flush`) might move the vertex
    ///   (and its ancestors) to a lower group, or error out.
    #[serde(default = "default_desired_group")]
    pub desired_group: Group,
}

const fn default_desired_group() -> Group {
    Group::NON_MASTER
}

impl Default for VertexOptions {
    fn default() -> Self {
        Self {
            reserve_size: 0,
            desired_group: default_desired_group(),
        }
    }
}

impl<'a> From<&'a [Vertex]> for VertexListWithOptions {
    fn from(list: &'a [Vertex]) -> Self {
        // Just use default options.
        Self {
            list: list
                .iter()
                .map(|v| (v.clone(), VertexOptions::default()))
                .collect(),
        }
    }
}

impl From<Vec<Vertex>> for VertexListWithOptions {
    fn from(list: Vec<Vertex>) -> Self {
        // Just use default options.
        Self {
            list: list
                .into_iter()
                .map(|v| (v, VertexOptions::default()))
                .collect(),
        }
    }
}

impl From<Vec<(Vertex, VertexOptions)>> for VertexListWithOptions {
    fn from(list: Vec<(Vertex, VertexOptions)>) -> Self {
        Self { list }
    }
}

impl VertexListWithOptions {
    /// Get the vertexes and their options.
    pub fn vertex_options(&self) -> Vec<(Vertex, VertexOptions)> {
        self.list.clone()
    }

    /// Get the vertexes.
    pub fn vertexes(&self) -> Vec<Vertex> {
        self.list.iter().map(|i| i.0.clone()).collect()
    }

    /// Sort the heads. Lower desired group first.
    pub fn sort_by_group(mut self) -> Self {
        self.list.sort_by_key(|(_v, opts)| opts.desired_group.0);
        self
    }

    /// Get the vertexes, filter by the `desired_group` option.
    pub fn vertexes_by_group(&self, group: Group) -> Vec<Vertex> {
        self.list
            .iter()
            .filter_map(|(v, o)| {
                if o.desired_group == group {
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
    pub fn push(&mut self, head_opts: (Vertex, VertexOptions)) {
        self.list.push(head_opts);
    }

    /// Set the `desired_group` option for all vertexes.
    pub fn with_desired_group(mut self, group: Group) -> Self {
        for (_v, opts) in self.list.iter_mut() {
            opts.desired_group = group;
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

    /// Length of the `VertexListWithOptions`.
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// Minimal `desired_group` from all heads. `None` for empty heads list.
    pub fn min_desired_group(&self) -> Option<Group> {
        self.list.iter().map(|v| v.1.desired_group).min()
    }

    /// Maximum `desired_group` from all heads. `None` for empty heads list.
    pub fn max_desired_group(&self) -> Option<Group> {
        self.list.iter().map(|v| v.1.desired_group).max()
    }
}
