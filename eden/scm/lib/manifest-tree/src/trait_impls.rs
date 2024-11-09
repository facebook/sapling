/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
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
        let entry = self.clone();
        let elements = entry.elements();
        let iter = elements
            .map(move |fallible_element| fallible_element.map(|e| (e.component, e.hgid, e.flag)));
        Ok(Box::new(iter))
    }

    fn lookup(&self, name: &PathComponent) -> anyhow::Result<Option<(HgId, TreeItemFlag)>> {
        self.elements().lookup(name)
    }
}
