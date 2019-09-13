// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::sync::Arc;

use failure::Fallible;

use types::{testutil::*, Node, RepoPath};

use crate::{
    tree::{
        store::{self, TestStore},
        Directory, File, Link, Tree,
    },
    FileMetadata, Manifest,
};

pub(crate) fn store_element(path: &str, hex: &str, flag: store::Flag) -> Fallible<store::Element> {
    Ok(store::Element::new(
        path_component_buf(path),
        node(hex),
        flag,
    ))
}

pub(crate) fn get_node(tree: &Tree, path: &RepoPath) -> Node {
    match tree.get_link(path).unwrap().unwrap() {
        Link::Leaf(file_metadata) => file_metadata.node,
        Link::Durable(ref entry) => entry.node,
        Link::Ephemeral(_) => panic!("Asked for node on path {} but found ephemeral node.", path),
    }
}

pub(crate) fn make_meta(hex: &str) -> FileMetadata {
    FileMetadata::regular(node(hex))
}

pub(crate) fn make_file(path: &str, hex: &str) -> File {
    File {
        path: repo_path_buf(path),
        meta: make_meta(hex),
    }
}

pub(crate) fn make_dir<'a>(path: &str, hex: Option<&str>, link: &'a Link) -> Directory<'a> {
    Directory {
        path: repo_path_buf(path),
        node: hex.map(node),
        link,
    }
}

pub(crate) fn make_tree<'a>(paths: impl IntoIterator<Item = &'a (&'a str, &'a str)>) -> Tree {
    let mut tree = Tree::ephemeral(Arc::new(TestStore::new()));
    for (path, filenode) in paths {
        tree.insert(repo_path_buf(path), make_meta(filenode))
            .unwrap();
    }
    tree
}
