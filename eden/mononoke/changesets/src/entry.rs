/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use abomonation_derive::Abomonation;
use anyhow::Result;
use bytes::Bytes;
use fbthrift::compact_protocol;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;

#[derive(Abomonation, Clone, Debug, Eq, Hash, PartialEq)]
pub struct ChangesetEntry {
    pub repo_id: RepositoryId,
    pub cs_id: ChangesetId,
    pub parents: Vec<ChangesetId>,
    pub gen: u64,
}

impl ChangesetEntry {
    pub fn from_thrift(thrift_entry: changeset_entry_thrift::ChangesetEntry) -> Result<Self> {
        let parents: Result<Vec<_>> = thrift_entry
            .parents
            .into_iter()
            .map(ChangesetId::from_thrift)
            .collect();

        Ok(Self {
            repo_id: RepositoryId::new(thrift_entry.repo_id.0),
            cs_id: ChangesetId::from_thrift(thrift_entry.cs_id)?,
            parents: parents?,
            gen: thrift_entry.gen.0 as u64,
        })
    }

    pub fn into_thrift(self) -> changeset_entry_thrift::ChangesetEntry {
        changeset_entry_thrift::ChangesetEntry {
            repo_id: changeset_entry_thrift::RepoId(self.repo_id.id()),
            cs_id: self.cs_id.into_thrift(),
            parents: self.parents.into_iter().map(|p| p.into_thrift()).collect(),
            gen: changeset_entry_thrift::GenerationNum(self.gen as i64),
        }
    }
}

pub fn serialize_cs_entries(cs_entries: Vec<ChangesetEntry>) -> Bytes {
    let mut thrift_entries = vec![];
    for entry in cs_entries {
        let thrift_entry = changeset_entry_thrift::ChangesetEntry {
            repo_id: changeset_entry_thrift::RepoId(entry.repo_id.id()),
            cs_id: entry.cs_id.into_thrift(),
            parents: entry.parents.into_iter().map(|p| p.into_thrift()).collect(),
            gen: changeset_entry_thrift::GenerationNum(entry.gen as i64),
        };
        thrift_entries.push(thrift_entry);
    }

    compact_protocol::serialize(&thrift_entries)
}

pub fn deserialize_cs_entries(blob: &Bytes) -> Result<Vec<ChangesetEntry>> {
    let thrift_entries: Vec<changeset_entry_thrift::ChangesetEntry> =
        compact_protocol::deserialize(blob)?;
    let mut entries = vec![];
    for thrift_entry in thrift_entries {
        let parents = thrift_entry
            .parents
            .into_iter()
            .map(ChangesetId::from_thrift)
            .collect::<Result<Vec<_>>>()?;
        let entry = ChangesetEntry {
            repo_id: RepositoryId::new(thrift_entry.repo_id.0),
            cs_id: ChangesetId::from_thrift(thrift_entry.cs_id)?,
            parents,
            gen: thrift_entry.gen.0 as u64,
        };
        entries.push(entry);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_deserialize() {
        let entry = ChangesetEntry {
            repo_id: RepositoryId::new(0),
            cs_id: mononoke_types_mocks::changesetid::ONES_CSID,
            parents: vec![mononoke_types_mocks::changesetid::TWOS_CSID],
            gen: 2,
        };

        let res = deserialize_cs_entries(&serialize_cs_entries(vec![entry.clone(), entry.clone()]))
            .unwrap();
        assert_eq!(vec![entry.clone(), entry], res);
    }
}
