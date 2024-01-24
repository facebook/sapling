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
use bytes::Bytes;
use fbthrift::compact_protocol;
use quickcheck::Arbitrary;
use quickcheck::Gen;
use smallvec::SmallVec;
use sorted_vector_map::SortedVectorMap;

use crate::blob::Blob;
use crate::blob::BlobstoreValue;
use crate::blob::ChangesetBlob;
use crate::datetime::DateTime;
use crate::errors::MononokeTypeError;
use crate::file_change::BasicFileChange;
use crate::file_change::FileChange;
use crate::hash::GitSha1;
use crate::path;
use crate::path::NonRootMPath;
use crate::thrift;
use crate::typed_hash::ChangesetId;
use crate::typed_hash::ChangesetIdContext;
use crate::ContentId;

const ARBITRARY_SHRINK_FACTOR: usize = 3;

/// A struct callers can use to build up a `BonsaiChangeset`.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct BonsaiChangesetMut {
    pub parents: Vec<ChangesetId>,
    pub author: String,
    pub author_date: DateTime,
    pub committer: Option<String>,
    pub committer_date: Option<DateTime>,
    pub message: String,
    pub hg_extra: SortedVectorMap<String, Vec<u8>>,
    pub git_extra_headers: Option<SortedVectorMap<SmallVec<[u8; 24]>, Bytes>>,
    pub file_changes: SortedVectorMap<NonRootMPath, FileChange>,
    pub is_snapshot: bool,
    pub git_tree_hash: Option<GitSha1>,
    pub git_annotated_tag: Option<BonsaiAnnotatedTag>,
}

