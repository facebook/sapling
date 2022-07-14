/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

use anyhow::bail;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use fbthrift::compact_protocol;
use quickcheck::Arbitrary;
use quickcheck::Gen;
use sorted_vector_map::SortedVectorMap;

use crate::blob::Blob;
use crate::blob::BlobstoreValue;
use crate::blob::ChangesetBlob;
use crate::datetime::DateTime;
use crate::errors::ErrorKind;
use crate::file_change::BasicFileChange;
use crate::file_change::FileChange;
use crate::path;
use crate::path::MPath;
use crate::thrift;
use crate::typed_hash::ChangesetId;
use crate::typed_hash::ChangesetIdContext;

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
    pub extra: SortedVectorMap<String, Vec<u8>>,
    pub file_changes: SortedVectorMap<MPath, FileChange>,
    pub is_snapshot: bool,
}

impl BonsaiChangesetMut {
    /// Create from a thrift `BonsaiChangeset`.
    fn from_thrift(tc: thrift::BonsaiChangeset) -> Result<Self> {
        Ok(BonsaiChangesetMut {
            parents: tc
                .parents
                .into_iter()
                .map(ChangesetId::from_thrift)
                .collect::<Result<_>>()?,
            author: tc.author,
            author_date: DateTime::from_thrift(
                tc.author_date
                    .ok_or_else(|| Error::msg("missing author date field"))?,
            )?,
            committer: tc.committer,
            committer_date: tc.committer_date.map(DateTime::from_thrift).transpose()?,
            message: tc.message,
            extra: tc.extra,
            file_changes: tc
                .file_changes
                .into_iter()
                .map(|(f, fc_opt)| {
                    let mpath = MPath::from_thrift(f)?;
                    let fc_opt = FileChange::from_thrift(fc_opt, &mpath)?;
                    Ok((mpath, fc_opt))
                })
                .collect::<Result<_>>()?,
            is_snapshot: tc.snapshot_state.is_some(),
        })
    }

    /// Convert into a thrift `BonsaiChangeset`.
    fn into_thrift(self) -> thrift::BonsaiChangeset {
        thrift::BonsaiChangeset {
            parents: self
                .parents
                .into_iter()
                .map(|parent| parent.into_thrift())
                .collect(),
            author: self.author,
            author_date: Some(self.author_date.into_thrift()),
            committer: self.committer,
            committer_date: self.committer_date.map(DateTime::into_thrift),
            message: self.message,
            extra: self.extra,
            file_changes: self
                .file_changes
                .into_iter()
                .map(|(f, c)| (f.into_thrift(), c.into_thrift()))
                .collect(),
            snapshot_state: self.is_snapshot.then(|| thrift::SnapshotState {}),
        }
    }

    /// Compute the changeset id for the `BonsaiChangeset`.
    fn changeset_id(&self) -> ChangesetId {
        let thrift = self.clone().into_thrift();
        let data = compact_protocol::serialize(&thrift);
        let mut context = ChangesetIdContext::new();
        context.update(&data);
        context.finish()
    }

    /// Freeze this instance and turn it into a `BonsaiChangeset`.
    pub fn freeze(self) -> Result<BonsaiChangeset> {
        self.verify()?;
        let id = self.changeset_id();
        Ok(BonsaiChangeset { inner: self, id })
    }

