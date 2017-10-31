// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt;
use std::sync::Arc;

use futures::{Future, Stream};

use error_chain::ChainedError;

use mercurial_types::{Entry, Manifest};
use mercurial_types::manifest::Content;
use mercurial_types::path::{MPathElement, DOT, DOTDOT};

use node::{VfsDir, VfsFile, VfsNode};
use tree::{TNodeId, Tree, TreeValue, ROOT_ID};

use errors::*;

const INCONCISTENCY: &str = "Internal inconsitency in Tree detected, a nodeid is missing";

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
struct TEntryId(usize);

/// For a given Manifest return a VfsDir representing the root of the file system defined by it
pub fn vfs_from_manifest<M, E>(
    manifest: &M,
) -> impl Future<Item = ManifestVfsDir<E>, Error = Error> + Send + 'static
where
    M: Manifest<Error = E>,
    E: Send + 'static + ::std::error::Error,
{
    manifest
        .list()
        .map_err(|err| {
            ChainedError::with_chain(err, "failed while listing the manifest")
        })
        .collect()
        .and_then(|entries| {
            let mut path_tree = Tree::new();
            for (entry_idx, entry) in entries.iter().enumerate() {
                let mut path = entry.get_mpath().clone().into_iter();
                let leaf_key = path.next_back().ok_or_else(|| {
                    ErrorKind::ManifestInvalidPath("the path shouldn't be empty".into())
                })?;

                path_tree.insert(path, leaf_key, TEntryId(entry_idx))?;
            }
            Ok(ManifestVfsDir {
                root: Arc::new(ManifestVfsRoot { entries, path_tree }),
                nodeid: ROOT_ID,
            })
        })
}

struct ManifestVfsRoot<E: Send + 'static + ::std::error::Error> {
    entries: Vec<Box<Entry<Error = E> + Sync>>,
    path_tree: Tree<MPathElement, TEntryId>,
}

impl<E: Send + 'static + ::std::error::Error> ManifestVfsRoot<E> {
    fn get_node(
        this: &Arc<Self>,
        nodeid: TNodeId,
    ) -> VfsNode<ManifestVfsDir<E>, ManifestVfsFile<E>> {
        match this.path_tree.get_value(nodeid).expect(INCONCISTENCY) {
            &TreeValue::Leaf(_) => VfsNode::File(ManifestVfsFile {
                root: this.clone(),
                nodeid,
            }),
            &TreeValue::Node(_) => VfsNode::Dir(ManifestVfsDir {
                root: this.clone(),
                nodeid,
            }),
        }
    }
}

impl<E: Send + 'static + ::std::error::Error> fmt::Debug for ManifestVfsRoot<E> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "ManifestVfsRoot {{ entries: (There are {} entries), path_tree: {:?}}}",
            self.entries.len(),
            self.path_tree
        )
    }
}

/// Structure implementing the VfsDir interface that represents a dir within a Manifest Vfs
#[derive(Debug)]
pub struct ManifestVfsDir<E: Send + 'static + ::std::error::Error> {
    root: Arc<ManifestVfsRoot<E>>,
    nodeid: TNodeId,
}

impl<E: Send + 'static + ::std::error::Error> Clone for ManifestVfsDir<E> {
    fn clone(&self) -> Self {
        ManifestVfsDir {
            root: self.root.clone(),
            nodeid: self.nodeid,
        }
    }
}

impl<E: Send + 'static + ::std::error::Error> VfsDir for ManifestVfsDir<E> {
    type TFile = ManifestVfsFile<E>;

    fn read(&self) -> Vec<&MPathElement> {
        self.root
            .path_tree
            .get_value(self.nodeid)
            .expect(INCONCISTENCY)
            .get_node()
            .expect("Expected an internal node, not a leaf")
            .keys()
            .collect()
    }

    fn step(&self, path: &MPathElement) -> Option<VfsNode<Self, Self::TFile>> {
        if path == &*DOT {
            return Some(VfsNode::Dir(self.clone()));
        }

        let tree = &self.root.path_tree;
        let nodeid = if path == &*DOTDOT {
            tree.get_parent(self.nodeid)
        } else {
            tree.get_child(self.nodeid, path)
        };
        nodeid.map(|nodeid| ManifestVfsRoot::get_node(&self.root, nodeid))
    }
}