impl Default for BonsaiChangesetMut {
    fn default() -> Self {
        Self {
            parents: Vec::new(),
            author: String::default(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message: String::default(),
            hg_extra: SortedVectorMap::default(),
            git_extra_headers: None,
            file_changes: SortedVectorMap::default(),
            is_snapshot: false,
            git_tree_hash: None,
            git_annotated_tag: None,
        }
    }
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
            hg_extra: tc.hg_extra,
            git_extra_headers: tc
                .git_extra_headers
                .map(|extra| extra.into_iter().map(|(k, v)| (k.0, v)).collect()),
            file_changes: tc
                .file_changes
                .into_iter()
                .map(|(f, fc_opt)| {
                    let mpath = NonRootMPath::from_thrift(f)?;
                    let fc_opt = FileChange::from_thrift(fc_opt, &mpath)?;
                    Ok((mpath, fc_opt))
                })
                .collect::<Result<_>>()?,
            is_snapshot: tc.snapshot_state.is_some(),
            git_tree_hash: tc
                .git_tree_hash
                .map(|hash| GitSha1::from_bytes(hash.0))
                .transpose()
                .context("Invalid SHA1 hash for git tree hash")?,
            git_annotated_tag: tc
                .git_annotated_tag
                .map(BonsaiAnnotatedTag::from_thrift)
                .transpose()
                .context("Invalid annotated tag")?,
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
            hg_extra: self.hg_extra,
            git_extra_headers: self.git_extra_headers.map(|extra| {
                extra
                    .into_iter()
                    .map(|(k, v)| (thrift::small_binary(k), v))
                    .collect()
            }),
            file_changes: self
                .file_changes
                .into_iter()
                .map(|(f, c)| (f.into_thrift(), c.into_thrift()))
                .collect(),
            snapshot_state: self.is_snapshot.then_some(thrift::SnapshotState {}),
            git_tree_hash: self.git_tree_hash.map(|hash| hash.into_thrift()),
            git_annotated_tag: self.git_annotated_tag.map(BonsaiAnnotatedTag::into_thrift),
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
        // Check that the author and committer do not contain newline
        // characters.
        if let Some(offset) = self.author.find('\n') {
            bail!(MononokeTypeError::InvalidBonsaiChangeset(format!(
                "commit author contains a newline at offset {}",
                offset
            )));
        }
        if let Some(offset) = self.committer.as_ref().and_then(|c| c.find('\n')) {
            bail!(MononokeTypeError::InvalidBonsaiChangeset(format!(
                "committer contains a newline at offset {}",
                offset
            )));
        }

        // Check that the copy info ID refers to a parent in the parent set.
        for (path, fc) in &self.file_changes {
            if let Some((copy_from_path, copy_from_id)) = fc.copy_from() {
                if !self.parents.contains(copy_from_id) {
                    bail!(MononokeTypeError::InvalidBonsaiChangeset(format!(
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
        .with_context(|| {
            MononokeTypeError::InvalidBonsaiChangeset("invalid file change list".into())
        })?;

        // Check that this changeset has untracked/missing files only if it is a snapshot
        if !self.is_snapshot {
            for (_, fc) in &self.file_changes {
                match fc {
                    FileChange::UntrackedDeletion | FileChange::UntrackedChange(_) => {
                        bail!(MononokeTypeError::InvalidBonsaiChangeset(
                            "untracked changes present in non-snapshot changeset".to_string()
                        ));
                    }
                    FileChange::Deletion | FileChange::Change(_) => {}
                }
            }
        }

        // A changeset that represents a git tree should not have any file changes or parents
        if self.git_tree_hash.is_some()
            && !(self.parents.is_empty() && self.file_changes.is_empty())
        {
            bail!(MononokeTypeError::InvalidBonsaiChangeset("bonsai changeset representing a git tree should not have any parents or file changes".to_string()))
        }

        // If the changeset is a git annotated tag, it should not have any parents or file changes
        if self.git_annotated_tag.is_some()
            && !(self.parents.is_empty() && self.file_changes.is_empty())
        {
            bail!(MononokeTypeError::InvalidBonsaiChangeset("bonsai changeset representing a git annotated tag should not have any parents or file changes".to_string()))
        }

        // The changeset is either a git tree or a git annotated tag, but it cannot be both
        if self.git_tree_hash.is_some() && self.git_annotated_tag.is_some() {
            bail!(MononokeTypeError::InvalidBonsaiChangeset("bonsai changeset cannot represent both git tree and git annotated tag at the same time".to_string()))
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
            .with_context(|| MononokeTypeError::BlobDeserializeError("BonsaiChangeset".into()))?;
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
            MononokeTypeError::InvalidThrift("BonsaiChangeset".into(), "Invalid changeset".into())
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
    pub fn file_changes(&self) -> impl ExactSizeIterator<Item = (&NonRootMPath, &FileChange)> {
        self.inner.file_changes.iter()
    }

    /// File changes, but untracked and tracked changes are merged together for easy handling
    pub fn simplified_file_changes(
        &self,
    ) -> impl Iterator<Item = (&NonRootMPath, Option<&BasicFileChange>)> {
        self.inner
            .file_changes
            .iter()
            .map(|(path, fc)| (path, fc.simplify()))
    }

    pub fn file_changes_map(&self) -> &SortedVectorMap<NonRootMPath, FileChange> {
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

    /// Get the hg_extra fields for this message.
    pub fn hg_extra(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.inner
            .hg_extra
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_slice()))
    }

    /// Get the git_extra_headers fields for this message.
    pub fn git_extra_headers(&self) -> Option<impl Iterator<Item = (&[u8], &[u8])>> {
        self.inner
            .git_extra_headers
            .as_ref()
            .map(|extra| extra.iter().map(|(k, v)| (k.as_slice(), v.as_ref())))
    }

    /// Get the git tree hash corresponding to the current changeset
    pub fn git_tree_hash(&self) -> Option<&GitSha1> {
        self.inner.git_tree_hash.as_ref()
    }

    /// Get the changeset ID of this changeset.
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
            .with_context(|| MononokeTypeError::BlobDeserializeError("BonsaiChangeset".into()))?;
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
                (NonRootMPath::arbitrary(g), fc_opt)
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
            // Author and committer cannot contain newline, so remove any that
            // are generated.
            BonsaiChangesetMut {
                parents,
                file_changes: file_changes.into(),
                author: String::arbitrary(g).replace('\n', ""),
                author_date: DateTime::arbitrary(g),
                committer: Option::<String>::arbitrary(g).map(|s| s.replace('\n', "")),
                committer_date: Option::<DateTime>::arbitrary(g),
                message: String::arbitrary(g),
                hg_extra: SortedVectorMap::arbitrary(g),
                git_extra_headers: None,
                is_snapshot: bool::arbitrary(g),
                git_tree_hash: None,
                git_annotated_tag: None,
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
            cs.hg_extra.clone(),
            cs.git_tree_hash.clone(),
        )
            .shrink()
            .map(move |(parents, file_changes, hg_extra, git_tree_hash)| {
                BonsaiChangesetMut {
                    parents,
                    file_changes,
                    author: cs.author.clone(),
                    author_date: cs.author_date,
                    committer: cs.committer.clone(),
                    committer_date: cs.committer_date,
                    message: cs.message.clone(),
                    hg_extra,
                    git_extra_headers: cs.git_extra_headers.clone().map(|extra| {
                        extra
                            .into_iter()
                            .enumerate()
                            .filter_map(|(idx, val)| {
                                if idx % ARBITRARY_SHRINK_FACTOR == 0 {
                                    Some(val)
                                } else {
                                    None
                                }
                            })
                            .collect()
                    }),
                    is_snapshot: cs.is_snapshot,
                    git_tree_hash,
                    git_annotated_tag: cs.git_annotated_tag.clone(),
                }
                .freeze()
                .expect("shrunken bonsai changeset must be valid")
            });
        Box::new(iter)
    }
}

/// Target of an annotated tag imported from Git into Bonsai format.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum BonsaiAnnotatedTagTarget {
    /// Tag target as a Commit, Tree or yet another Tag.
    Changeset(ChangesetId),
    /// Tag target as raw file contents, i.e. Blob
    Content(ContentId),
}

impl BonsaiAnnotatedTagTarget {
    /// Create from a thrift `BonsaiAnnotatedTagTarget`.
    fn from_thrift(thrift_tag: thrift::BonsaiAnnotatedTagTarget) -> Result<Self> {
        match thrift_tag {
            thrift::BonsaiAnnotatedTagTarget::Changeset(id) => Ok(
                BonsaiAnnotatedTagTarget::Changeset(ChangesetId::from_thrift(id)?),
            ),
            thrift::BonsaiAnnotatedTagTarget::Content(id) => Ok(BonsaiAnnotatedTagTarget::Content(
                ContentId::from_thrift(id)?,
            )),
            thrift::BonsaiAnnotatedTagTarget::UnknownField(x) => {
                bail!(MononokeTypeError::InvalidThrift(
                    "BonsaiAnnotatedTagTarget".into(),
                    format!("unknown bonsai annotated tag target field: {}", x)
                ))
            }
        }
    }

    /// Convert into a thrift `BonsaiAnnotatedTagTarget`.
    fn into_thrift(self) -> thrift::BonsaiAnnotatedTagTarget {
        match self {
            BonsaiAnnotatedTagTarget::Changeset(id) => {
                thrift::BonsaiAnnotatedTagTarget::Changeset(id.into_thrift())
            }
            BonsaiAnnotatedTagTarget::Content(id) => {
                thrift::BonsaiAnnotatedTagTarget::Content(id.into_thrift())
            }
        }
    }
}

impl Arbitrary for BonsaiAnnotatedTagTarget {
    fn arbitrary(g: &mut Gen) -> Self {
        match u8::arbitrary(g) % 2 {
            0 => BonsaiAnnotatedTagTarget::Changeset(ChangesetId::arbitrary(g)),
            _ => BonsaiAnnotatedTagTarget::Content(ContentId::arbitrary(g)),
        }
    }
}

/// Bonsai counterpart of a git annotated tag. Used for referring to commits, tree, or other tags.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct BonsaiAnnotatedTag {
    // The target of the annotated tag
    pub target: BonsaiAnnotatedTagTarget,
    // The signature included with the tag
    pub pgp_signature: Option<Bytes>,
}

impl BonsaiAnnotatedTag {
    /// Create from a thrift `BonsaiAnnotatedTag`.
    fn from_thrift(thrift_tag: thrift::BonsaiAnnotatedTag) -> Result<Self> {
        Ok(Self {
            target: BonsaiAnnotatedTagTarget::from_thrift(thrift_tag.target)?,
            pgp_signature: thrift_tag.pgp_signature,
        })
    }

    /// Convert into a thrift `BonsaiAnnotatedTag`.
    fn into_thrift(self) -> thrift::BonsaiAnnotatedTag {
        thrift::BonsaiAnnotatedTag {
            target: self.target.into_thrift(),
            pgp_signature: self.pgp_signature,
        }
    }
}

impl Arbitrary for BonsaiAnnotatedTag {
    fn arbitrary(g: &mut Gen) -> Self {
        Self {
            target: BonsaiAnnotatedTagTarget::arbitrary(g),
            pgp_signature: Option::<Vec<u8>>::arbitrary(g).map(Bytes::from),
        }
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use quickcheck::quickcheck;
    use sorted_vector_map::sorted_vector_map;

    use super::*;
    use crate::file_change::FileType;
    use crate::hash::Blake2;
    use crate::typed_hash::ContentId;

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
            hg_extra: SortedVectorMap::new(),
            git_extra_headers: None,
            git_tree_hash: None,
            file_changes: sorted_vector_map![
                NonRootMPath::new("a/b").unwrap() => FileChange::tracked(
                    ContentId::from_byte_array([1; 32]),
                    FileType::Regular,
                    42,
                    None,
                ),
                NonRootMPath::new("c/d").unwrap() => FileChange::tracked(
                    ContentId::from_byte_array([2; 32]),
                    FileType::Executable,
                    84,
                    Some((
                        NonRootMPath::new("e/f").unwrap(),
                        ChangesetId::from_byte_array([3; 32]),
                    )),
                ),
                NonRootMPath::new("g/h").unwrap() => FileChange::Deletion,
                NonRootMPath::new("i/j").unwrap() => FileChange::Deletion,
            ],
            is_snapshot: false,
            git_annotated_tag: None,
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
    fn invalid_author_committer() {
        let invalid_author = BonsaiChangesetMut {
            parents: vec![],
            author: "test\nuser".into(),
            author_date: DateTime::from_timestamp(1, 2).unwrap(),
            committer: None,
            committer_date: None,
            message: "a".into(),
            hg_extra: SortedVectorMap::new(),
            git_extra_headers: None,
            git_tree_hash: None,
            file_changes: sorted_vector_map![],
            is_snapshot: false,
            git_annotated_tag: None,
        };

        assert!(invalid_author.freeze().is_err());

        let invalid_committer = BonsaiChangesetMut {
            parents: vec![],
            author: "test user".into(),
            author_date: DateTime::from_timestamp(1, 2).unwrap(),
            committer: Some("test\nuser".into()),
            committer_date: Some(DateTime::from_timestamp(1, 2).unwrap()),
            message: "a".into(),
            hg_extra: SortedVectorMap::new(),
            git_extra_headers: None,
            git_tree_hash: None,
            file_changes: sorted_vector_map![],
            is_snapshot: false,
            git_annotated_tag: None,
        };

        assert!(invalid_committer.freeze().is_err());
    }

    #[test]
    fn bonsai_snapshots() {
        fn create(untracked: bool, missing: bool, is_snapshot: bool) -> Result<BonsaiChangeset> {
            let mut file_changes = sorted_vector_map! [
                NonRootMPath::new("a").unwrap() => FileChange::tracked(
                    ContentId::from_byte_array([1; 32]),
                    FileType::Regular,
                    42,
                    None,
                ),
                NonRootMPath::new("b").unwrap() => FileChange::Deletion,
            ];
            if untracked {
                file_changes.insert(
                    NonRootMPath::new("c").unwrap(),
                    FileChange::untracked(
                        ContentId::from_byte_array([2; 32]),
                        FileType::Regular,
                        42,
                    ),
                );
            }
            if missing {
                file_changes.insert(
                    NonRootMPath::new("d").unwrap(),
                    FileChange::UntrackedDeletion,
                );
            }
            BonsaiChangesetMut {
                parents: vec![],
                author: "foo".into(),
                author_date: DateTime::from_timestamp(1, 2).unwrap(),
                committer: None,
                committer_date: None,
                message: "a".into(),
                hg_extra: SortedVectorMap::new(),
                git_extra_headers: None,
                git_tree_hash: None,
                file_changes,
                is_snapshot,
                git_annotated_tag: None,
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

    #[test]
    fn invalid_git_tree_bonsai_changeset() {
        let changeset = BonsaiChangesetMut {
            parents: vec![ChangesetId::from_byte_array([3; 32])],
            author: String::new(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message: String::new(),
            hg_extra: SortedVectorMap::new(),
            git_extra_headers: None,
            file_changes: SortedVectorMap::new(),
            is_snapshot: false,
            git_tree_hash: GitSha1::from_bytes([
                0xda, 0x39, 0xa3, 0xee, 0x5e, 0x6b, 0x4b, 0x0d, 0x32, 0x55, 0xbf, 0xef, 0x95, 0x60,
                0x18, 0x90, 0xaf, 0xd8, 0x07, 0x09,
            ])
            .ok(),
            git_annotated_tag: None,
        };
        changeset
            .freeze()
            .expect_err("Changeset representing Git trees can't have parents");

        let changeset = BonsaiChangesetMut {
            parents: vec![],
            author: String::new(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message: String::new(),
            hg_extra: SortedVectorMap::new(),
            git_extra_headers: None,
            file_changes: sorted_vector_map! [
                NonRootMPath::new("a").unwrap() => FileChange::tracked(
                    ContentId::from_byte_array([1; 32]),
                    FileType::Regular,
                    42,
                    None,
                ),
                NonRootMPath::new("b").unwrap() => FileChange::Deletion,
            ],
            is_snapshot: false,
            git_tree_hash: GitSha1::from_bytes([
                0xda, 0x39, 0xa3, 0xee, 0x5e, 0x6b, 0x4b, 0x0d, 0x32, 0x55, 0xbf, 0xef, 0x95, 0x60,
                0x18, 0x90, 0xaf, 0xd8, 0x07, 0x09,
            ])
            .ok(),
            git_annotated_tag: None,
        };
        changeset
            .freeze()
            .expect_err("Changeset representing Git trees can't have file changes");
    }

    #[test]
    fn valid_git_tree_bonsai_changeset() {
        let changeset = BonsaiChangesetMut {
            parents: vec![],
            author: String::new(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message: String::new(),
            hg_extra: SortedVectorMap::new(),
            git_extra_headers: None,
            file_changes: SortedVectorMap::new(),
            is_snapshot: false,
            git_tree_hash: GitSha1::from_bytes([
                0xda, 0x39, 0xa3, 0xee, 0x5e, 0x6b, 0x4b, 0x0d, 0x32, 0x55, 0xbf, 0xef, 0x95, 0x60,
                0x18, 0x90, 0xaf, 0xd8, 0x07, 0x09,
            ])
            .ok(),
            git_annotated_tag: None,
        };
        changeset.freeze().unwrap();
    }

    #[test]
    fn valid_git_extra_headers_bonsai_changeset() {
        let changeset = BonsaiChangesetMut {
            parents: vec![ChangesetId::from_byte_array([3; 32])],
            author: String::new(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message: String::new(),
            hg_extra: SortedVectorMap::new(),
            git_extra_headers: Some(sorted_vector_map! [
                SmallVec::from_vec(b"JustAHeader".to_vec()) => Bytes::from_static(b"Some Content")
            ]),
            file_changes: sorted_vector_map! [
                NonRootMPath::new("a").unwrap() => FileChange::tracked(
                    ContentId::from_byte_array([1; 32]),
                    FileType::Regular,
                    42,
                    None,
                ),
                NonRootMPath::new("b").unwrap() => FileChange::Deletion,
            ],
            is_snapshot: false,
            git_tree_hash: None,
            git_annotated_tag: None,
        };
        changeset.freeze().unwrap();
    }

    #[test]
    fn invalid_git_tag_bonsai_changeset() {
        let changeset = BonsaiChangesetMut {
            parents: vec![ChangesetId::from_byte_array([3; 32])],
            author: String::new(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message: String::new(),
            hg_extra: SortedVectorMap::new(),
            git_extra_headers: Some(sorted_vector_map! [
                SmallVec::from_vec(b"JustAHeader".to_vec()) => Bytes::from_static(b"Some Content")
            ]),
            file_changes: SortedVectorMap::new(),
            is_snapshot: false,
            git_tree_hash: None,
            git_annotated_tag: Some(BonsaiAnnotatedTag {
                target: BonsaiAnnotatedTagTarget::Changeset(ChangesetId::from_byte_array([4; 32])),
                pgp_signature: None,
            }),
        };
        changeset
            .freeze()
            .expect_err("Bonsai changeset representing a git annotated tag cannot have parents");

        let changeset = BonsaiChangesetMut {
            parents: vec![],
            author: String::new(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message: String::new(),
            hg_extra: SortedVectorMap::new(),
            git_extra_headers: Some(sorted_vector_map! [
                SmallVec::from_vec(b"JustAHeader".to_vec()) => Bytes::from_static(b"Some Content")
            ]),
            file_changes: sorted_vector_map! [
                NonRootMPath::new("a").unwrap() => FileChange::tracked(
                    ContentId::from_byte_array([1; 32]),
                    FileType::Regular,
                    42,
                    None,
                ),
                NonRootMPath::new("b").unwrap() => FileChange::Deletion,
            ],
            is_snapshot: false,
            git_tree_hash: None,
            git_annotated_tag: Some(BonsaiAnnotatedTag {
                target: BonsaiAnnotatedTagTarget::Changeset(ChangesetId::from_byte_array([3; 32])),
                pgp_signature: None,
            }),
        };
        changeset.freeze().expect_err(
            "Bonsai changeset representing a git annotated tag cannot have file changes",
        );

        let changeset = BonsaiChangesetMut {
            parents: vec![],
            author: String::new(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message: String::new(),
            hg_extra: SortedVectorMap::new(),
            git_extra_headers: None,
            file_changes: SortedVectorMap::new(),
            is_snapshot: false,
            git_tree_hash: GitSha1::from_bytes([
                0xda, 0x39, 0xa3, 0xee, 0x5e, 0x6b, 0x4b, 0x0d, 0x32, 0x55, 0xbf, 0xef, 0x95, 0x60,
                0x18, 0x90, 0xaf, 0xd8, 0x07, 0x09,
            ])
            .ok(),
            git_annotated_tag: Some(BonsaiAnnotatedTag {
                target: BonsaiAnnotatedTagTarget::Changeset(ChangesetId::from_byte_array([3; 32])),
                pgp_signature: None,
            }),
        };
        changeset.freeze().expect_err(
            "Bonsai changeset representing a git annotated tag cannot have a git tree hash (indicating that its a git tree) as part of it",
        );
    }

    #[test]
    fn valid_git_tag_bonsai_changeset() {
        let changeset = BonsaiChangesetMut {
            parents: vec![],
            author: String::new(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message: String::new(),
            hg_extra: SortedVectorMap::new(),
            git_extra_headers: None,
            file_changes: SortedVectorMap::new(),
            is_snapshot: false,
            git_tree_hash: None,
            git_annotated_tag: Some(BonsaiAnnotatedTag {
                target: BonsaiAnnotatedTagTarget::Changeset(ChangesetId::from_byte_array([3; 32])),
                pgp_signature: None,
            }),
        };
        changeset.freeze().unwrap();
    }

    #[test]
    fn valid_git_tag_bonsai_changeset_with_author_and_message() {
        let changeset = BonsaiChangesetMut {
            parents: vec![],
            author: "TagCreator".to_string(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message: "This is the tag description".to_string(),
            hg_extra: SortedVectorMap::new(),
            git_extra_headers: None,
            file_changes: SortedVectorMap::new(),
            is_snapshot: false,
            git_tree_hash: None,
            git_annotated_tag: Some(BonsaiAnnotatedTag {
                target: BonsaiAnnotatedTagTarget::Changeset(ChangesetId::from_byte_array([3; 32])),
                pgp_signature: None,
            }),
        };
        changeset.freeze().unwrap();
    }
}
