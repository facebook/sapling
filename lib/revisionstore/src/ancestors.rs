// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    iter::Iterator,
};

use failure::{Fail, Fallible};

use types::{Key, NodeInfo};

use crate::historystore::Ancestors;

#[derive(Debug, Fail)]
#[fail(display = "Ancestor Iterator Error: {:?}", _0)]
struct AncestorIteratorError(String);

pub enum AncestorTraversal {
    Partial,
    Complete,
}

pub struct AncestorIterator<T: Fn(&Key, &HashSet<Key>) -> Fallible<Option<NodeInfo>>> {
    traversal: AncestorTraversal,
    get_more: T,
    seen: HashSet<Key>,
    queue: VecDeque<Key>,
}

pub struct BatchedAncestorIterator<T: Fn(&Key, &HashSet<Key>) -> Fallible<Option<Ancestors>>> {
    #[allow(dead_code)]
    traversal: AncestorTraversal,
    get_more: T,
    seen: HashSet<Key>,
    queue: VecDeque<Key>,
    pending_infos: HashMap<Key, NodeInfo>,
}

impl<T: Fn(&Key, &HashSet<Key>) -> Fallible<Option<NodeInfo>>> AncestorIterator<T> {
    pub fn new(key: &Key, get_more: T, traversal: AncestorTraversal) -> Self {
        let mut iter = AncestorIterator {
            traversal,
            get_more,
            seen: HashSet::new(),
            queue: VecDeque::new(),
        };
        iter.queue.push_back(key.clone());
        iter.seen.insert(key.clone());

        // Insert the null id so we stop iterating there
        iter.seen.insert(Key::default());
        iter
    }
}

impl<T: Fn(&Key, &HashSet<Key>) -> Fallible<Option<Ancestors>>> BatchedAncestorIterator<T> {
    pub fn new(key: &Key, get_more: T, traversal: AncestorTraversal) -> Self {
        let mut iter = BatchedAncestorIterator {
            traversal,
            get_more,
            seen: HashSet::new(),
            queue: VecDeque::new(),
            pending_infos: HashMap::new(),
        };
        iter.queue.push_back(key.clone());
        iter.seen.insert(key.clone());

        // Insert the null id so we stop iterating there
        iter.seen.insert(Key::default());
        iter
    }
}

impl<T: Fn(&Key, &HashSet<Key>) -> Fallible<Option<NodeInfo>>> Iterator for AncestorIterator<T> {
    type Item = Fallible<Option<(Key, NodeInfo)>>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(current) = self.queue.pop_front() {
            return match (self.get_more)(&current, &self.seen) {
                Ok(Some(node_info)) => {
                    for parent in node_info.parents.iter() {
                        // We add the parent to seen here (vs when we pop it off), so we can
                        // avoid processing commits an exponential number of times same
                        // commits multiple times when traversing across a very mergy history.
                        if self.seen.insert(parent.clone()) {
                            self.queue.push_back(parent.clone());
                        }
                    }
                    Some(Ok(Some((current, node_info.clone()))))
                }
                Ok(None) => match self.traversal {
                    AncestorTraversal::Partial => {
                        if self.seen.len() == 2 {
                            return Some(Ok(None));
                        }
                        continue;
                    }
                    AncestorTraversal::Complete => Some(Ok(None)),
                },
                Err(e) => match self.traversal {
                    AncestorTraversal::Partial => {
                        // If the only entries are the original and the the null entry,
                        // then we were unable to find the desired key, which is an error.
                        if self.seen.len() == 2 {
                            return Some(Err(e));
                        }
                        continue;
                    }
                    AncestorTraversal::Complete => Some(Err(e)),
                },
            };
        }

        None
    }
}

