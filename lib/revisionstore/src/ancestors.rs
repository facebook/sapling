use std::collections::{HashMap, HashSet, VecDeque};
use std::iter::Iterator;

use error::Result;
use historystore::{Ancestors, NodeInfo};
use key::Key;

#[derive(Debug, Fail)]
#[fail(display = "Ancestor Iterator Error: {:?}", _0)]
struct AncestorIteratorError(String);

pub enum AncestorTraversal {
    Partial,
    Complete,
}

pub struct AncestorIterator<T: Fn(&Key, &HashSet<Key>) -> Result<NodeInfo>> {
    traversal: AncestorTraversal,
    get_more: T,
    seen: HashSet<Key>,
    queue: VecDeque<Key>,
}

pub struct BatchedAncestorIterator<T: Fn(&Key, &HashSet<Key>) -> Result<Ancestors>> {
    traversal: AncestorTraversal,
    get_more: T,
    seen: HashSet<Key>,
    queue: VecDeque<Key>,
    pending_infos: HashMap<Key, NodeInfo>,
}

impl<T: Fn(&Key, &HashSet<Key>) -> Result<NodeInfo>> AncestorIterator<T> {
    pub fn new(key: &Key, get_more: T, traversal: AncestorTraversal) -> Self {
        let mut iter = AncestorIterator {
            traversal: traversal,
            get_more: get_more,
            seen: HashSet::new(),
            queue: VecDeque::new(),
        };
        iter.queue.push_back(key.clone());

        // Insert the null id so we stop iterating there
        iter.seen.insert(Key::default());
        iter
    }
}

impl<T: Fn(&Key, &HashSet<Key>) -> Result<Ancestors>> BatchedAncestorIterator<T> {
    pub fn new(key: &Key, get_more: T, traversal: AncestorTraversal) -> Self {
        let mut iter = BatchedAncestorIterator {
            traversal: traversal,
            get_more: get_more,
            seen: HashSet::new(),
            queue: VecDeque::new(),
            pending_infos: HashMap::new(),
        };
        iter.queue.push_back(key.clone());

        // Insert the null id so we stop iterating there
        iter.seen.insert(Key::default());
        iter
    }
}

impl<T: Fn(&Key, &HashSet<Key>) -> Result<NodeInfo>> Iterator for AncestorIterator<T> {
    type Item = Result<(Key, NodeInfo)>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(current) = self.queue.pop_front() {
            return match (self.get_more)(&current, &self.seen) {
                Err(e) => match self.traversal {
                    AncestorTraversal::Partial => {
                        // If the only entry is the null entry, then we were
                        // unable to find the desired key, which is an error.
                        if self.seen.len() == 1 {
                            return Some(Err(e));
                        }
                        continue;
                    }
                    AncestorTraversal::Complete => Some(Err(e)),
                },
                Ok(node_info) => {
                    self.seen.insert(current.clone());
                    for parent in node_info.parents.iter() {
                        if !self.seen.contains(parent) {
                            self.queue.push_back(parent.clone());
                        }
                    }
                    Some(Ok((current, node_info.clone())))
                }
            };
        }

        None
    }
}

impl<T: Fn(&Key, &HashSet<Key>) -> Result<Ancestors>> Iterator for BatchedAncestorIterator<T> {
    type Item = Result<(Key, NodeInfo)>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(current) = self.queue.pop_front() {
            if !self.pending_infos.contains_key(&current) {
                match (self.get_more)(&current, &self.seen) {
                    Err(e) => return Some(Err(e)),
                    Ok(partial_ancestors) => for (key, node_info) in partial_ancestors.iter() {
                        self.pending_infos.insert(key.clone(), node_info.clone());
                    },
                };
            }

            if let Some(node_info) = self.pending_infos.remove(&current) {
                self.seen.insert(current.clone());
                for parent in &node_info.parents {
                    if !self.seen.contains(parent) {
                        self.queue.push_back(parent.clone());
                    }
                }

                Some(Ok((current.clone(), node_info.clone())))
            } else {
                Some(Err(AncestorIteratorError(format!(
                    "expected {:?} ancestor",
                    current
                )).into()))
            }
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::chacha::ChaChaRng;
    use types::node::Node;

    fn build_diamond_graph() -> (Key, Ancestors) {
        let mut rng = ChaChaRng::from_seed([0u8; 32]);

        let mut ancestors = Ancestors::new();
        let keys = vec![
            Key::new(Box::from([]), Node::random(&mut rng)),
            Key::new(Box::from([]), Node::random(&mut rng)),
            Key::new(Box::from([]), Node::random(&mut rng)),
            Key::new(Box::from([]), Node::random(&mut rng)),
        ];

        let null_key = Key::new(Box::from([]), Node::null_id().clone());

        // Build a simple diamond graph
        ancestors.insert(
            keys[0].clone(),
            NodeInfo {
                parents: [keys[1].clone(), keys[2].clone()],
                linknode: Node::random(&mut rng),
            },
        );
        ancestors.insert(
            keys[1].clone(),
            NodeInfo {
                parents: [keys[3].clone(), null_key.clone()],
                linknode: Node::random(&mut rng),
            },
        );
        ancestors.insert(
            keys[2].clone(),
            NodeInfo {
                parents: [keys[3].clone(), null_key.clone()],
                linknode: Node::random(&mut rng),
            },
        );
        ancestors.insert(
            keys[3].clone(),
            NodeInfo {
                parents: [null_key.clone(), null_key.clone()],
                linknode: Node::random(&mut rng),
            },
        );

        return (keys[0].clone(), ancestors);
    }

    #[test]
    fn test_single_ancestor_iterator() {
        let (tip, ancestors) = build_diamond_graph();

        let found_ancestors = AncestorIterator::new(
            &tip,
            |k, _seen| Ok(ancestors.get(&k).unwrap().clone()),
            AncestorTraversal::Complete,
        ).collect::<Result<Ancestors>>()
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
                Ok(k_ancestors)
            },
            AncestorTraversal::Complete,
        ).collect::<Result<Ancestors>>()
            .unwrap();
        assert_eq!(ancestors, found_ancestors);
    }
}
