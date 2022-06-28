/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::manifest::Entry;
use ::manifest::Manifest;
use mononoke_types::MPathElement;

use crate::BlobHandle;
use crate::Tree;
use crate::TreeHandle;
use crate::Treeish;

impl Manifest for Tree {
    type TreeId = TreeHandle;
    type LeafId = BlobHandle;

    fn list(&self) -> Box<dyn Iterator<Item = (MPathElement, Entry<Self::TreeId, Self::LeafId>)>> {
        let members: Vec<_> = self
            .members()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone().into()))
            .collect();

        Box::new(members.into_iter())
    }

    fn lookup(&self, name: &MPathElement) -> Option<Entry<Self::TreeId, Self::LeafId>> {
        self.members().get(name).map(|e| e.clone().into())
    }
}
