/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::fsnode::Fsnode;
use mononoke_types::fsnode::FsnodeEntry;
use mononoke_types::fsnode::FsnodeFile;
use mononoke_types::FsnodeId;
use mononoke_types::MPathElement;

use super::Entry;
use super::Manifest;
use super::OrderedManifest;
use super::Weight;

impl Manifest for Fsnode {
    type TreeId = FsnodeId;
    type LeafId = FsnodeFile;

    fn lookup(&self, name: &MPathElement) -> Option<Entry<Self::TreeId, Self::LeafId>> {
        self.lookup(name).map(convert_fsnode)
    }

    fn list(&self) -> Box<dyn Iterator<Item = (MPathElement, Entry<Self::TreeId, Self::LeafId>)>> {
        let v: Vec<_> = self
            .list()
            .map(|(basename, entry)| (basename.clone(), convert_fsnode(entry)))
            .collect();
        Box::new(v.into_iter())
    }
}

fn convert_fsnode(fsnode_entry: &FsnodeEntry) -> Entry<FsnodeId, FsnodeFile> {
    match fsnode_entry {
        FsnodeEntry::File(fsnode_file) => Entry::Leaf(*fsnode_file),
        FsnodeEntry::Directory(fsnode_directory) => Entry::Tree(fsnode_directory.id().clone()),
    }
}

impl OrderedManifest for Fsnode {
    fn lookup_weighted(
        &self,
        name: &MPathElement,
    ) -> Option<Entry<(Weight, <Self as Manifest>::TreeId), <Self as Manifest>::LeafId>> {
        self.lookup(name).map(convert_fsnode_weighted)
    }

    fn list_weighted(
        &self,
    ) -> Box<
        dyn Iterator<
            Item = (
                MPathElement,
                Entry<(Weight, <Self as Manifest>::TreeId), <Self as Manifest>::LeafId>,
            ),
        >,
    > {
        let v: Vec<_> = self
            .list()
            .map(|(basename, entry)| (basename.clone(), convert_fsnode_weighted(entry)))
            .collect();
        Box::new(v.into_iter())
    }
}

fn convert_fsnode_weighted(fsnode_entry: &FsnodeEntry) -> Entry<(Weight, FsnodeId), FsnodeFile> {
    match fsnode_entry {
        FsnodeEntry::File(fsnode_file) => Entry::Leaf(*fsnode_file),
        FsnodeEntry::Directory(fsnode_directory) => {
            let summary = fsnode_directory.summary();
            // Fsnodes don't have a full descendant dirs count, so we use the
            // child count as a lower-bound estimate.
            let weight = summary.descendant_files_count + summary.child_dirs_count;
            Entry::Tree((weight as Weight, fsnode_directory.id().clone()))
        }
    }
}
