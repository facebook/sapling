use std::collections::{HashMap, HashSet, VecDeque};
use std::iter::Iterator;

use error::Result;
use historystore::{Ancestors, NodeInfo};
use key::Key;

#[derive(Debug, Fail)]
#[fail(display = "Ancestor Iterator Error: {:?}", _0)]
struct AncestorIteratorError(String);

pub struct AncestorIterator<T: Fn(&Key, &HashSet<Key>) -> Result<NodeInfo>> {
    get_more: T,
    seen: HashSet<Key>,
    queue: VecDeque<Key>,
}

pub struct BatchedAncestorIterator<T: Fn(&Key, &HashSet<Key>) -> Result<Ancestors>> {
    get_more: T,
    seen: HashSet<Key>,
    queue: VecDeque<Key>,
    pending_infos: HashMap<Key, NodeInfo>,
}

impl<T: Fn(&Key, &HashSet<Key>) -> Result<NodeInfo>> AncestorIterator<T> {
    pub fn new(key: &Key, get_more: T) -> Self {
        let mut iter = AncestorIterator {
            get_more: get_more,
            seen: HashSet::new(),
            queue: VecDeque::new(),
        };
        iter.queue.push_back(key.clone());
        iter
    }
}

impl<T: Fn(&Key, &HashSet<Key>) -> Result<Ancestors>> BatchedAncestorIterator<T> {
    pub fn new(key: &Key, get_more: T) -> Self {
        let mut iter = BatchedAncestorIterator {
            get_more: get_more,
            seen: HashSet::new(),
            queue: VecDeque::new(),
            pending_infos: HashMap::new(),
        };
        iter.queue.push_back(key.clone());
        iter
    }
}

impl<T: Fn(&Key, &HashSet<Key>) -> Result<NodeInfo>> Iterator for AncestorIterator<T> {
    type Item = Result<(Key, NodeInfo)>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(current) = self.queue.pop_front() {
            match (self.get_more)(&current, &self.seen) {
                Err(e) => Some(Err(e)),
                Ok(node_info) => {
                    self.seen.insert(current.clone());
                    for parent in node_info.parents.iter() {
                        if !self.seen.contains(parent) {
                            self.queue.push_back(parent.clone());
                        }
                    }
                    Some(Ok((current, node_info.clone())))
                }
            }
        } else {
            None
        }
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
