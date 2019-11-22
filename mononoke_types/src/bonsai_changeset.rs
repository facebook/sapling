/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::BTreeMap;

use failure_ext::{bail_err, chain::*, err_msg};
use fbthrift::compact_protocol;
use quickcheck::{Arbitrary, Gen};
use rand::Rng;

use crate::blob::{Blob, BlobstoreValue, ChangesetBlob};
use crate::datetime::DateTime;
use crate::errors::*;
use crate::file_change::FileChange;
use crate::path::{self, MPath};
use crate::thrift;
use crate::typed_hash::{ChangesetId, ChangesetIdContext};

/// A struct callers can use to build up a `BonsaiChangeset`.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct BonsaiChangesetMut {
    pub parents: Vec<ChangesetId>,
    pub author: String,
    pub author_date: DateTime,
    pub committer: Option<String>,
    // XXX should committer date always be recorded? If so, it should probably be a
    // monotonically increasing value:
    // max(author date, max(committer date of parents) + epsilon)
    pub committer_date: Option<DateTime>,
    pub message: String,
    pub extra: BTreeMap<String, Vec<u8>>,
    pub file_changes: BTreeMap<MPath, Option<FileChange>>,
}

impl BonsaiChangesetMut {
    /// Freeze this instance and turn it into a `BonsaiChangeset`.
    pub fn freeze(self) -> Result<BonsaiChangeset> {
        self.verify()?;
        Ok(BonsaiChangeset { inner: self })
    }

    /// Verify that this will form a valid `BonsaiChangeset`.
    ///
    /// Note that this doesn't (and can't) make any checks that require referring to data
    /// that's external to this changeset. For example, a changeset that deletes a file that
    /// doesn't exist in its parent is invalid. Instead, it only checks for internal consistency.
    pub fn verify(&self) -> Result<()> {
        // Check that the copy info ID refers to a parent in the parent set.
        for (path, fc_opt) in &self.file_changes {
            if let &Some(ref fc) = fc_opt {
                if let Some(&(ref copy_from_path, ref copy_from_id)) = fc.copy_from() {
                    if !self.parents.contains(copy_from_id) {
                        bail_err!(ErrorKind::InvalidBonsaiChangeset(format!(
                            "copy information for path '{}' (from '{}') has parent {} which isn't \
                             recognized",
                            path, copy_from_path, copy_from_id
                        )));
                    }
                }
            }
        }

        // Check that the list of file changes doesn't have any path conflicts.
        path::check_pcf(
            self.file_changes
                .iter()
                .map(|(path, change)| (path, change.is_some())),
        )
        .with_context(|| ErrorKind::InvalidBonsaiChangeset("invalid file change list".into()))?;

        Ok(())
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct BonsaiChangeset {
    inner: BonsaiChangesetMut,
}

impl BonsaiChangeset {
    pub(crate) fn from_thrift(tc: thrift::BonsaiChangeset) -> Result<Self> {
        let catch_block = || -> Result<_> {
            Ok(BonsaiChangesetMut {
                parents: tc
                    .parents
                    .into_iter()
                    .map(|parent| ChangesetId::from_thrift(parent))
                    .collect::<Result<_>>()?,
                author: tc.author,
                author_date: DateTime::from_thrift(
                    tc.author_date
                        .ok_or_else(|| err_msg("missing author date field"))?,
                )?,
                committer: tc.committer,
                committer_date: match tc.committer_date {
                    Some(dt) => Some(DateTime::from_thrift(dt)?),
                    None => None,
                },
                message: tc.message,
                extra: tc.extra,
                file_changes: tc
                    .file_changes
                    .into_iter()
                    .map(|(f, fc_opt)| {
                        let mpath = MPath::from_thrift(f)?;
                        let fc_opt = FileChange::from_thrift_opt(fc_opt, &mpath)?;
                        Ok((mpath, fc_opt))
                    })
                    .collect::<Result<_>>()?,
            }
            .freeze()?)
        };

        Ok(catch_block().with_context(|| {
            ErrorKind::InvalidThrift("BonsaiChangeset".into(), "Invalid changeset".into())
        })?)
    }

    /// Get the parents for this changeset. The order of parents is significant.
    pub fn parents<'a>(&'a self) -> impl Iterator<Item = ChangesetId> + 'a {
        self.inner.parents.iter().cloned()
    }

    /// Get the files changed in this changeset. The items returned are guaranteed
    /// to be in depth-first traversal order: once all the changes to a particular
    /// tree have been applied, it will never be referred to again.
    pub fn file_changes(&self) -> impl Iterator<Item = (&MPath, Option<&FileChange>)> {
        self.inner
            .file_changes
            .iter()
            .map(|(path, fc_opt)| (path, fc_opt.as_ref()))
    }

    pub fn file_changes_map(&self) -> &BTreeMap<MPath, Option<FileChange>> {
        &self.inner.file_changes
    }

    /// Get the author for this changeset.
    pub fn author(&self) -> &str {
        &self.inner.author
    }

    /// Get the author date (time and timezone) for this changeset.
    pub fn author_date(&self) -> &DateTime {
        &self.inner.author_date
    }

    /// Get the committer for this changeset.
    pub fn committer(&self) -> Option<&str> {
        match self.inner.committer {
            Some(ref c) => Some(c.as_str()),
            None => None,
        }
    }

    /// Get the committer date (time and timezone) for this changeset.
    pub fn committer_date(&self) -> Option<&DateTime> {
        match self.inner.committer_date {
            Some(ref dt) => Some(dt),
            None => None,
        }
    }

    /// Get the commit message.
    pub fn message(&self) -> &str {
        &self.inner.message
    }

