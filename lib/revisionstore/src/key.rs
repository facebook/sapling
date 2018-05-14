use node::Node;

pub struct Key {
    // Name is usually a file or directory path
    name: Box<[u8]>,
    // Node is always a 20 byte hash. This will be changed to a fix length array later.
    node: Node,
}

impl Key {
    pub fn new(name: Box<[u8]>, node: Node) -> Self {
        Key { name, node }
    }

    pub fn name(&self) -> &[u8] {
        &self.name
    }

    pub fn node(&self) -> &Node {
        &self.node
    }
}
