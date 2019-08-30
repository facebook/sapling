// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{str::from_utf8, sync::Arc};

use bytes::{Bytes, BytesMut};
use failure::{format_err, Fallible};

use types::{Node, PathComponent, PathComponentBuf, RepoPath};

use crate::FileType;

/// The `TreeStore` is an abstraction layer for the tree manifest that decouples how or where the
/// data is stored. This allows more easy iteration on serialization format. It also simplifies
/// writing storage migration.
pub trait TreeStore {
    fn get(&self, path: &RepoPath, node: Node) -> Fallible<Bytes>;

    fn insert(&self, path: &RepoPath, node: Node, data: Bytes) -> Fallible<()>;
}

#[derive(Clone)]
pub struct InnerStore {
    tree_store: Arc<dyn TreeStore + Send + Sync>,
}

impl InnerStore {
    pub fn new(tree_store: Arc<dyn TreeStore + Send + Sync>) -> Self {
        InnerStore { tree_store }
    }

    pub fn get_entry(&self, path: &RepoPath, node: Node) -> Fallible<Entry> {
        let bytes = self.tree_store.get(path, node)?;
        Ok(Entry(bytes))
    }

    pub fn insert_entry(&self, path: &RepoPath, node: Node, entry: Entry) -> Fallible<()> {
        self.tree_store.insert(path, node, entry.0)
    }
}

/// The `Entry` is the data that is stored on disk. It should be seen as opaque to whether it
/// represents serialized or deserialized data. It is the object that performs the bytes to
/// business object transformations. It provides method for interacting with parsed data.
///
/// The ABNF specification for the current serialization is:
/// Entry         = 1*( Element LF )
/// Element       = PathComponent %x00 Node [ Flag ]
/// Flag          = %s"x" / %s"l" / %s"t"
/// PathComponent = 1*( %x01-%x09 / %x0B-%xFF )
/// Node          = 40HEXDIG
///
/// In this case an `Entry` is equivalent to the contents of a directory. The elements of the
/// directory are described by `Element`. `Entry` is a list of serialized `Element`s that are
/// separated by `\n`. An `Element` will always have a name (`PathComponent`) and a hash (`Node`).
/// `Elements` may be different types of files or they can be directories. The type of element is
/// described by the flag or the absence of the flag. When the flag is missing we have a regular
/// file, the various flag options are: `x` for executable, `l` for symlink and `d` for directory.
/// It should be noted that Nodes are represented in their hex format rather than a straight up
/// binary format so they are 40 characters long rather than 20 bytes.
/// Check the documentation of the `PathComponent` struct for more details about it's
/// representation. For this serialization format it is important that they don't contain
/// `\0` or `\n`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Entry(Bytes);

pub struct EntryMut(BytesMut);

/// The `Element` is a parsed element of a directory. Directory elements are either files either
/// direcotries. The type of element is signaled by `Flag`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Element {
    pub component: PathComponentBuf,
    pub node: Node,
    pub flag: Flag,
}

/// Used to signal the type of element in a directory: file or directory.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Flag {
    File(FileType),
    Directory,
}

impl Entry {
    /// Returns an iterator over the elements that the current `Entry` contains. This is the
    /// primary method of inspection for an `Entry`.
    pub fn elements<'a>(&'a self) -> Elements<'a> {
        Elements {
            byte_slice: &self.0,
            position: 0,
        }
    }

    /// The primary builder of an Entry, from a list of `Element`.
    pub fn from_elements<I: IntoIterator<Item = Fallible<Element>>>(
        elements: I,
    ) -> Fallible<Entry> {
        let mut underlying = BytesMut::new();
        for element_result in elements.into_iter() {
            underlying.extend(element_result?.to_byte_vec());
            underlying.extend(b"\n");
        }
        Ok(Entry(underlying.freeze()))
    }

    // used in tests, finalize and subtree_diff
    pub fn to_bytes(self) -> Bytes {
        self.0
    }
}