    /// Verify that this will form a valid `BonsaiChangeset`.
    ///
    /// Note that this doesn't (and can't) make any checks that require referring to data
    /// that's external to this changeset. For example, a changeset that deletes a file that
    /// doesn't exist in its parent is invalid. Instead, it only checks for internal consistency.
    pub fn verify(&self) -> Result<()> {
        // Check that the copy info ID refers to a parent in the parent set.
        for (path, fc) in &self.file_changes {
            if let Some(&(ref copy_from_path, ref copy_from_id)) = fc.copy_from() {
                if !self.parents.contains(copy_from_id) {
                    bail!(ErrorKind::InvalidBonsaiChangeset(format!(
                        "copy information for path '{}' (from '{}') has parent {} which isn't \
                             recognized",
                        path, copy_from_path, copy_from_id
                    )));
                }
            }
        }

        // Check that the list of file changes doesn't have any path conflicts.
        path::check_pcf(
            self.file_changes
                .iter()
                .map(|(path, change)| (path, change.is_changed())),
        )
        .with_context(|| ErrorKind::InvalidBonsaiChangeset("invalid file change list".into()))?;

        // Check that this changeset has untracked/missing files only if it is a snapshot
        if !self.is_snapshot {
            for (_, fc) in &self.file_changes {
                match fc {
                    FileChange::UntrackedDeletion | FileChange::UntrackedChange(_) => {
                        bail!(ErrorKind::InvalidBonsaiChangeset(
                            "untracked changes present in non-snapshot changeset".to_string()
                        ));
                    }
                    FileChange::Deletion | FileChange::Change(_) => {}
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct BonsaiChangeset {
    /// The changeset data.
    ///
    /// This is immutable while the changeset is frozen.  Use `into_mut` to
    /// thaw the changeset and make it mutable again.
    inner: BonsaiChangesetMut,

    /// The changeset id.
    ///
    /// `BonsaiChangeset` is immutable, so this is computed once, either when
    /// the changeset is frozen, or when it is loaded.
    id: ChangesetId,
}

impl BonsaiChangeset {
    pub fn from_bytes(data: impl AsRef<[u8]>) -> Result<Self> {
        let data = data.as_ref();
        let id = {
            let mut context = ChangesetIdContext::new();
            context.update(data);
            context.finish()
        };
        let thrift_tc = compact_protocol::deserialize(data)
            .with_context(|| ErrorKind::BlobDeserializeError("BonsaiChangeset".into()))?;
        let bcs = Self::from_thrift_with_id(thrift_tc, id)?;
        Ok(bcs)
    }

    fn from_thrift_with_id(tc: thrift::BonsaiChangeset, id: ChangesetId) -> Result<Self> {
        let catch_block = || -> Result<_> {
            let inner = BonsaiChangesetMut::from_thrift(tc)?;
            inner.verify()?;
            Ok(BonsaiChangeset { inner, id })
        };

        catch_block().with_context(|| {
            ErrorKind::InvalidThrift("BonsaiChangeset".into(), "Invalid changeset".into())
        })
    }

    /// Get the parents for this changeset. The order of parents is significant.
    pub fn parents<'a>(&'a self) -> impl Iterator<Item = ChangesetId> + 'a {
        self.inner.parents.iter().cloned()
    }

    #[inline]
    pub fn is_merge(&self) -> bool {
        self.inner.parents.len() > 1
    }

    /// Get the files changed in this changeset. The items returned are guaranteed
    /// to be in depth-first traversal order: once all the changes to a particular
    /// tree have been applied, it will never be referred to again.
    pub fn file_changes(&self) -> impl ExactSizeIterator<Item = (&MPath, &FileChange)> {
        self.inner.file_changes.iter()
    }

    /// File changes, but untracked and tracked changes are merged together for easy handling
    pub fn simplified_file_changes(
        &self,
    ) -> impl Iterator<Item = (&MPath, Option<&BasicFileChange>)> {
        self.inner
            .file_changes
            .iter()
            .map(|(path, fc)| (path, fc.simplify()))
    }

    pub fn file_changes_map(&self) -> &SortedVectorMap<MPath, FileChange> {
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
        self.inner.committer_date.as_ref()
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
        self.id
    }

    /// Whether this changeset is a snapshot
    pub fn is_snapshot(&self) -> bool {
        self.inner.is_snapshot
    }

    /// Allow mutating this instance of `BonsaiChangeset`.
    pub fn into_mut(self) -> BonsaiChangesetMut {
        self.inner
    }
}

impl BlobstoreValue for BonsaiChangeset {
    type Key = ChangesetId;

    fn into_blob(self) -> ChangesetBlob {
        let id = self.id;
        let thrift = self.inner.into_thrift();
        let data = compact_protocol::serialize(&thrift);
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(blob.data().as_ref())
            .with_context(|| ErrorKind::BlobDeserializeError("BonsaiChangeset".into()))?;
        let bcs = Self::from_thrift_with_id(thrift_tc, *blob.id())?;
        Ok(bcs)
    }
}

impl Arbitrary for BonsaiChangeset {
    fn arbitrary(g: &mut Gen) -> Self {
        // In the future Mononoke would like to support changesets with more parents than 2.
        // Start testing that now.
        let size = g.size();
        let num_parents = usize::arbitrary(g) % 8;
        let parents: Vec<_> = (0..num_parents)
            .map(|_| ChangesetId::arbitrary(g))
            .collect();

        let num_changes = usize::arbitrary(g) % size;
        let file_changes: BTreeMap<_, _> = (0..num_changes)
            .map(|_| {
                let fc_opt = if *g.choose(&[0, 1, 2]).unwrap() < 1 {
                    FileChange::arbitrary_from_parents(g, &parents)
                } else {
                    FileChange::Deletion
                };
                // XXX be smarter about generating paths here?
                (MPath::arbitrary(g), fc_opt)
            })
            .collect();

        if path::check_pcf(
            file_changes
                .iter()
                .map(|(path, change)| (path, change.is_changed())),
        )
        .is_err()
        {
            // This is rare but is definitely possible. Retry in this case.
            Self::arbitrary(g)
        } else {
            BonsaiChangesetMut {
                parents,
                file_changes: file_changes.into(),
                author: String::arbitrary(g),
                author_date: DateTime::arbitrary(g),
                committer: Option::<String>::arbitrary(g),
                committer_date: Option::<DateTime>::arbitrary(g),
                message: String::arbitrary(g),
                extra: SortedVectorMap::arbitrary(g),
                is_snapshot: bool::arbitrary(g),
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
                    is_snapshot: cs.is_snapshot,
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
    use quickcheck::quickcheck;
    use sorted_vector_map::sorted_vector_map;
    use std::str::FromStr;

    quickcheck! {
        fn thrift_roundtrip(cs: BonsaiChangeset) -> bool {
            let thrift_cs = cs.inner.clone().into_thrift();
            let cs2 = BonsaiChangeset::from_thrift_with_id(thrift_cs, cs.id)
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
            extra: SortedVectorMap::new(),
            file_changes: sorted_vector_map![
                MPath::new("a/b").unwrap() => FileChange::tracked(
                    ContentId::from_byte_array([1; 32]),
                    FileType::Regular,
                    42,
                    None,
                ),
                MPath::new("c/d").unwrap() => FileChange::tracked(
                    ContentId::from_byte_array([2; 32]),
                    FileType::Executable,
                    84,
                    Some((
                        MPath::new("e/f").unwrap(),
                        ChangesetId::from_byte_array([3; 32]),
                    )),
                ),
                MPath::new("g/h").unwrap() => FileChange::Deletion,
                MPath::new("i/j").unwrap() => FileChange::Deletion,
            ],
            is_snapshot: false,
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

    #[test]
    fn bonsai_snapshots() {
        fn create(untracked: bool, missing: bool, is_snapshot: bool) -> Result<BonsaiChangeset> {
            let mut file_changes = sorted_vector_map! [
                MPath::new("a").unwrap() => FileChange::tracked(
                    ContentId::from_byte_array([1; 32]),
                    FileType::Regular,
                    42,
                    None,
                ),
                MPath::new("b").unwrap() => FileChange::Deletion,
            ];
            if untracked {
                file_changes.insert(
                    MPath::new("c").unwrap(),
                    FileChange::untracked(
                        ContentId::from_byte_array([2; 32]),
                        FileType::Regular,
                        42,
                    ),
                );
            }
            if missing {
                file_changes.insert(MPath::new("d").unwrap(), FileChange::UntrackedDeletion);
            }
            BonsaiChangesetMut {
                parents: vec![],
                author: "foo".into(),
                author_date: DateTime::from_timestamp(1, 2).unwrap(),
                committer: None,
                committer_date: None,
                message: "a".into(),
                extra: SortedVectorMap::new(),
                file_changes,
                is_snapshot,
            }
            .freeze()
        }
        create(true, true, true).unwrap();
        create(false, false, false).unwrap();
        create(true, false, true).unwrap();
        create(true, false, false).expect_err("Non-snapshot can't have untracked");
        create(false, true, false).expect_err("Non-snapshot can't have missing");
        create(true, true, false).unwrap_err();
    }
}
