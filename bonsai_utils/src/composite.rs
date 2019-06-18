// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::{btree_map, BTreeMap, HashMap};

use crate::failure::Error;
use futures::{future, stream, Future, Stream};

use context::CoreContext;
use mercurial_types::{manifest::Content, Entry, HgEntryId, HgFileNodeId, HgManifestId};
use mononoke_types::{FileType, MPathElement};

/// An entry representing composite state formed by multiple parents.
pub struct CompositeEntry {
    files: HashMap<(FileType, HgFileNodeId), Box<dyn Entry + Sync>>,
    trees: HashMap<HgManifestId, Box<dyn Entry + Sync>>,
}

impl CompositeEntry {
    #[inline]
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
            trees: HashMap::new(),
        }
    }

    #[inline]
    pub fn add_parent(&mut self, entry: Box<dyn Entry + Sync>) {
        match entry.get_hash() {
            HgEntryId::Manifest(mf) => self.trees.insert(mf, entry),
            HgEntryId::File(ft, h) => self.files.insert((ft, h), entry),
        };
    }

    #[inline]
    pub fn num_files(&self) -> usize {
        self.files.len()
    }

    #[inline]
    pub fn contains_file(&self, file_type: &FileType, hash: HgFileNodeId) -> bool {
        self.files.contains_key(&(*file_type, hash))
    }

    /// Whether this composite entry contains the same hash but a different type.
    #[inline]
    pub fn contains_file_other_type(&self, file_type: &FileType, hash: HgFileNodeId) -> bool {
        file_type
            .complement()
            .iter()
            .any(|ft| self.contains_file(ft, hash))
    }

    /// Whether this composite entry contains a file with this hash but with any possible type.
    #[inline]
    pub fn contains_file_any_type(&self, hash: HgFileNodeId) -> bool {
        FileType::all()
            .iter()
            .any(|ft| self.contains_file(ft, hash))
    }

    #[inline]
    pub fn num_trees(&self) -> usize {
        self.trees.len()
    }

    #[inline]
    pub fn contains_tree(&self, hash: HgManifestId) -> bool {
        self.trees.contains_key(&hash)
    }

    pub fn manifest(
        &self,
        ctx: CoreContext,
    ) -> impl Future<Item = CompositeManifest, Error = Error> + Send {
        // Manifests can only exist for tree entries. If self.trees is empty then an empty
        // composite manifest will be returned. This is by design.
        let mf_futs = self.trees.values().map(|entry| {
            entry.get_content(ctx.clone()).map({
                move |content| match content {
                    Content::Tree(mf) => mf,
                    _other => unreachable!("tree content must be a manifest"),
                }
            })
        });
        stream::futures_unordered(mf_futs).fold(CompositeManifest::new(), |mut composite_mf, mf| {
            for entry in mf.list() {
                composite_mf.add(entry);
            }
            future::ok::<_, Error>(composite_mf)
        })
    }
}

/// Represents a manifest formed from the state of multiple changesets. `CompositeManifest` and
/// `CompositeEntry` work in tandem to provide a way to lazily iterate over multiple parents.
pub struct CompositeManifest {
    entries: BTreeMap<MPathElement, CompositeEntry>,
}

impl CompositeManifest {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    pub fn add(&mut self, entry: Box<dyn Entry + Sync>) {
        self.entries
            .entry(entry.get_name().expect("entry cannot be root").clone())
            .or_insert_with(|| CompositeEntry::new())
            .add_parent(entry)
    }
}

impl IntoIterator for CompositeManifest {
    type Item = (MPathElement, CompositeEntry);
    type IntoIter = btree_map::IntoIter<MPathElement, CompositeEntry>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}
