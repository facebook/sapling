// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! The Tree structure from this module is designed to represent a file system and has the
//! following characterictics:
//! - all leaf nodes hold values (files), the other nodes (folders) do not have values
//! - the nodes have links to their parent and children to enable traversing up and down the tree
//! - every tree contains a root node that is not a leaf and is the only node that does not have
//!   a parent

use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;

use errors::*;

use itertools::FoldWhile;
use itertools::Itertools;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct TNodeId(usize);
pub static ROOT_ID: TNodeId = TNodeId(0);

pub enum TreeValue<K, V> {
    Leaf(V),
    Node(HashMap<K, TNodeId>),
}

impl<K: Hash + Eq, V> TreeValue<K, V> {
    pub fn get_leaf(&self) -> Option<&V> {
        match self {
            &TreeValue::Leaf(ref data) => Some(data),
            _ => None,
        }
    }

    pub fn get_node(&self) -> Option<&HashMap<K, TNodeId>> {
        match self {
            &TreeValue::Node(ref data) => Some(data),
            _ => None,
        }
    }
}

struct TreeNode<K, V> {
    parent: Option<TNodeId>,
    value: TreeValue<K, V>,
}

pub struct Tree<K, V>(Vec<TreeNode<K, V>>);

impl<K: Hash + Eq + Clone, V> Tree<K, V> {
    pub fn new() -> Self {
        Tree(vec![
            TreeNode {
                parent: None,
                value: TreeValue::Node(HashMap::new()),
            },
        ])
    }

    pub fn get_parent(&self, nodeid: TNodeId) -> Option<TNodeId> {
        self.0.get(nodeid.0).and_then(|node| node.parent)
    }

    pub fn get_child(&self, parent_nodeid: TNodeId, key: &K) -> Option<TNodeId> {
        self.get_value(parent_nodeid).and_then(|value| {
            value.get_node().and_then(|nodes| nodes.get(key).cloned())
        })
    }

    pub fn get_value(&self, nodeid: TNodeId) -> Option<&TreeValue<K, V>> {
        self.0.get(nodeid.0).map(|node| &node.value)
    }

    /// This method inserts into the tree the `leaf` under the `leaf_key` at the end of the `path`
    pub fn insert<Keys>(&mut self, path: Keys, leaf_key: K, leaf: V) -> Result<()>
    where
        Keys: IntoIterator<Item = K>,
    {
        let mut path = path.into_iter();

        let nodeid = path.fold_while(Ok(ROOT_ID), |nodeid, key| {
            let nodeid = nodeid.expect("short circuting shouldn't allow nodeid to be Err");

            let maybe_child = match self.0
                .get(nodeid.0)
                .expect(
                    "inconsistency in Tree, the passed nodeid was either ROOT_ID \
                     or a result of register_node and yet is not present",
                )
                .value
            {
                TreeValue::Node(ref nodes) => nodes.get(&key).cloned(),
                _ => None,
            };

            short_circuit(maybe_child.map_or_else(
                || self.register_node(nodeid, key, TreeValue::Node(HashMap::new())),
                Ok,
            ))
        }).into_inner()?;

        self.register_node(nodeid, leaf_key, TreeValue::Leaf(leaf))?;
        Ok(())
    }

    fn register_node(
        &mut self,
        parent_nodeid: TNodeId,
        key: K,
        value: TreeValue<K, V>,
    ) -> Result<TNodeId> {
        let nodeid = TNodeId(self.0.len());
        self.0.push(TreeNode {
            parent: Some(parent_nodeid),
            value,
        });
        match self.0.get_mut(parent_nodeid.0).unwrap().value {
            TreeValue::Node(ref mut nodes) => {
                nodes.insert(key, nodeid);
                Ok(nodeid)
            }
            _ => bail_err!(ErrorKind::TreeInsert(
                "Tried to insert a subnode into a leaf node".into(),
            )),
        }
    }
}

