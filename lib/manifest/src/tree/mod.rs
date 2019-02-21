// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

mod store;

use self::store::{Store, TestStore};
use crate::{FileMetadata, Manifest};
use failure::{bail, format_err, Fallible};
use lazy_init::Lazy;
use std::collections::BTreeMap;
use std::sync::Arc;
use types::{Node, PathComponentBuf, RepoPath, RepoPathBuf};

/// The Tree implementation of a Manifest dedicates an inner node for each directory in the
/// repository and a leaf for each file.
pub struct Tree<S> {
    store: Arc<S>,
    // TODO: root can't be a Leaf
    root: Link,
}

impl Tree<TestStore> {
    /// Creates a new Tree without any history
    pub fn ephemeral() -> Self {
        Tree {
            store: Arc::new(TestStore::new()),
            root: Link::Ephemeral(BTreeMap::new()),
        }
    }
}

impl<S: Store> Tree<S> {
    /// Instantiates a tree manifest that was stored with the specificed `Node`
    pub fn durable(store: Arc<S>, node: Node) -> Self {
        Tree {
            store,
            root: Link::durable(node),
        }
    }
}

/// `Link` describes the type of nodes that tree manifest operates on.
#[derive(Clone)]
pub enum Link {
    /// `Leaf` nodes store FileMetadata. They are terminal nodes and don't have any other
    /// information.
    Leaf(FileMetadata),
    /// `Ephemeral` nodes are inner nodes that have not been committed to storage. They are only
    /// available in memory. They need to be persisted to be available in future. They are the
    /// mutable type of an inner node. They store the contents of a directory that has been
    /// modified.
    Ephemeral(BTreeMap<PathComponentBuf, Link>),
    /// `Durable` nodes are inner nodes that come from storage. Their contents can be
    /// shared between multiple instances of Tree. They are lazily evaluated. Their children
    /// list will be read from storage only when it is accessed.
    Durable(Arc<DurableEntry>),
}
use self::Link::*;

impl Link {
    pub fn durable(node: Node) -> Link {
        Link::Durable(Arc::new(DurableEntry::new(node)))
    }
}

// TODO: Use Vec instead of BTreeMap
/// The inner structure of a durable link. Of note is that failures are cached "forever".
// The interesting question about this structure is what do we do when we have a failure when
// reading from storage?
// We can cache the failure or we don't cache it. Caching it is mostly fine if we had an error
// reading from local storage or when deserializing. It is not the best option if our storage
// is remote and we hit a network blip. On the other hand we would not want to always retry when
// there is a failure on remote storage, we'd want to have a least an exponential backoff on
// retries. Long story short is that caching the failure is a reasonable place to start from.
pub struct DurableEntry {
    node: Node,
    links: Lazy<Fallible<BTreeMap<PathComponentBuf, Link>>>,
}

impl DurableEntry {
    fn new(node: Node) -> Self {
        DurableEntry {
            node,
            links: Lazy::new(),
        }
    }

    fn get_links<S: Store>(
        &self,
        store: &S,
        path: &RepoPath,
    ) -> Fallible<&BTreeMap<PathComponentBuf, Link>> {
        // TODO: be smarter around how failures are handled when reading from the store
        // Currently this loses the stacktrace
        match self.links.get_or_create(|| store.get(path, &self.node)) {
            Ok(links) => Ok(links),
            Err(error) => Err(format_err!("{}", error)),
        }
    }
}

impl<S: Store> Manifest for Tree<S> {
    fn get(&self, path: &RepoPath) -> Fallible<Option<&FileMetadata>> {
        let mut cursor = &self.root;
        // TODO: parent_path: &RepoPath
        let mut parent_path = RepoPathBuf::new();
        for component in path.components() {
            let child = match cursor {
                Leaf(_) => bail!("Encountered file where a directory was expected."),
                Ephemeral(links) => links.get(component),
                Durable(ref entry) => {
                    let links = entry.get_links(&*self.store, &parent_path)?;
                    links.get(component)
                }
            };
            parent_path.push(component);
            match child {
                None => return Ok(None),
                Some(link) => cursor = link,
            }
        }
        if let Leaf(file_metadata) = cursor {
            Ok(Some(file_metadata))
        } else {
            Err(format_err!("Encountered directory where file was expected"))
        }
    }