    /// Get the extra fields for this message.
    pub fn extra(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.inner
            .extra
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_slice()))
    }

    pub fn get_changeset_id(&self) -> ChangesetId {
        *self.clone().into_blob().id()
    }

    /// Allow mutating this instance of `BonsaiChangeset`.
    pub fn into_mut(self) -> BonsaiChangesetMut {
        self.inner
    }

    pub(crate) fn into_thrift(self) -> thrift::BonsaiChangeset {
        thrift::BonsaiChangeset {
            parents: self
                .inner
                .parents
                .into_iter()
                .map(|parent| parent.into_thrift())
                .collect(),
            author: self.inner.author,
            author_date: Some(self.inner.author_date.into_thrift()),
            committer: self.inner.committer,
            committer_date: self.inner.committer_date.map(|dt| dt.into_thrift()),
            message: self.inner.message,
            extra: self.inner.extra,
            file_changes: self
                .inner
                .file_changes
                .into_iter()
                .map(|(f, c)| (f.into_thrift(), FileChange::into_thrift_opt(c)))
                .collect(),
        }
    }
}

impl BlobstoreValue for BonsaiChangeset {
    type Key = ChangesetId;

    fn into_blob(self) -> ChangesetBlob {
        let thrift = self.into_thrift();
        let data = compact_protocol::serialize(&thrift);
        let mut context = ChangesetIdContext::new();
        context.update(&data);
        let id = context.finish();
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(blob.data().as_ref())
            .chain_err(ErrorKind::BlobDeserializeError("BonsaiChangeset".into()))?;
        Self::from_thrift(thrift_tc)
    }
}

impl Arbitrary for BonsaiChangeset {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        // In the future Mononoke would like to support changesets with more parents than 2.
        // Start testing that now.
        let size = g.size();
        let num_parents = g.gen_range(0, 8);
        let parents: Vec<_> = (0..num_parents)
            .map(|_| ChangesetId::arbitrary(g))
            .collect();

        let num_changes = g.gen_range(0, size);
        let file_changes: BTreeMap<_, _> = (0..num_changes)
            .map(|_| {
                let fc_opt = if g.gen_ratio(1, 3) {
                    Some(FileChange::arbitrary_from_parents(g, &parents))
                } else {
                    None
                };
                // XXX be smarter about generating paths here?
                (MPath::arbitrary(g), fc_opt)
            })
            .collect();

        if path::check_pcf(
            file_changes
                .iter()
                .map(|(path, change)| (path, change.is_some())),
        )
        .is_err()
        {
            // This is rare but is definitely possible. Retry in this case.
            Self::arbitrary(g)
        } else {
            BonsaiChangesetMut {
                parents,
                file_changes,
                author: String::arbitrary(g),
                author_date: DateTime::arbitrary(g),
                committer: Option::<String>::arbitrary(g),
                committer_date: Option::<DateTime>::arbitrary(g),
                message: String::arbitrary(g),
                extra: BTreeMap::arbitrary(g),
            }
            .freeze()
            .expect("generated bonsai changeset must be valid")
        }
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        let cs = self.clone().inner;
        let iter = (
            cs.parents.clone(),
            cs.file_changes.clone(),
            cs.extra.clone(),
        )
            .shrink()
            .map(move |(parents, file_changes, extra)| {
                BonsaiChangesetMut {
                    parents,
                    file_changes,
                    author: cs.author.clone(),
                    author_date: cs.author_date,
                    committer: cs.committer.clone(),
                    committer_date: cs.committer_date,
                    message: cs.message.clone(),
                    extra,
                }
                .freeze()
                .expect("shrunken bonsai changeset must be valid")
            });
        Box::new(iter)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::file_change::FileType;
    use crate::hash::Blake2;
    use crate::typed_hash::ContentId;
    use maplit::btreemap;
    use quickcheck::quickcheck;
    use std::str::FromStr;

    quickcheck! {
        fn thrift_roundtrip(cs: BonsaiChangeset) -> bool {
            let thrift_cs = cs.clone().into_thrift();
            let cs2 = BonsaiChangeset::from_thrift(thrift_cs)
                .expect("thrift roundtrips should always be valid");
            cs == cs2
        }

        fn blob_roundtrip(cs: BonsaiChangeset) -> bool {
            let blob = cs.clone().into_blob();
            let cs2 = BonsaiChangeset::from_blob(blob)
                .expect("blob roundtrips should always be valid");
            cs == cs2
        }
    }

    #[test]
    fn fixed_blob() {
        let tc = BonsaiChangesetMut {
            parents: vec![ChangesetId::from_byte_array([3; 32])],
            author: "foo".into(),
            author_date: DateTime::from_timestamp(1234567890, 36800).unwrap(),
            committer: Some("bar".into()),
            committer_date: Some(DateTime::from_timestamp(1500000000, -36800).unwrap()),
            message: "Commit message".into(),
            extra: BTreeMap::new(),
            file_changes: btreemap![
                MPath::new("a/b").unwrap() => Some(FileChange::new(
                    ContentId::from_byte_array([1; 32]),
                    FileType::Regular,
                    42,
                    None,
                )),
                MPath::new("c/d").unwrap() => Some(FileChange::new(
                    ContentId::from_byte_array([2; 32]),
                    FileType::Executable,
                    84,
                    Some((
                        MPath::new("e/f").unwrap(),
                        ChangesetId::from_byte_array([3; 32]),
                    )),
                )),
                MPath::new("g/h").unwrap() => None,
                MPath::new("i/j").unwrap() => None,
            ],
        };
        let tc = tc.freeze().expect("fixed bonsai changeset must be valid");

        assert_eq!(
            tc.get_changeset_id(),
            ChangesetId::new(
                Blake2::from_str(
                    "189e67041363f9dc7d10de57aaf0fbd202dec989357e76cada7fa940936c712a"
                )
                .unwrap()
            )
        );
    }
}
