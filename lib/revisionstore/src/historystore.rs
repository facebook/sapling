use error::Result;
use key::Key;
use node::Node;

use std::collections::HashMap;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NodeInfo {
    pub parents: [Key; 2],
    pub linknode: Node,
}

pub type Ancestors = HashMap<Key, NodeInfo>;

pub trait HistoryStore {
    fn get_ancestors(&self, key: &Key) -> Result<Ancestors>;
    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>>;
    fn get_node_info(&self, key: &Key) -> Result<NodeInfo>;
}

#[cfg(test)]
use quickcheck;

#[cfg(test)]
impl quickcheck::Arbitrary for NodeInfo {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        NodeInfo {
            parents: [Key::arbitrary(g), Key::arbitrary(g)],
            linknode: Node::arbitrary(g),
        }
    }
}
