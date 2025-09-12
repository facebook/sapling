/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use storemodel::BoxIterator;
use storemodel::BoxRefIterator;
use storemodel::TreeEntry;
use storemodel::TreeItemFlag;
use types::HgId;
use types::PathComponent;
use types::PathComponentBuf;
use types::SerializationFormat;

impl TreeEntry for crate::store::Entry {
    fn iter<'a>(
        &'a self,
    ) -> anyhow::Result<BoxRefIterator<anyhow::Result<(&'a PathComponent, HgId, TreeItemFlag)>>>
    {
        let elements = self.elements_ref();
        let iter = elements
            .map(move |fallible_element| fallible_element.map(|e| (e.component, e.hgid, e.flag)));
        Ok(Box::new(iter))
    }

    fn iter_owned(
        &self,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(PathComponentBuf, HgId, TreeItemFlag)>>> {
        let elements = self.elements();
        let iter = elements
            .map(move |fallible_element| fallible_element.map(|e| (e.component, e.hgid, e.flag)));
        Ok(Box::new(iter))
    }

    fn lookup(&self, name: &PathComponent) -> anyhow::Result<Option<(HgId, TreeItemFlag)>> {
        self.elements().lookup(name)
    }

    fn size_hint(&self) -> Option<usize> {
        match self.1 {
            // Hg format has no binary data, so we can just count the newlines.
            SerializationFormat::Hg => Some(bytecount::count(self.0.as_ref(), b'\n')),
            // Git format has binary data - slightly more work. Skip hint for now.
            SerializationFormat::Git => None,
        }
    }
}