    fn insert(&mut self, path: RepoPathBuf, file_metadata: FileMetadata) -> Fallible<()> {
        let mut cursor = &mut self.root;
        // TODO: parent_path: &RepoPath
        let mut parent_path = RepoPathBuf::new();
        for component in path.components() {
            cursor = match cursor {
                Leaf(_) => bail!("Encountered file where a directory was expected."),
                Ephemeral(links) => links
                    .entry(component.to_owned())
                    .or_insert_with(|| Ephemeral(BTreeMap::new())),
                Durable(ref entry) => {
                    let durable_links = entry.get_links(&*self.store, &parent_path)?;
                    *cursor = Ephemeral(durable_links.clone());
                    if let Ephemeral(links) = cursor {
                        links
                            .entry(component.to_owned())
                            .or_insert_with(|| Ephemeral(BTreeMap::new()))
                    } else {
                        unreachable!("Assigned ephemeral disappeared after assignment");
                    }
                }
            };
            parent_path.push(component);
        }
        match cursor {
            Leaf(current_metadata) => {
                *current_metadata = file_metadata;
            }
            Ephemeral(links) => {
                if !links.is_empty() {
                    bail!("Asked to set file metadata on a directory.");
                }
                *cursor = Leaf(file_metadata);
            }
            Durable(_) => {
                bail!("Asked to set file metadata on a directory.");
            }
        }
        Ok(())
    }

    fn remove(&mut self, _path: &RepoPath) -> Fallible<()> {
        // TODO: implement deletion
        unimplemented!("manifest::tree::Tree::remove is not implemented")
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
    fn path_component_buf(s: &str) -> PathComponentBuf {
        PathComponentBuf::from_string(s.to_owned()).unwrap()
    }

    #[test]
    fn test_insert() {
        let mut tree = Tree::ephemeral();
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

    #[test]
    fn test_durable_link() {
        let mut store = TestStore::new();
        let mut root_children = BTreeMap::new();
        root_children.insert(path_component_buf("foo"), Link::durable(Node::from_u8(10)));
        root_children.insert(path_component_buf("baz"), Link::Leaf(meta(20)));
        store
            .insert(repo_path_buf(""), Node::from_u8(1), root_children)
            .unwrap();
        let mut foo_children = BTreeMap::new();
        foo_children.insert(path_component_buf("bar"), Link::Leaf(meta(11)));
        store
            .insert(repo_path_buf("foo"), Node::from_u8(10), foo_children)
            .unwrap();
        let mut tree = Tree::durable(Arc::new(store), Node::from_u8(1));

        assert_eq!(tree.get(repo_path("foo/bar")).unwrap(), Some(&meta(11)));
        assert_eq!(tree.get(repo_path("baz")).unwrap(), Some(&meta(20)));

        tree.insert(repo_path_buf("foo/bat"), meta(12)).unwrap();
        assert_eq!(tree.get(repo_path("foo/bat")).unwrap(), Some(&meta(12)));
        assert_eq!(tree.get(repo_path("foo/bar")).unwrap(), Some(&meta(11)));
        assert_eq!(tree.get(repo_path("baz")).unwrap(), Some(&meta(20)));
    }

    #[test]
    fn test_insert_into_directory() {
        let mut tree = Tree::ephemeral();
        tree.insert(repo_path_buf("foo/bar/baz"), meta(10)).unwrap();
        assert!(tree.insert(repo_path_buf("foo/bar"), meta(20)).is_err());
        assert!(tree.insert(repo_path_buf("foo"), meta(30)).is_err());
    }

    #[test]
    fn test_insert_with_file_parent() {
        let mut tree = Tree::ephemeral();
        tree.insert(repo_path_buf("foo"), meta(10)).unwrap();
        assert!(tree.insert(repo_path_buf("foo/bar"), meta(20)).is_err());
        assert!(tree.insert(repo_path_buf("foo/bar/baz"), meta(30)).is_err());
    }

    #[test]
    fn test_get_from_directory() {
        let mut tree = Tree::ephemeral();
        tree.insert(repo_path_buf("foo/bar/baz"), meta(10)).unwrap();
        assert!(tree.get(repo_path("foo/bar")).is_err());
        assert!(tree.get(repo_path("foo")).is_err());
    }

    #[test]
    fn test_get_with_file_parent() {
        let mut tree = Tree::ephemeral();
        tree.insert(repo_path_buf("foo"), meta(10)).unwrap();
        assert!(tree.get(repo_path("foo/bar")).is_err());
        assert!(tree.get(repo_path("foo/bar/baz")).is_err());
    }
}
