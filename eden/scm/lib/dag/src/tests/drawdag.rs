/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::collections::HashSet;

use crate::namedag::MemNameDag;
use crate::ops::Parents;
use crate::Result;
use crate::Vertex;

/// Represents a graph from ASCII parsed by `drawdag`.
pub struct DrawDag {
    parents: HashMap<Vertex, Vec<Vertex>>,
}

impl<'a> From<&'a str> for DrawDag {
    fn from(text: &'a str) -> Self {
        let parents = ::drawdag::parse(text);
        let v = |s: String| Vertex::copy_from(s.as_bytes());
        let parents = parents
            .into_iter()
            .map(|(k, vs)| (v(k), vs.into_iter().map(v).collect()))
            .collect();
        Self { parents }
    }
}

impl DrawDag {
    /// Heads in the graph. Vertexes that are not parents of other vertexes.
    pub fn heads(&self) -> Vec<Vertex> {
        let mut heads = self
            .parents
            .keys()
            .collect::<HashSet<_>>()
            .difference(&self.parents.values().flatten().collect())
            .map(|&v| v.clone())
            .collect::<Vec<_>>();
        heads.sort();
        heads
    }
}

#[async_trait::async_trait]
impl Parents for DrawDag {
    async fn parent_names(&self, name: Vertex) -> Result<Vec<Vertex>> {
        Parents::parent_names(&self.parents, name).await
    }

    async fn hint_subdag_for_insertion(&self, heads: &[Vertex]) -> Result<MemNameDag> {
        Parents::hint_subdag_for_insertion(&self.parents, heads).await
    }
}
