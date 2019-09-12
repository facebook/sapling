// Copyright Facebook, Inc. 2018

use std::ops::Range;
use std::path::Path;

use failure::{Fail, Fallible};
use indexedlog::log::{self, IndexOutput, Log};
use types::errors::KeyError;
use types::node::Node;

#[derive(Debug, Fail)]
#[fail(display = "Node Map Error: {:?}", _0)]
struct NodeMapError(String);

impl From<NodeMapError> for KeyError {
    fn from(err: NodeMapError) -> Self {
        KeyError::new(err.into())
    }
}

/// A persistent bidirectional mapping between two Nodes
///
/// [NodeMap] is implemented on top of [indexedlog::log::Log] to store a mapping between two kinds
/// of nodes.
pub struct NodeMap {
    log: Log,
}

impl NodeMap {
    pub fn open(dir: impl AsRef<Path>) -> Fallible<Self> {
        // Update the index every 100KB, i.e. every 256 entries
        let first_index = |_data: &[u8]| vec![IndexOutput::Reference(0..20)];
        let second_index = |_data: &[u8]| vec![IndexOutput::Reference(20..40)];
        Ok(NodeMap {
            log: log::OpenOptions::new()
                .create(true)
                .index("first", first_index)
                .index("second", second_index)
                .open(dir)?,
        })
    }

    pub fn flush(&mut self) -> Fallible<()> {
        self.log.flush()?;
        Ok(())
    }

    pub fn add(&mut self, first: &Node, second: &Node) -> Fallible<()> {
        let mut buf = Vec::with_capacity(40);
        buf.extend_from_slice(first.as_ref());
        buf.extend_from_slice(second.as_ref());
        self.log.append(buf).map_err(|e| e.into())
    }

    pub fn lookup_by_first(&self, first: &Node) -> Fallible<Option<Node>> {
        self.lookup(first, 0, 20..40)
    }

    pub fn lookup_by_second(&self, second: &Node) -> Fallible<Option<Node>> {
        self.lookup(second, 1, 0..20)
    }

    fn lookup(&self, key: &Node, index_id: usize, range: Range<usize>) -> Fallible<Option<Node>> {
        let mut lookup_iter = self.log.lookup(index_id, key)?;
        Ok(match lookup_iter.next() {
            Some(result) => Some(Node::from_slice(&result?[range])?),
            None => None,
        })
    }

    pub fn iter<'a>(&'a self) -> Fallible<Box<dyn Iterator<Item = Fallible<(Node, Node)>> + 'a>> {
        let iter = self.log.iter().map(move |entry| match entry {
            Ok(data) => {
                let mut first = self.log.index_func(0, &data)?;
                if first.len() != 1 {
                    return Err(NodeMapError(format!(
                        "invalid index 1 keys in {:?}",
                        self.log.dir
                    ))
                    .into());
                }
                let first = first.pop().unwrap();
                let mut second = self.log.index_func(1, &data)?;
                if second.len() != 1 {
                    return Err(NodeMapError(format!(
                        "invalid index 2 keys in {:?}",
                        self.log.dir
                    ))
                    .into());
                }
                let second = second.pop().unwrap();

                Ok((Node::from_slice(&first)?, Node::from_slice(&second)?))
            }
            Err(e) => Err(e.into()),
        });
        Ok(Box::new(iter))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::quickcheck;
    use tempfile::TempDir;

    quickcheck! {
        fn test_roundtrip(pairs: Vec<(Node, Node)>) -> bool {
            let mut pairs = pairs;
            if pairs.len() == 0 {
                return true;
            }

            let dir = TempDir::new().unwrap();
            let mut map = NodeMap::open(dir).unwrap();
            let last = pairs.pop().unwrap();
            for (first, second) in pairs.iter() {
                map.add(&first, &second).unwrap();
            }

            for (first, second) in pairs.iter() {
                if first != &map.lookup_by_second(second).unwrap().unwrap() {
                    return false;
                }
                if second != &map.lookup_by_first(first).unwrap().unwrap() {
                    return false;
                }
            }

            for value in vec![last.0, last.1].iter() {
                if !map.lookup_by_first(value).unwrap().is_none() {
                    return false;
                }
                if !map.lookup_by_second(value).unwrap().is_none() {
                    return false;
                }

            }

            let actual_pairs = map.iter().unwrap().collect::<Fallible<Vec<_>>>().unwrap();
            actual_pairs == pairs
        }
    }
}
