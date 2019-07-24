// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    iter,
};

/// Sort nodes of DAG topologically. Implemented as depth-first search with tail-call
/// eliminated. Complexity: `O(N)` from number of nodes.
/// It returns None if graph has a cycle.
pub fn sort_topological<T>(dag: &HashMap<T, Vec<T>>) -> Option<Vec<T>>
where
    T: Clone + Eq + Hash,
{
    enum Mark {
        Temporary,
        Marked,
    }

    enum Action<T> {
        Visit(T),
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
                            Mark::Temporary => return None, // cycle
                            Mark::Marked => continue,
                        }
                    }
                    marks.insert(node, Mark::Temporary);
                    stack.push(Action::Mark(node));
                    if let Some(children) = dag.get(node) {
                        for child in children {
                            stack.push(Action::Visit(child));
                        }
                    }
                }
                Action::Mark(node) => {
                    marks.insert(node, Mark::Marked);
                    output.push(node.clone());
                }
            }
        }
    }

    output.reverse();
    Some(output)
}

#[cfg(test)]
mod test {

    use super::*;
    use maplit::hashmap;

    #[test]
    fn sort_topological_test() {
        let res = sort_topological(&hashmap! {1 => vec![2]});
        assert_eq!(Some(vec![1, 2]), res);

        let res = sort_topological(&hashmap! {1 => vec![1]});
        assert_eq!(None, res);

        let res = sort_topological(&hashmap! {1 => vec![2], 2 => vec![3]});
        assert_eq!(Some(vec![1, 2, 3]), res);

        let res = sort_topological(&hashmap! {1 => vec![2, 3], 2 => vec![3]});
        assert_eq!(Some(vec![1, 2, 3]), res);

        let res = sort_topological(&hashmap! {1 => vec![2, 3], 2 => vec![4], 3 => vec![4]});
        assert!(Some(vec![1, 2, 3, 4]) == res || Some(vec![1, 3, 2, 4]) == res);
    }
}
