// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use mercurial_types::{HgChangesetId, RepositoryId};
use mononoke_types::ChangesetId;

use schema::bonsai_hg_mapping;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[derive(Queryable, Insertable)]
#[table_name = "bonsai_hg_mapping"]
pub(crate) struct BonsaiHgMappingRow {
    pub repo_id: RepositoryId,
    pub hg_cs_id: HgChangesetId,
    pub bcs_id: ChangesetId,
}
