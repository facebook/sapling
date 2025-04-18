/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::collections::HashSet;

use dag::Dag;
use dag::Vertex;
use dag::ops::DagAddHeads;

use crate::GraphNode;
use crate::HgCommit;
use crate::Result;

pub(crate) fn commits_to_graph_nodes(commits: &[HgCommit]) -> Vec<GraphNode> {
    commits
        .iter()
        .map(|c| GraphNode {
            vertex: c.vertex.clone(),
            parents: c.parents.clone(),
        })
        .collect::<Vec<_>>()
}

pub(crate) async fn add_graph_nodes_to_dag(dag: &mut Dag, graph_nodes: &[GraphNode]) -> Result<()> {
    // Write commit graph to DAG.
    let parents: HashMap<Vertex, Vec<Vertex>> = graph_nodes
        .iter()
        .cloned()
        .map(|c| (c.vertex, c.parents))
        .collect();
    let heads: Vec<Vertex> = {
        let mut non_heads = HashSet::new();
        for graph_node in graph_nodes {
            for parent in graph_node.parents.iter() {
                non_heads.insert(parent);
            }
        }
        graph_nodes
            .iter()
            .map(|c| &c.vertex)
            .filter(|v| !non_heads.contains(v))
            .cloned()
            .collect()
    };
    dag.add_heads(&parents, &heads.into()).await?;
    Ok(())
}
