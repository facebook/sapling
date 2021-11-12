/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::VertexName;

/// A list of [`VertexName`]s (usually heads) with options attached to each vertex.
#[derive(Default, Debug)]
pub struct VertexListWithOptions {
    list: Vec<(VertexName, VertexOptions)>,
}

/// Options attached to a vertex. Usually the vertex is a head. The head and its
/// ancestors are going to be inserted to the graph. The options controls some
/// details about the insertion.
#[derive(Default, Debug, Clone)]
#[non_exhaustive]
pub struct VertexOptions {
    /// How many ids to reserve for this vertex. Suppose this vertex has id `n`,
    /// then `n+1..=n+reserve_size` can only be used when inserting this vertex
    /// and its ancestors in the same batch.
    ///
    /// Note: if any id `j` in the `n+1..=n+reserve_size` range were already
    /// taken, then the reserve range becomes `n+1..j` instead. This avoids
    /// fragmentation.
    pub reserve_size: u32,
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

impl<'a> From<Vec<(VertexName, VertexOptions)>> for VertexListWithOptions {
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
}