impl<T: Fn(&Key, &HashSet<Key>) -> Fallible<Option<Ancestors>>> Iterator
    for BatchedAncestorIterator<T>
{
    type Item = Fallible<Option<(Key, NodeInfo)>>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(current) = self.queue.pop_front() {
            if !self.pending_infos.contains_key(&current) {
                match (self.get_more)(&current, &self.seen) {
                    Err(e) => return Some(Err(e)),
                    Ok(None) => return Some(Ok(None)),
                    Ok(Some(partial_ancestors)) => {
                        for (key, node_info) in partial_ancestors.iter() {
                            self.pending_infos.insert(key.clone(), node_info.clone());
                        }
                    }
                };
            }

            if let Some(node_info) = self.pending_infos.remove(&current) {
                for parent in &node_info.parents {
                    if self.seen.insert(parent.clone()) {
                        self.queue.push_back(parent.clone());
                    }
                }

                Some(Ok(Some((current.clone(), node_info.clone()))))
            } else {
                Some(Err(AncestorIteratorError(format!(
                    "expected {:?} ancestor",
                    current
                ))
                .into()))
            }
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use types::{testutil::*, Node, RepoPathBuf};

    fn build_diamond_graph() -> (Key, Ancestors) {
        let mut ancestors = Ancestors::new();
        let keys = vec![key("a", "1"), key("b", "2"), key("c", "3"), key("d", "4")];
        let null_key = Key::new(RepoPathBuf::new(), Node::null_id().clone());

        // Build a simple diamond graph
        ancestors.insert(
            keys[0].clone(),
            NodeInfo {
                parents: [keys[1].clone(), keys[2].clone()],
                linknode: node("101"),
            },
        );
        ancestors.insert(
            keys[1].clone(),
            NodeInfo {
                parents: [keys[3].clone(), null_key.clone()],
                linknode: node("102"),
            },
        );
        ancestors.insert(
            keys[2].clone(),
            NodeInfo {
                parents: [keys[3].clone(), null_key.clone()],
                linknode: node("103"),
            },
        );
        ancestors.insert(
            keys[3].clone(),
            NodeInfo {
                parents: [null_key.clone(), null_key.clone()],
                linknode: node("104"),
            },
        );

        return (keys[0].clone(), ancestors);
    }

    #[test]
    fn test_single_ancestor_iterator() {
        let (tip, ancestors) = build_diamond_graph();

        let found_ancestors = AncestorIterator::new(
            &tip,
            |k, _seen| Ok(ancestors.get(&k).cloned()),
            AncestorTraversal::Complete,
        )
        .collect::<Fallible<Option<Ancestors>>>()
        .unwrap()
        .unwrap();
        assert_eq!(ancestors, found_ancestors);
    }

    #[test]
    fn test_batched_ancestor_iterator() {
        let (tip, ancestors) = build_diamond_graph();

        let found_ancestors = BatchedAncestorIterator::new(
            &tip,
            |k, _seen| {
                let mut k_ancestors = Ancestors::new();
                let k_info = ancestors.get(k).unwrap();
                k_ancestors.insert(k.clone(), k_info.clone());

                // For the tip commit, return two entries to simulate a batch
                if k == &tip {
                    let k_p1_info = ancestors.get(&k_info.parents[0]).unwrap();
                    k_ancestors.insert(k_info.parents[0].clone(), k_p1_info.clone());
                }
                Ok(Some(k_ancestors))
            },
            AncestorTraversal::Complete,
        )
        .collect::<Fallible<Option<Ancestors>>>()
        .unwrap()
        .unwrap();
        assert_eq!(ancestors, found_ancestors);
    }

    #[test]
    fn test_mergey_ancestor_iterator() {
        // Tests for exponential time complexity in a merge ancestory. This doesn't won't fail,
        // but may take a long time if there is bad time complexity.
        let size = 5000;
        let mut ancestors = Ancestors::new();
        let mut keys = vec![];
        for i in 1..=size {
            keys.push(key(&i.to_string(), &i.to_string()));
        }
        let null_key = Key::new(RepoPathBuf::new(), Node::null_id().clone());

        // Build a mergey history where commit N has parents N-1 and N-2
        for i in 0..size {
            let p1 = if i > 0 {
                keys[i - 1].clone()
            } else {
                null_key.clone()
            };
            let p2 = if i > 1 {
                keys[i - 2].clone()
            } else {
                null_key.clone()
            };
            ancestors.insert(
                keys[i].clone(),
                NodeInfo {
                    parents: [p1, p2],
                    linknode: node(&(10000 + i).to_string()),
                },
            );
        }

        let tip = keys[size - 1].clone();

        let found_ancestors = AncestorIterator::new(
            &tip,
            |k, _seen| Ok(ancestors.get(&k).cloned()),
            AncestorTraversal::Complete,
        )
        .collect::<Fallible<Option<Ancestors>>>()
        .unwrap()
        .unwrap();
        assert_eq!(ancestors, found_ancestors);
    }
}
