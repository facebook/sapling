/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use failure::Fallible;

use types::{testutil::*, HgId, RepoPath};

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
        hgid(hex),
        flag,
    ))
}

pub(crate) fn get_hgid(tree: &Tree, path: &RepoPath) -> HgId {
    match tree.get_link(path).unwrap().unwrap() {
        Link::Leaf(file_metadata) => file_metadata.hgid,
        Link::Durable(ref entry) => entry.hgid,
        Link::Ephemeral(_) => panic!("Asked for hgid on path {} but found ephemeral hgid.", path),
    }
}

pub(crate) fn make_meta(hex: &str) -> FileMetadata {
    FileMetadata::regular(hgid(hex))
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
        hgid: hex.map(hgid),
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
