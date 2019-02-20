// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::FileMetadata;
use crate::Manifest;
use failure::{bail, Fallible};
use std::collections::BTreeMap;
use types::{PathComponent, PathComponentBuf, RepoPath, RepoPathBuf};

/// The Tree implementation of a Manifest dedicates an inner node for each directory in the
/// repository and a leaf for each file.
pub struct Tree {
    // TODO: root can't be a Leaf
    root: Link,
}

impl Tree {
    /// Creates a new Tree without any history
    pub fn new() -> Tree {
        Tree {
            root: Link::ephemeral(),
        }
    }
}

/// `Link` describes the type of nodes that tree manifest operates on.
enum Link {
    /// `Leaf` nodes store FileMetadata. They are terminal nodes and don't have any other
    /// information.
    Leaf(FileMetadata),
    /// `Ephemeral` nodes are inner nodes that have not been committed to storage. They are only
    /// available in memory. They need to be persisted to be available in future. They are the
    /// mutable type of an inner node. They store the contents of a directory that has been
    /// modified.
    Ephemeral(BTreeMap<PathComponentBuf, Link>),
    // TODO: add durable link (reading from storage)
}
use self::Link::*;

impl Link {
    fn leaf(file_metadata: FileMetadata) -> Link {
        Leaf(file_metadata)
    }

    fn ephemeral() -> Link {
        Ephemeral(BTreeMap::new())
    }

    fn child(&self, component: &PathComponent) -> Fallible<Option<&Link>> {
        match self {
            Leaf(_) => bail!("Encountered file where a directory was expected."),
            Ephemeral(links) => Ok(links.get(component)),
        }
    }

    fn child_mut_or_insert(&mut self, component: PathComponentBuf) -> Fallible<&mut Link> {
        match self {
            Leaf(_) => bail!("Encountered file where a directory was expected."),
            Ephemeral(links) => Ok(links.entry(component).or_insert(Link::ephemeral())),
        }
    }

    fn set_metadata(&mut self, file_metadata: FileMetadata) -> Fallible<()> {
        match self {
            Leaf(content) => {
                *content = file_metadata;
                Ok(())
            }
            Ephemeral(links) => {
                if !links.is_empty() {
                    bail!("Asked to set file metadata on a directory.");
                }
                *self = Link::leaf(file_metadata);
                Ok(())
            }
        }
    }

    fn file_metadata(&self) -> Fallible<&FileMetadata> {
        match self {
            Leaf(file_metadata) => Ok(file_metadata),
            Ephemeral(_) => bail!("Encountered directory where file was expected"),
        }
    }
}

impl Manifest for Tree {
    fn get(&self, path: &RepoPath) -> Fallible<Option<&FileMetadata>> {
        let mut cursor = &self.root;
        for component in path.components() {
            match cursor.child(component)? {
                None => return Ok(None),
                Some(link) => cursor = link,
            }
        }
        Ok(Some(cursor.file_metadata()?))
    }

    fn insert(&mut self, path: RepoPathBuf, file_metadata: FileMetadata) -> Fallible<()> {
        let mut cursor = &mut self.root;
        for component in path.components() {
            cursor = cursor.child_mut_or_insert(component.to_owned())?;
        }
        cursor.set_metadata(file_metadata)
    }

    fn remove(&mut self, _path: &RepoPath) -> Fallible<()> {
        // TODO: implement deletion
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use types::node::Node;

    fn meta(node: u8) -> FileMetadata {
        FileMetadata::regular(Node::from_u8(node))
    }
    fn repo_path(s: &str) -> &RepoPath {
        RepoPath::from_str(s).unwrap()
    }
    fn repo_path_buf(s: &str) -> RepoPathBuf {
        RepoPathBuf::from_string(s.to_owned()).unwrap()
    }

    #[test]
    fn test_insert() {
        let mut tree = Tree::new();
        tree.insert(repo_path_buf("foo/bar"), meta(10)).unwrap();
        assert_eq!(tree.get(repo_path("foo/bar")).unwrap(), Some(&meta(10)));
        assert_eq!(tree.get(repo_path("baz")).unwrap(), None);

        tree.insert(repo_path_buf("baz"), meta(20)).unwrap();
        assert_eq!(tree.get(repo_path("foo/bar")).unwrap(), Some(&meta(10)));
        assert_eq!(tree.get(repo_path("baz")).unwrap(), Some(&meta(20)));

        tree.insert(repo_path_buf("foo/bat"), meta(30)).unwrap();
        assert_eq!(tree.get(repo_path("foo/bat")).unwrap(), Some(&meta(30)));
        assert_eq!(tree.get(repo_path("foo/bar")).unwrap(), Some(&meta(10)));
        assert_eq!(tree.get(repo_path("baz")).unwrap(), Some(&meta(20)));
    }
}
