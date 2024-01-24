/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use storemodel::BoxIterator;
use storemodel::TreeEntry;
use storemodel::TreeItemFlag;
use types::HgId;
use types::PathComponent;
use types::PathComponentBuf;

impl TreeEntry for crate::store::Entry {
    fn iter(
        &self,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(PathComponentBuf, HgId, TreeItemFlag)>>> {
        let elements = self.elements();
        let iter = elements
            .map(|fallible_element| fallible_element.map(|e| (e.component, e.hgid, e.flag)));
        Ok(Box::new(iter))
    }

    fn lookup(&self, name: &PathComponent) -> anyhow::Result<Option<(HgId, TreeItemFlag)>> {
        self.elements().lookup(name)
    }
}
