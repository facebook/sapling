// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt;
use std::sync::Arc;

use futures::{stream, Future, Stream};

use mercurial_types::{Entry, Manifest, Type};
use mercurial_types::manifest::Content;
use mercurial_types::manifest_utils::recursive_entry_stream;
use mononoke_types::path::{MPath, MPathElement, DOT, DOTDOT};

use node::{VfsDir, VfsFile, VfsNode};
use tree::{TNodeId, Tree, TreeValue, ROOT_ID};

use errors::*;

const INCONCISTENCY: &str = "Internal inconsitency in Tree detected, a nodeid is missing";

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
struct TEntryId(usize);

/// For a given Manifest return a VfsDir representing the root of the file system defined by it
pub fn vfs_from_manifest<M>(
    manifest: &M,
) -> impl Future<Item = ManifestVfsDir, Error = Error> + Send + 'static
where
    M: Manifest,
{
    let entry_streams = manifest
        .list()
        .map(|entry| recursive_entry_stream(None, entry));
    stream::iter_ok::<_, Error>(entry_streams)
        .flatten()
        .filter(|pathentry| pathentry.1.get_type() != Type::Tree)
        .collect()
        .and_then(|pathentries| {
            let mut path_tree = Tree::new();
            let mut entries = vec![];
            for (entry_idx, (path, entry)) in pathentries.into_iter().enumerate() {
                let name = entry.get_name().cloned();
                let name = name.ok_or_else(|| {
                    ErrorKind::ManifestInvalidPath("name shouldn't be empty".into())
                })?;
                path_tree.insert(MPath::into_iter_opt(path), name, TEntryId(entry_idx))?;
                entries.push(entry);
            }
            Ok(ManifestVfsDir {
                root: Arc::new(ManifestVfsRoot { entries, path_tree }),
                nodeid: ROOT_ID,
            })
        })
}

struct ManifestVfsRoot {
    entries: Vec<Box<Entry + Sync>>,
    path_tree: Tree<MPathElement, TEntryId>,
}

impl ManifestVfsRoot {
    fn get_node(this: &Arc<Self>, nodeid: TNodeId) -> VfsNode<ManifestVfsDir, ManifestVfsFile> {
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

impl fmt::Debug for ManifestVfsRoot {
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
pub struct ManifestVfsDir {
    root: Arc<ManifestVfsRoot>,
    nodeid: TNodeId,
}

impl Clone for ManifestVfsDir {
    fn clone(&self) -> Self {
        ManifestVfsDir {
            root: self.root.clone(),
            nodeid: self.nodeid,
        }
    }
}

impl VfsDir for ManifestVfsDir {
    type TFile = ManifestVfsFile;

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
pub struct ManifestVfsFile {
    root: Arc<ManifestVfsRoot>,
    nodeid: TNodeId,
}

impl Clone for ManifestVfsFile {
    fn clone(&self) -> Self {
        ManifestVfsFile {
            root: self.root.clone(),
            nodeid: self.nodeid,
        }
    }
}

impl VfsFile for ManifestVfsFile {
    type TDir = ManifestVfsDir;

    fn read(&self) -> Box<Future<Item = Content, Error = Error> + Send> {
        let &TEntryId(entryid) = self.root
            .path_tree
            .get_value(self.nodeid)
            .expect(INCONCISTENCY)
            .get_leaf()
            .expect("Expected a leaf, not an internal node");
        self.root
            .entries
            .get(entryid)
            .expect("HgEntryId not found in entries list")
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

    use mercurial_types::{FileType, MPath};
    use mercurial_types_mocks::manifest::MockManifest;
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
        let manifest = MockManifest::empty();
        let vfs = vfs_from_manifest(&manifest)
            .wait()
            .expect("failed to get vfs");
        assert!(vfs.read().is_empty());

        match vfs.step(&pel("a")) {
            None => (),
            Some(_) => panic!("Expected the Vfs to be empty"),
        }
    }

    fn example_vfs() -> ManifestVfsDir {
        let paths = btreemap! {
            "a/b" => (FileType::Regular, ""),
            "a/ab" => (FileType::Regular, ""),
            "c/d/e" => (FileType::Regular, ""),
            "c/d/da" => (FileType::Regular, ""),
            "c/ca/afsd" => (FileType::Regular, ""),
            "f" => (FileType::Regular, ""),
        };
        let root_manifest = MockManifest::from_paths(paths).expect("invalid manifest?");

        vfs_from_manifest(&root_manifest)
            .wait()
            .expect("failed to get vfs")
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
