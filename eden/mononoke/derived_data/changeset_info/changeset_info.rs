/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::format_err;
use blobstore::BlobstoreGetData;
use bytes::Bytes;
use fbthrift::compact_protocol;
use mononoke_types::BlobstoreBytes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::errors::MononokeTypeError;
use mononoke_types_serialization as thrift;
use smallvec::SmallVec;
use sorted_vector_map::SortedVectorMap;
use unicode_segmentation::UnicodeSegmentation;

/// Changeset Info is a derived data structure that represents a Bonsai changeset's
/// metadata.
///
/// Bonsai changeset consists of the commit metadata and a set of all the file
/// changes associated with the commit. Some of the changesets, usually for merge
/// commits, include thousands of file changes and touch history of even bigger
/// number of files. This makes fetching changesets an expensive operation due
/// to large commits taking many seconds to deserialize and affects performance
/// of the Source Control Service API methods.
///
/// Changeset Info data structure comes to resolve the necessity to waste time
/// deserializing lots of file changes, when commit's metadata is the main
/// reason the commit was fetched.
///
/// Q&A
///
/// Is the ChangesetInfo content any different from the Bonsai apart from the file changes?
///
/// Not really, ChangesetInfo includes all the metadata Bonsai has and its own "linknode" -
/// changeset id of the source Bonsai.
///
/// Why do we store commit message as union?
///
/// The commit message potentially can be quite large. So in the future we might want
/// to store the whole description separately from the changeset info. As an alternative
/// to the string containing message we could have a title and a message handler -
/// message id to fetch.
///
/// Why do we need title?
///
/// Some of the important SCS API methods only need title in most of the cases. Having
/// the whole heavy commit message in the store and a small title always available let's
/// us avoid fetching items from the blobstore twice: first commit info and then message.

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct ChangesetInfo {
    /// changeset id of the source Bonsai changeset
    changeset_id: ChangesetId,
    parents: Vec<ChangesetId>,
    author: String,
    author_date: DateTime,
    committer: Option<String>,
    committer_date: Option<DateTime>,
    message: ChangesetMessage,
    hg_extra: SortedVectorMap<String, Vec<u8>>,
    git_extra_headers: Option<SortedVectorMap<SmallVec<[u8; 24]>, Bytes>>,
    subtree_change_count: usize,
}

/// At some point we may like to store large commit messages as separate blobs
/// to make fetching changesets faster if there is no need in the whole description.
/// For example:
///     Handler((String /* title */, ChangesetMessageId /* message blob id */))
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum ChangesetMessage {
    Message(String),
}

impl ChangesetMessage {
    pub(crate) fn from_thrift(tc: thrift::changeset_info::ChangesetMessage) -> Result<Self> {
        match tc {
            thrift::changeset_info::ChangesetMessage::message(message) => {
                Ok(ChangesetMessage::Message(message))
            }
            thrift::changeset_info::ChangesetMessage::UnknownField(other) => {
                Err(format_err!("Unknown ChangesetMessage field: {}", other))
            }
        }
    }

    pub(crate) fn into_thrift(self) -> thrift::changeset_info::ChangesetMessage {
        match self {
            ChangesetMessage::Message(msg) => {
                thrift::changeset_info::ChangesetMessage::message(msg)
            }
        }
    }
}

pub const DEFAULT_TITLE_LENGTH: usize = 1024;

impl ChangesetInfo {
    pub fn new(changeset_id: ChangesetId, changeset: BonsaiChangeset) -> Self {
        let BonsaiChangesetMut {
            parents,
            author,
            author_date,
            committer,
            committer_date,
            message,
            hg_extra,
            git_extra_headers,
            subtree_changes,
            ..
        } = changeset.into_mut();

        Self {
            changeset_id,
            parents,
            author,
            author_date,
            committer,
            committer_date,
            message: ChangesetMessage::Message(message),
            hg_extra,
            git_extra_headers,
            subtree_change_count: subtree_changes.len(),
        }
    }

    /// Get id of the source Bonsai changeset.
    pub fn changeset_id(&self) -> &ChangesetId {
        &self.changeset_id
    }

