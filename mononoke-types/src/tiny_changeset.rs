// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::{BTreeMap, BTreeSet};

use quickcheck::{Arbitrary, Gen};

use datetime::DateTime;
use errors::*;
use file_change::FileChange;
use path::MPath;
use thrift;
use typed_hash::ChangesetId;

/// A struct callers can use to build up a `TinyChangeset`.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct TinyChangesetMut {
    pub parents: Vec<ChangesetId>,
    pub user: String,
    pub date: DateTime,
    pub message: String,
    pub extra: BTreeMap<String, String>,
    pub file_changes: BTreeMap<MPath, FileChange>,
    pub file_deletes: BTreeSet<MPath>,
}

impl TinyChangesetMut {
    /// Freeze this instance and turn it into a `TinyChangeset`.
    pub fn freeze(self) -> TinyChangeset {
        TinyChangeset { inner: self }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct TinyChangeset {
    inner: TinyChangesetMut,
}

impl TinyChangeset {
    pub(crate) fn from_thrift(tc: thrift::TinyChangeset) -> Result<Self> {
        let catch_block = || {
            Ok(TinyChangesetMut {
                parents: tc.parents
                    .into_iter()
                    .map(|parent| ChangesetId::from_thrift(parent))
                    .collect::<Result<_>>()?,
                user: tc.user,
                date: DateTime::from_thrift(tc.date)?,
                message: tc.message,
                extra: tc.extra,
                file_changes: tc.file_changes
                    .into_iter()
                    .map(|(f, c)| {
                        let mpath = MPath::from_thrift(f)?;
                        let cf = FileChange::from_thrift(c, &mpath)?;
                        Ok((mpath, cf))
                    })
                    .collect::<Result<_>>()?,
                file_deletes: tc.file_deletes
                    .into_iter()
                    .map(|f| MPath::from_thrift(f))
                    .collect::<Result<_>>()?,
            })
        };

        Ok(catch_block()
            .with_context(|_: &Error| {
                ErrorKind::InvalidThrift("TinyChangeset".into(), "Invalid changeset".into())
            })?
            .freeze())
    }
    /// Get the parents for this changeset. The order of parents is significant.
    pub fn parents(&self) -> impl Iterator<Item = &ChangesetId> {
        self.inner.parents.iter()
    }

    /// Get the files changed in this changeset. The items returned are guaranteed
    /// to be in depth-first traversal order: once all the changes to a particular
    /// tree have been applied, it will never be referred to again.
    pub fn file_changes(&self) -> impl Iterator<Item = (&MPath, &FileChange)> {
        self.inner.file_changes.iter()
    }

    /// Get the files deleted in this changeset. The items returned are guaranteed
    /// to be in depth-first traversal order: once all the changes to a particular
    /// tree have been applied, it will never be referred to again.
    pub fn file_deletes(&self) -> impl Iterator<Item = &MPath> {
        self.inner.file_deletes.iter()
    }

    /// Get the user for this changeset.
    pub fn user(&self) -> &str {
        &self.inner.user
    }

    /// Get the time and timezone for this changeset.
    pub fn date(&self) -> &DateTime {
        &self.inner.date
    }

    /// Get the commit message.
    pub fn message(&self) -> &str {
        &self.inner.message
    }

    /// Get the extra fields for this message.
    pub fn extra(&self) -> impl Iterator<Item = (&str, &str)> {
        self.inner
            .extra
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Allow mutating this instance of `TinyChangeset`.
    pub fn into_mut(self) -> TinyChangesetMut {
        self.inner
    }

    pub(crate) fn into_thrift(self) -> thrift::TinyChangeset {
        thrift::TinyChangeset {
            parents: self.inner
                .parents
                .into_iter()
                .map(|parent| parent.into_thrift())
                .collect(),
            user: self.inner.user,
            date: self.inner.date.into_thrift(),
            message: self.inner.message,
            extra: self.inner.extra,
            file_changes: self.inner
                .file_changes
                .into_iter()
                .map(|(f, c)| (f.into_thrift(), c.into_thrift()))
                .collect(),
            file_deletes: self.inner
                .file_deletes
                .into_iter()
                .map(|f| f.into_thrift())
                .collect(),
        }
    }
}

impl Arbitrary for TinyChangeset {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        // In the future Mononoke would like to support changesets with more parents than 2.
        // Start testing that now.
        let num_parents = g.gen_range(0, 8);
        let parents = (0..num_parents)
            .map(|_| ChangesetId::arbitrary(g))
            .collect();
        TinyChangesetMut {
            parents,
            file_changes: BTreeMap::arbitrary(g),
            file_deletes: BTreeSet::arbitrary(g),
            user: String::arbitrary(g),
            date: DateTime::arbitrary(g),
            message: String::arbitrary(g),
            extra: BTreeMap::arbitrary(g),
        }.freeze()
    }

    fn shrink(&self) -> Box<Iterator<Item = Self>> {
        let cs = self.clone().inner;
        let iter = (
            cs.parents.clone(),
            cs.file_changes.clone(),
            cs.file_deletes.clone(),
            cs.extra.clone(),
        ).shrink()
            .map(move |(parents, file_changes, file_deletes, extra)| {
                TinyChangesetMut {
                    parents,
                    file_changes,
                    file_deletes,
                    user: cs.user.clone(),
                    date: cs.date,
                    message: cs.message.clone(),
                    extra,
                }.freeze()
            });
        Box::new(iter)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    quickcheck! {
        fn thrift_roundtrip(cs: TinyChangeset) -> bool {
            let thrift_cs = cs.clone().into_thrift();
            let cs2 = TinyChangeset::from_thrift(thrift_cs)
                .expect("thrift roundtrips should always be valid");
            cs == cs2
        }
    }
}