impl EntryMut {
    /// Constructs an empty `Entry`. It is not valid to save an empty `Entry`.
    pub fn new() -> Self {
        EntryMut(BytesMut::new())
    }

    /// Adds an element to the list of elements represented by this `Entry`.
    /// It is expected that elements are added sorted by paths.
    pub fn add_element(&mut self, element: Element) {
        self.0.extend(element.to_byte_vec());
        self.0.extend(b"\n");
    }

    pub fn freeze(self) -> Entry {
        Entry(self.0.freeze())
    }
}

impl AsRef<[u8]> for Entry {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

pub struct Elements<'a> {
    byte_slice: &'a [u8],
    position: usize,
}

impl<'a> Iterator for Elements<'a> {
    type Item = Fallible<Element>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.position >= self.byte_slice.len() {
            return None;
        }
        let end = match self.byte_slice[self.position..]
            .iter()
            .position(|&byte| byte == b'\n')
        {
            None => {
                return Some(Err(format_err!(
                    "failed to deserialize tree manifest entry, missing line feed\n{:?}",
                    String::from_utf8_lossy(self.byte_slice)
                )));
            }
            Some(delta) => self.position + delta,
        };
        let result = Element::from_byte_slice(&self.byte_slice[self.position..end]);
        self.position = end + 1;
        Some(result)
    }
}

impl Element {
    pub fn new(component: PathComponentBuf, node: Node, flag: Flag) -> Element {
        Element {
            component,
            node,
            flag,
        }
    }

    fn from_byte_slice(byte_slice: &[u8]) -> Fallible<Element> {
        let path_len = match byte_slice.iter().position(|&x| x == b'\0') {
            Some(position) => position,
            None => return Err(format_err!("did not find path delimiter")),
        };
        let component = PathComponent::from_utf8(&byte_slice[..path_len])?.to_owned();
        if path_len + Node::hex_len() > byte_slice.len() {
            return Err(format_err!("node length is shorter than expected"));
        }
        if byte_slice.len() > path_len + Node::hex_len() + 2 {
            return Err(format_err!("entry longer than expected"));
        }
        // TODO: We don't need this conversion to string
        let utf8_parsed = from_utf8(&byte_slice[path_len + 1..path_len + Node::hex_len() + 1])?;
        let node = Node::from_str(utf8_parsed)?;
        let flag = match byte_slice.get(path_len + Node::hex_len() + 1) {
            None => Flag::File(FileType::Regular),
            Some(b'x') => Flag::File(FileType::Executable),
            Some(b'l') => Flag::File(FileType::Symlink),
            Some(b't') => Flag::Directory,
            Some(bad_flag) => return Err(format_err!("invalid flag {}", bad_flag)),
        };
        let element = Element {
            component,
            node,
            flag,
        };
        Ok(element)
    }

    fn to_byte_vec(&self) -> Vec<u8> {
        let component = self.component.as_byte_slice();
        // TODO: benchmark taking a buffer as a parameter
        // We may not use the last byte but it doesn't hurt to allocate
        let mut buffer = Vec::with_capacity(component.len() + Node::hex_len() + 2);
        buffer.extend_from_slice(component);
        buffer.push(0);
        buffer.extend_from_slice(self.node.to_hex().as_ref());
        let flag = match self.flag {
            Flag::File(FileType::Regular) => None,
            Flag::File(FileType::Executable) => Some(b'x'),
            Flag::File(FileType::Symlink) => Some(b'l'),
            Flag::Directory => Some(b't'),
        };
        if let Some(byte) = flag {
            buffer.push(byte);
        }
        buffer
    }
}

#[cfg(test)]
use std::{collections::HashMap, sync::RwLock};
#[cfg(test)]
use types::RepoPathBuf;

#[cfg(test)]
/// An in memory `Store` implementation backed by HashMaps. Primarily intended for tests.
pub struct TestStore(RwLock<HashMap<RepoPathBuf, HashMap<Node, Bytes>>>);

