/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;

use edenapi_types::TreeEntry as EdenApiTreeEntry;
use minibytes::Bytes;
use types::{Key, Parents};

use crate::{datastore::Metadata, indexedlogdatastore::Entry};

#[derive(Clone, Debug)]
pub struct StoreTree {
    key: Option<Key>,
    #[allow(dead_code)]
    parents: Option<Parents>,
    raw_content: Option<Bytes>,
    #[allow(dead_code)]
    entry_metadata: Option<Metadata>,
}

impl TryFrom<Entry> for StoreTree {
    type Error = Error;

    fn try_from(mut v: Entry) -> Result<Self, Self::Error> {
        let raw_content = v.content()?;
        let key = v.key().clone();
        let entry_metadata = v.metadata().clone();

        Ok(StoreTree {
            key: Some(key),
            parents: None,
            entry_metadata: Some(entry_metadata),
            raw_content: Some(raw_content),
        })
    }
}

impl TryFrom<EdenApiTreeEntry> for StoreTree {
    type Error = Error;

    fn try_from(v: EdenApiTreeEntry) -> Result<Self, Self::Error> {
        // TODO(meyer): Optimize this to remove unnecessary clones.
        let raw_content = v.data_checked()?.into();
        Ok(StoreTree {
            key: Some(v.key().clone()),
            parents: v.parents.clone(),
            entry_metadata: None,
            raw_content: Some(raw_content),
        })
    }
}

impl StoreTree {
    pub fn key(&self) -> Option<&Key> {
        self.key.as_ref()
    }

    /// The tree content blob in the serialized tree-manifest format.
    pub fn content(&self) -> Option<&Bytes> {
        self.raw_content.as_ref()
    }
}