    /// Get the changeset parents.
    pub fn parents<'a>(&'a self) -> impl Iterator<Item = ChangesetId> + 'a {
        self.parents.iter().cloned()
    }

    /// Get the author.
    pub fn author(&self) -> &str {
        &self.author
    }

    /// Get the author date (time and timezone).
    pub fn author_date(&self) -> &DateTime {
        &self.author_date
    }

    /// Get the committer.
    pub fn committer(&self) -> Option<&str> {
        self.committer.as_deref()
    }

    /// Get the committer date (time and timezone).
    pub fn committer_date(&self) -> Option<&DateTime> {
        self.committer_date.as_ref()
    }

    /// Get the commit title: the first line of the commit message or the first
    /// DEFAULT_TITLE_LENGTH characters.
    pub fn title(&self) -> &str {
        match &self.message {
            ChangesetMessage::Message(message) => get_title(message),
        }
    }

    /// Get the commit message.
    pub fn message(&self) -> &str {
        match &self.message {
            ChangesetMessage::Message(msg) => msg,
        }
    }

    /// Get the extra fields for this message.
    pub fn hg_extra(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.hg_extra
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_slice()))
    }

    pub fn git_extra_headers(&self) -> Option<impl Iterator<Item = (&[u8], &[u8])>> {
        self.git_extra_headers
            .as_ref()
            .map(|extra| extra.iter().map(|(k, v)| (k.as_slice(), v.as_ref())))
    }

    /// Get the count of subtree changes for this changeset.
    pub fn subtree_change_count(&self) -> usize {
        self.subtree_change_count
    }

    pub(crate) fn from_thrift(tc: thrift::changeset_info::ChangesetInfo) -> Result<Self> {
        let catch_block = || -> Result<_> {
            Ok(ChangesetInfo {
                changeset_id: ChangesetId::from_thrift(tc.changeset_id)?,
                parents: tc
                    .parents
                    .into_iter()
                    .map(ChangesetId::from_thrift)
                    .collect::<Result<_>>()?,
                author: tc.author,
                author_date: DateTime::from_thrift(tc.author_date)?,
                committer: tc.committer,
                committer_date: match tc.committer_date {
                    Some(dt) => Some(DateTime::from_thrift(dt)?),
                    None => None,
                },
                message: ChangesetMessage::from_thrift(tc.message)?,
                hg_extra: tc.hg_extra,
                git_extra_headers: tc
                    .git_extra_headers
                    .map(|extra| extra.into_iter().map(|(k, v)| (k.0, v)).collect()),
                subtree_change_count: tc.subtree_change_count.map_or(0, |count| count as usize),
            })
        };

        catch_block().with_context(|| {
            MononokeTypeError::InvalidThrift(
                "ChangesetInfo".into(),
                "Invalid changeset info".into(),
            )
        })
    }

    pub fn into_thrift(self) -> thrift::changeset_info::ChangesetInfo {
        thrift::changeset_info::ChangesetInfo {
            changeset_id: self.changeset_id.into_thrift(),
            parents: self
                .parents
                .into_iter()
                .map(|parent| parent.into_thrift())
                .collect(),
            author: self.author,
            author_date: self.author_date.into_thrift(),
            committer: self.committer,
            committer_date: self.committer_date.map(|dt| dt.into_thrift()),
            message: self.message.into_thrift(),
            hg_extra: self.hg_extra,
            git_extra_headers: self.git_extra_headers.map(|extra| {
                extra
                    .into_iter()
                    .map(|(k, v)| (thrift::data::SmallBinary(k), v))
                    .collect()
            }),
            subtree_change_count: Some(self.subtree_change_count as i64).filter(|count| *count > 0),
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(bytes)
            .with_context(|| MononokeTypeError::BlobDeserializeError("ChangesetInfo".into()))?;
        Self::from_thrift(thrift_tc)
    }
}

impl TryFrom<BlobstoreBytes> for ChangesetInfo {
    type Error = Error;

    fn try_from(blob_bytes: BlobstoreBytes) -> Result<Self> {
        ChangesetInfo::from_bytes(&blob_bytes.into_bytes())
    }
}

impl TryFrom<BlobstoreGetData> for ChangesetInfo {
    type Error = Error;

    fn try_from(blob_get_data: BlobstoreGetData) -> Result<Self> {
        blob_get_data.into_bytes().try_into()
    }
}

impl From<ChangesetInfo> for BlobstoreBytes {
    fn from(info: ChangesetInfo) -> BlobstoreBytes {
        let data = compact_protocol::serialize(info.into_thrift());
        BlobstoreBytes::from_bytes(data)
    }
}

/// Given a commit message returns the commit title: either the first line of the
/// message or the message itself, cropped by the DEFAULT_TITLE_LENGTH number of
/// characters.
fn get_title(message: &str) -> &str {
    // either first line or the whole message
    let title = message.trim_start().lines().next().unwrap_or("");
    match title.grapheme_indices(true).nth(DEFAULT_TITLE_LENGTH) {
        Some((i, _ch)) => &title[..i],
        None => title,
    }
}

#[cfg(test)]
mod test {
    use mononoke_macros::mononoke;
    use mononoke_types::BonsaiChangeset;
    use mononoke_types::BonsaiChangesetMut;
    use mononoke_types::DateTime;
    use mononoke_types::FileChange;
    use mononoke_types::NonRootMPath;
    use sorted_vector_map::sorted_vector_map;

    use super::*;

    #[mononoke::fbinit_test]
    fn changeset_info_title_test() {
        {
            let bcs = create_bonsai_with_message("   \n\n  title \n\n summary\n");
            let info = ChangesetInfo::new(bcs.get_changeset_id(), bcs);

            check_info(&info, "title ");
        };

        {
            let bcs = create_bonsai_with_message("   \n  title - summary");
            let info = ChangesetInfo::new(bcs.get_changeset_id(), bcs);

            check_info(&info, "title - summary");
        };

        {
            let bcs = create_bonsai_with_message("  no title - no new lines ");
            let info = ChangesetInfo::new(bcs.get_changeset_id(), bcs);

            check_info(&info, "no title - no new lines ");
        };

        {
            let bcs = create_bonsai_with_message("  \n\n ");
            let info = ChangesetInfo::new(bcs.get_changeset_id(), bcs);

            check_info(&info, "");
        };
    }

    fn check_info(info: &ChangesetInfo, title: &str) {
        assert_eq!(info.title(), title);
    }

    fn create_bonsai_with_message(message: &str) -> BonsaiChangeset {
        BonsaiChangesetMut {
            parents: vec![],
            author: "author".to_string(),
            author_date: DateTime::now(),
            message: message.to_string(),
            git_extra_headers: Some(sorted_vector_map! { SmallVec::new() => Bytes::from_static(b"world")}),
            file_changes: sorted_vector_map! { NonRootMPath::new("file").unwrap() => FileChange::Deletion },
            ..Default::default()
        }
        .freeze()
        .unwrap()
    }
}