/// Structure implementing the VfsFile interface that represents a file within a Manifest Vfs
#[derive(Debug)]
pub struct ManifestVfsFile<E: Send + 'static + ::std::error::Error> {
    root: Arc<ManifestVfsRoot<E>>,
    nodeid: TNodeId,
}

impl<E: Send + 'static + ::std::error::Error> Clone for ManifestVfsFile<E> {
    fn clone(&self) -> Self {
        ManifestVfsFile {
            root: self.root.clone(),
            nodeid: self.nodeid,
        }
    }
}

impl<E: Send + 'static + ::std::error::Error> VfsFile for ManifestVfsFile<E> {
    type TDir = ManifestVfsDir<E>;
    type Error = E;

    fn read(&self) -> Box<Future<Item = Content<E>, Error = E> + Send> {
        let &TEntryId(entryid) = self.root
            .path_tree
            .get_value(self.nodeid)
            .expect(INCONCISTENCY)
            .get_leaf()
            .expect("Expected a leaf, not an internal node");
        self.root
            .entries
            .get(entryid)
            .expect("EntryId not found in entries list")
            .get_content()
    }

    fn parent_dir(&self) -> Self::TDir {
        let parentid = self.root
            .path_tree
            .get_parent(self.nodeid)
            .expect("No parent node found for a file");
        match ManifestVfsRoot::get_node(&self.root, parentid) {
            VfsNode::Dir(vfs) => vfs,
            _ => panic!("Parent of a file is not a dir"),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use test::*;

    use mercurial_types::MPath;
    use node::VfsWalker;

    fn unwrap_dir<TDir: VfsDir>(node: VfsNode<TDir, TDir::TFile>) -> TDir {
        match node {
            VfsNode::Dir(dir) => dir,
            _ => panic!("Expected dir, found file"),
        }
    }

    fn unwrap_file<TFile: VfsFile>(node: VfsNode<TFile::TDir, TFile>) -> TFile {
        match node {
            VfsNode::File(file) => file,
            _ => panic!("Expected file, found dir"),
        }
    }

    #[test]
    fn test_empty_vfs() {
        let vfs = get_vfs::<Error>(Vec::<&str>::new());
        assert!(vfs.read().is_empty());

        match vfs.step(&pel("a")) {
            None => (),
            Some(_) => panic!("Expected the Vfs to be empty"),
        }
    }

    fn example_vfs() -> ManifestVfsDir<Error> {
        get_vfs::<Error>(vec!["a/b", "a/ab", "c/d/e", "c/d/da", "c/ca/afsd", "f"])
    }

    #[test]
    fn test_dir() {
        let vfs = example_vfs();
        cmp(vfs.read(), vec!["a", "c", "f"]);

        let dir_a = unwrap_dir(vfs.step(&pel("a")).unwrap());
        cmp(dir_a.read(), vec!["ab", "b"]);

        // Check that dir_c_d can outlive dir_c
        let dir_c_d = {
            let dir_c = unwrap_dir(vfs.step(&pel("c")).unwrap());
            cmp(dir_c.read(), vec!["ca", "d"]);
            unwrap_dir(dir_c.step(&pel("d")).unwrap())
        };

        cmp(dir_c_d.read(), vec!["da", "e"]);

        unwrap_file(dir_c_d.step(&pel("da")).unwrap());
    }

    #[test]
    #[should_panic]
    fn test_file() {
        unimplemented!() // TODO(luk, T20453159): implement this
    }

    #[test]
    fn test_walk() {
        let vfs = VfsNode::Dir(example_vfs());
        let dir_c_d = unwrap_dir(
            VfsWalker::new(vfs.clone(), MPath::new("c/d").unwrap())
                .walk()
                .wait()
                .unwrap(),
        );

        cmp(dir_c_d.read(), vec!["da", "e"]);

        unwrap_file(
            VfsWalker::new(vfs.clone(), MPath::new("c/d/da").unwrap())
                .walk()
                .wait()
                .unwrap(),
        );

        assert!(
            VfsWalker::new(vfs.clone(), MPath::new("z").unwrap())
                .walk()
                .wait()
                .is_err()
        );
    }
}
