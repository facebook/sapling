/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod sql;

use mononoke_types::ChangesetId;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct BonsaiTagMappingEntry {
    pub changeset_id: ChangesetId,
    pub tag_name: String,
}

impl BonsaiTagMappingEntry {
    pub fn new(changeset_id: ChangesetId, tag_name: String) -> Self {
        BonsaiTagMappingEntry {
            changeset_id,
            tag_name,
        }
    }
}