#[cfg(test)]
impl TestStore {
    pub fn new() -> Self {
        TestStore(RwLock::new(HashMap::new()))
    }
}

#[cfg(test)]
impl TreeStore for TestStore {
    fn get(&self, path: &RepoPath, node: Node) -> Fallible<Bytes> {
        let underlying = self
            .0
            .read()
            .map_err(|err| format_err!("Failed to acquire read lock: {}", err))?;
        let result = underlying
            .get(path)
            .and_then(|node_hash| node_hash.get(&node))
            .map(|entry| entry.clone());
        result.ok_or_else(|| format_err!("Could not find manifest entry for ({}, {})", path, node))
    }

    fn insert(&self, path: &RepoPath, node: Node, data: Bytes) -> Fallible<()> {
        let mut underlying = self
            .0
            .write()
            .map_err(|err| format_err!("Failed to acquire the write lock: {}", err))?;
        underlying
            .entry(path.to_owned())
            .or_insert(HashMap::new())
            .insert(node, data);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use quickcheck::quickcheck;

    use types::testutil::*;

    #[test]
    fn test_element_from_byte_slice() {
        let mut buffer = vec![];
        let path = PathComponent::from_str("foo").unwrap();
        let node = node("123");
        assert!(Element::from_byte_slice(&buffer).is_err());
        buffer.extend_from_slice(path.as_byte_slice());
        assert!(Element::from_byte_slice(&buffer).is_err());
        buffer.push(b'\0');
        assert!(Element::from_byte_slice(&buffer).is_err());
        buffer.extend_from_slice(node.to_hex().as_ref());
        assert_eq!(
            Element::from_byte_slice(&buffer).unwrap(),
            Element::new(path.to_owned(), node, Flag::File(FileType::Regular))
        );

        buffer.push(b'x');
        assert_eq!(
            Element::from_byte_slice(&buffer).unwrap(),
            Element::new(path.to_owned(), node, Flag::File(FileType::Executable))
        );

        *buffer.last_mut().unwrap() = b'l';
        assert_eq!(
            Element::from_byte_slice(&buffer).unwrap(),
            Element::new(path.to_owned(), node, Flag::File(FileType::Symlink))
        );

        *buffer.last_mut().unwrap() = b't';
        assert_eq!(
            Element::from_byte_slice(&buffer).unwrap(),
            Element::new(path.to_owned(), node, Flag::Directory)
        );

        *buffer.last_mut().unwrap() = b's';
        assert!(Element::from_byte_slice(&buffer).is_err());

        *buffer.last_mut().unwrap() = b'x';
        buffer.push(b'\0');
        assert!(Element::from_byte_slice(&buffer).is_err());
    }

    #[test]
    fn test_roundtrip_serialization_on_directory() {
        let component = PathComponentBuf::from_string(String::from("c")).unwrap();
        let node = Node::from_str("2e31d52f551e445002a6e6690700ce2ac31f196e").unwrap();
        let flag = Flag::Directory;
        let byte_slice = b"c\02e31d52f551e445002a6e6690700ce2ac31f196et";
        let element = Element::new(component, node, flag);
        assert_eq!(Element::from_byte_slice(byte_slice).unwrap(), element);
        let buffer = element.to_byte_vec();
        assert_eq!(buffer.to_vec(), byte_slice.to_vec());
    }

    quickcheck! {
        fn test_rountrip_serialization(
            component: PathComponentBuf,
            node: Node,
            flag_proxy: Option<FileType>
        ) -> bool {
            let flag = match flag_proxy {
                Some(file_type) => Flag::File(file_type),
                None => Flag::Directory,
            };
            let element = Element::new(component, node, flag);
            let buffer = element.to_byte_vec();
            Element::from_byte_slice(&buffer).unwrap() == element
        }
    }
}
