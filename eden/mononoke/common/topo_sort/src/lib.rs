/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    iter,
};

/// Sort nodes of DAG topologically. Implemented as depth-first search with tail-call
/// eliminated. Complexity: `O(N)` from number of nodes.
/// It returns None if graph has a cycle.
/// Nodes with no outgoing edges will be *first* in the resulting vector i.e. ancestors go first
pub fn sort_topological<T>(dag: &HashMap<T, Vec<T>>) -> Option<Vec<T>>
where
    T: Clone + Eq + Hash,
{
    /// Current state of the node in the DAG
    enum Mark {
        /// DFS is currently visiting the sub-DAG, reachable from this node
        /// *and* it entered this sub-DAG from this node. (the node is present
        /// deeper in the stack with `Action::Mark`). If there is an
        /// `InProgress` node at the end of the edge we are traversing,
        /// this means that the graph has a cycle.
        InProgress,
        /// The node has been visited before, and we have already visited
        /// the entire sub-DAG, reachable from this node
        Visited,
    }

    /// Action to be applied to the node, once we pop it from the stack
    enum Action<T> {
        /// Visit the node and every node, reachable from it, which has
        /// not been visited yet. Mark the node as `Mark::InProgress`
        Visit(T),
        /// Mark the node `Mark::Visited`, as we have just finished
        /// processing the entire sub-DAG reachable from it. This node
        /// no longer needs visiting
        Mark(T),
    }

    let mut marks = HashMap::new();
    let mut stack = Vec::new();
    let mut output = Vec::new();
    for node in dag
        .iter()
        .flat_map(|(n, ns)| iter::once(n).chain(ns))
        .collect::<HashSet<_>>()
    {
        stack.push(Action::Visit(node));
        while let Some(action) = stack.pop() {
            match action {
                Action::Visit(node) => {
                    if let Some(mark) = marks.get(node) {
                        match mark {
                            Mark::InProgress => return None, // cycle
                            Mark::Visited => continue,
                        }
                    }
                    marks.insert(node, Mark::InProgress);
                    stack.push(Action::Mark(node));
                    if let Some(children) = dag.get(node) {
                        for child in children {
                            stack.push(Action::Visit(child));
                        }
                    }
                }
                Action::Mark(node) => {
                    marks.insert(node, Mark::Visited);
                    output.push(node.clone());
                }
            }
        }
    }

    Some(output)
}

#[cfg(test)]
mod test {

    use super::*;
    use maplit::hashmap;

    #[test]
    fn sort_topological_test() {
        let res = sort_topological(&hashmap! {1 => vec![2]});
        assert_eq!(Some(vec![2, 1]), res);

        let res = sort_topological(&hashmap! {1 => vec![1]});
        assert_eq!(None, res);

        let res = sort_topological(&hashmap! {1 => vec![2], 2 => vec![3]});
        assert_eq!(Some(vec![3, 2, 1]), res);

        let res = sort_topological(&hashmap! {1 => vec![2, 3], 2 => vec![3]});
        assert_eq!(Some(vec![3, 2, 1]), res);

        let res = sort_topological(&hashmap! {1 => vec![2, 3], 2 => vec![4], 3 => vec![4]});
        assert!(Some(vec![4, 3, 2, 1]) == res || Some(vec![4, 2, 3, 1]) == res);
    }
}