impl<K, V> fmt::Debug for Tree<K, V>
where
    K: Hash + Eq + Clone + fmt::Debug,
    V: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.0.is_empty() {
            return write!(f, "Tree(invalid rootless tree)");
        }

        fn print_node<K, V>(tree: &Tree<K, V>, nodeid: TNodeId) -> String
        where
            K: Hash + Eq + Clone + fmt::Debug,
            V: fmt::Debug,
        {
            match tree.0.get(nodeid.0) {
                None => "ERROR: node does not exist".to_owned(),
                Some(ref node) => match node.value {
                    TreeValue::Leaf(ref leaf) => format!("Leaf({:?})", leaf),
                    TreeValue::Node(ref nodes) => format!(
                        "Node([{}])",
                        nodes
                            .iter()
                            .map(|(key, nodeid)| {
                                format!("({:?}, {})", key, print_node(tree, *nodeid))
                            })
                            .join(", ")
                    ),
                },
            }
        }

        write!(f, "{}", print_node(self, ROOT_ID))
    }
}

/// Function for short circuting a fold_while on Result::Err
fn short_circuit<T>(result: Result<T>) -> FoldWhile<Result<T>> {
    if result.is_ok() {
        FoldWhile::Continue(result)
    } else {
        FoldWhile::Done(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Copy, Clone, Debug, PartialEq, Eq)]
    struct Leaf(usize);

    struct LeafGen(usize);

    impl LeafGen {
        fn new() -> Self {
            LeafGen(0)
        }

        fn next(&mut self) -> Leaf {
            self.0 += 1;
            Leaf(self.0)
        }
    }

    fn example_tree() -> (Tree<usize, Leaf>, Vec<(Vec<usize>, Leaf)>) {
        let mut gen = LeafGen::new();
        let leafs = vec![
            (vec![1, 1, 1], gen.next()),
            (vec![1, 1, 2], gen.next()),
            (vec![1, 1, 3, 1], gen.next()),
            (vec![2], gen.next()),
        ];

        let mut tree = Tree::new();
        for &(ref path, ref leaf) in &leafs {
            let (leaf_key, path) = path.split_last().unwrap();
            tree.insert(path.into_iter().cloned(), leaf_key.clone(), leaf.clone())
                .unwrap();
        }
        (tree, leafs)
    }

    #[test]
    fn test_non_existing_nodes() {
        let (tree, _) = example_tree();

        assert!(tree.get_parent(ROOT_ID).is_none());

        assert!(tree.get_child(ROOT_ID, &444).is_none());
        assert!(tree.get_child(TNodeId(444), &1).is_none());

        assert!(tree.get_value(TNodeId(444)).is_none());
    }

    #[test]
    fn test_traverse_tree() {
        let (tree, leafs) = example_tree();

        for &(ref path, ref leaf) in &leafs {
            let nodeid = path.iter()
                .fold(ROOT_ID, |nodeid, p| tree.get_child(nodeid, p).unwrap());

            assert_eq!(tree.get_value(nodeid).unwrap().get_leaf().unwrap(), leaf);

            assert_eq!(
                path.iter()
                    .fold(Some(nodeid), |nodeid, _| tree.get_parent(nodeid.unwrap()))
                    .unwrap(),
                ROOT_ID
            );
        }
    }

    fn count_leafs(tree: &Tree<usize, Leaf>, nodeid: TNodeId) -> usize {
        match tree.get_value(nodeid).unwrap() {
            &TreeValue::Leaf(_) => 1,
            &TreeValue::Node(ref map) => map.values()
                .into_iter()
                .fold(0, |cnt, id| cnt + count_leafs(tree, id.clone())),
        }
    }

    #[test]
    fn test_traverse_values() {
        let (tree, leafs) = example_tree();
        assert_eq!(count_leafs(&tree, ROOT_ID), leafs.len());
    }
}
