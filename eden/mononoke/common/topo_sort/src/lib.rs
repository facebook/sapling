/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::hash::Hash;
use std::iter;

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

// Wrapper that allows traversing commits in toposorted order. The biggest difference
// from sort_topological function above is that it allows visiting multiple commits
// in parallel, however it requires the caller to mark which commits were already visited.
// E.g. for a graph like
// A
// |\
// B C
//
// `sort_topological` returns a single order [B, C, A]. It's not clear that B and C are independent
// of each other and can be processed in parallel.
//
// On the other hand TopoSortedDagTraversal::drain() method first returns [B, C],
// and once B and C are marked as visited then commit A is returned. This allows processing
// commits B and C in parallel.
pub struct TopoSortedDagTraversal<T> {
    child_to_parents: HashMap<T, BTreeSet<T>>,
    parent_to_children: HashMap<T, BTreeSet<T>>,
    q: VecDeque<T>,
}

impl<T> TopoSortedDagTraversal<T>
where
    T: Copy + Clone + Eq + Hash + Ord,
{
    pub fn new(child_to_parents: HashMap<T, Vec<T>>) -> Self {
        let child_to_parents = child_to_parents
            .into_iter()
            .map(|(v, parents)| {
                let parents = parents.into_iter().collect::<BTreeSet<_>>();
                (v, parents)
            })
            .collect::<HashMap<_, _>>();

        // Find revert mapping - from parent to child
        let mut parent_to_children: HashMap<_, BTreeSet<_>> = HashMap::new();
        for (child, parents) in &child_to_parents {
            for p in parents {
                parent_to_children.entry(*p).or_default().insert(*child);
            }
        }

        // If we have no parents, then we can just add it to the queue
        let mut q = VecDeque::new();
        for (child, parents) in &child_to_parents {
            if parents.is_empty() {
                q.push_back(*child);
            }
            // An entry from `parents` does not have to be a key in child_to_parents.
            // e.g. child_to_parents - {1 => {2}}. `2` is not a key in child_to_parents,
            // but we still want to return it, and that's why below we are
            // adding it to the queue.
            for p in parents.iter() {
                if !child_to_parents.contains_key(p) {
                    q.push_back(*p);
                }
            }
        }

        Self {
            child_to_parents,
            parent_to_children,
            q,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.q.is_empty()
    }

    pub fn drain(&mut self, max_to_drain: usize) -> impl Iterator<Item = T> + '_ {
        self.q.drain(..std::cmp::min(self.q.len(), max_to_drain))
    }

    pub fn visited(&mut self, visited: T) {
        let children = match self.parent_to_children.get_mut(&visited) {
            Some(children) => children,
            None => {
                return;
            }
        };

        for child in children.iter() {
            if let Some(parents) = self.child_to_parents.get_mut(child) {
                parents.remove(&visited);
                if parents.is_empty() {
                    self.q.push_back(*child);
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use maplit::hashmap;
    use maplit::hashset;

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

    #[test]
    fn topo_sorted_traversal() {
        let mut dag = TopoSortedDagTraversal::new(hashmap! {1 => vec![]});
        assert_eq!(dag.drain(10).collect::<Vec<_>>(), vec![1]);
        assert_eq!(dag.drain(10).collect::<Vec<_>>(), vec![]);

        let mut dag = TopoSortedDagTraversal::new(hashmap! {1 => vec![2], 2 => vec![3]});
        assert_eq!(dag.drain(10).collect::<Vec<_>>(), vec![3]);
        dag.visited(3);
        assert_eq!(dag.drain(10).collect::<Vec<_>>(), vec![2]);
        assert_eq!(dag.drain(10).collect::<Vec<_>>(), vec![]);
        dag.visited(2);
        assert_eq!(dag.drain(10).collect::<Vec<_>>(), vec![1]);

        //   1
        //  / \
        // 2   3
        //  \ /
        //   4
        let mut dag = TopoSortedDagTraversal::new(
            hashmap! {1 => vec![2, 3], 2 => vec![4], 3 => vec![4], 4 => vec![]},
        );
        assert_eq!(dag.drain(10).collect::<Vec<_>>(), vec![4]);
        assert_eq!(dag.drain(10).collect::<Vec<_>>(), vec![]);
        dag.visited(4);
        assert_eq!(dag.drain(10).collect::<HashSet<_>>(), hashset![2, 3]);
        dag.visited(2);
        assert_eq!(dag.drain(10).collect::<Vec<_>>(), vec![]);
        dag.visited(3);
        assert_eq!(dag.drain(10).collect::<Vec<_>>(), vec![1]);

        //   1
        //  / \
        // 2   3
        // |   |
        // 4   5
        let mut dag = TopoSortedDagTraversal::new(
            hashmap! {1 => vec![2, 3], 2 => vec![4], 3 => vec![5], 4 => vec![], 5 => vec![]},
        );
        assert_eq!(dag.drain(2).collect::<HashSet<_>>(), hashset![4, 5]);
        dag.visited(4);
        dag.visited(5);
        assert_eq!(dag.drain(2).collect::<HashSet<_>>(), hashset![2, 3]);
        dag.visited(2);
        dag.visited(3);
        assert_eq!(dag.drain(2).collect::<HashSet<_>>(), hashset![1]);
    }
}
