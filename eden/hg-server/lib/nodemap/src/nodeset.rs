/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use indexedlog::{
    log::{self, IndexOutput, Log},
    DefaultOpenOptions,
};
use std::path::Path;
use thiserror::Error;
use types::errors::KeyError;
use types::node::Node;

#[derive(Debug, Error)]
#[error("Node Set Error: {0:?}")]
struct NodeSetError(String);

impl From<NodeSetError> for KeyError {
    fn from(err: NodeSetError) -> Self {
        KeyError::new(err.into())
    }
}

/// A persistent set of Nodes.
///
/// [NodeSet] is implemented on top of [indexedlog::log::Log] to store
/// a set of nodes. Its insertion and lookup complexity are `O(log N)`.
pub struct NodeSet {
    log: Log,
}

impl DefaultOpenOptions<log::OpenOptions> for NodeSet {
    fn default_open_options() -> log::OpenOptions {
        let node_index = |_data: &[u8]| vec![IndexOutput::Reference(0..Node::len() as u64)];
        log::OpenOptions::new()
            .create(true)
            .index("node", node_index)
    }
}

impl NodeSet {
    const INDEX_NODE: usize = 0;

    pub fn open(dir: impl AsRef<Path>) -> Result<Self> {
        Ok(NodeSet {
            log: Self::default_open_options().open(dir.as_ref())?,
        })
    }

    pub fn flush(&mut self) -> Result<()> {
        self.log.flush()?;
        Ok(())
    }

    pub fn add(&mut self, node: &Node) -> Result<()> {
        if !self.contains(node)? {
            self.log.append(node.as_ref())?;
        }
        Ok(())
    }

    pub fn contains(&self, node: &Node) -> Result<bool> {
        let mut lookup_iter = self.log.lookup(Self::INDEX_NODE, node.as_ref())?;
        Ok(lookup_iter.next().is_some())
    }

    pub fn iter<'a>(&'a self) -> impl Iterator<Item = Result<Node>> + 'a {
        self.log.iter().map(|slice| Node::from_slice(slice?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::quickcheck;
    use std::collections::HashSet;
    use tempfile::TempDir;

    quickcheck! {
        fn test_compare_with_hashset(nodes: HashSet<Node>) -> bool {
            let dir = TempDir::new().unwrap();
            let mut set = NodeSet::open(dir).unwrap();

            for node in &nodes {
                assert!(!set.contains(node).unwrap());
                set.add(node).unwrap();
                assert!(set.contains(node).unwrap());
            }

            let nodes2: HashSet<Node> = set.iter().map(|node| node.unwrap()).collect();
            nodes2 == nodes
        }
    }
}
