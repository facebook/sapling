/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::iter;
use std::num::NonZeroU64;

use anyhow::Result;
use bytes::Bytes;
use dag_types::Location;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Gen;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck_arbitrary_derive::Arbitrary;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use type_macros::auto_wire;
use types::hgid::HgId;
use types::Parents;
use types::RepoPathBuf;

use crate::BonsaiChangesetId;
use crate::CommitId;
use crate::CommitIdScheme;
use crate::FileType;
use crate::ServerError;
use crate::UploadToken;

/// Given a graph location, return `count` hashes following first parent links.
///
/// Example:
/// 0 - a - b - c
/// In this example our initial commit is `0`, then we have `a` the first commit, `b` second,
/// `c` third.
/// {
///   location: {
///     descendant: c,
///     distance: 1,
///   }
///   count: 2,
/// }
/// => [b, a]
#[auto_wire]
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct CommitLocationToHashRequest {
    #[id(1)]
    pub location: Location<HgId>,
    #[id(2)]
    pub count: u64,
}

#[auto_wire]
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct CommitLocationToHashResponse {
    #[id(1)]
    pub location: Location<HgId>,
    #[id(2)]
    pub count: u64,
    #[id(3)]
    pub hgids: Vec<HgId>,
}

#[auto_wire]
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct CommitLocationToHashRequestBatch {
    #[id(1)]
    pub requests: Vec<CommitLocationToHashRequest>,
}

#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct CommitHashToLocationRequestBatch {
    pub master_heads: Vec<HgId>,
    pub hgids: Vec<HgId>,
    pub unfiltered: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[derive(Serialize)] // used to convert to Python
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct CommitHashToLocationResponse {
    pub hgid: HgId,
    pub result: Result<Option<Location<HgId>>, ServerError>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[derive(Serialize)]
pub struct CommitKnownResponse {
    pub hgid: HgId,
    /// `Ok(true)`: The server verified that `hgid` is known.
    /// `Ok(false)`: The server does not known `hgid`.
    /// `Err`: The server cannot check `hgid`.
    pub known: Result<bool, ServerError>,
}

#[auto_wire]
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct CommitGraphEntry {
    #[id(1)]
    pub hgid: HgId,
    #[id(2)]
    pub parents: Vec<HgId>,
}

#[auto_wire]
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct CommitGraphRequest {
    #[id(1)]
    pub common: Vec<HgId>,
    #[id(2)]
    pub heads: Vec<HgId>,
}

/// The list of Mercurial commit identifiers for which we want the commit data to be returned.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct CommitRevlogDataRequest {
    pub hgids: Vec<HgId>,
}

/// A mercurial commit entry as it was serialized in the revlog.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct CommitRevlogData {
    #[serde(with = "types::serde_with::hgid::bytes")]
    pub hgid: HgId,
    pub revlog_data: Bytes,
}

impl CommitRevlogData {
    pub fn new(hgid: HgId, revlog_data: Bytes) -> Self {
        Self { hgid, revlog_data }
    }
}

/// Request commit hashes that fall in the [low..high] range.
// Note: limit is implied to be 10.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub enum CommitHashLookupRequest {
    InclusiveRange(HgId, HgId),
}

/// make a hash lookup request from a hash prefix for the range of hashes that
/// could match the hash prefix
/// ex: the range 3f54ab22d87423966e0000000000000000000000:3f54ab22d87423966effffffffffffffffffffff
///     matches the prefix 3f54ab22d87423966e
pub fn make_hash_lookup_request(prefix: String) -> Result<CommitHashLookupRequest, anyhow::Error> {
    let suffix_len = HgId::hex_len() - prefix.len();
    let low_hex = prefix
        .clone()
        .chars()
        .chain(iter::repeat('0').take(suffix_len))
        .collect::<String>();
    let low_id = HgId::from_hex(low_hex.as_bytes())?;
    let high_hex = prefix
        .chars()
        .chain(iter::repeat('f').take(suffix_len))
        .collect::<String>();
    let high_id = HgId::from_hex(high_hex.as_bytes())?;
    Ok(CommitHashLookupRequest::InclusiveRange(low_id, high_id))
}

/// Commit hashes that are known to the server in the described range.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct CommitHashLookupResponse {
    pub request: CommitHashLookupRequest,
    pub hgids: Vec<HgId>,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct Extra {
    #[id(1)]
    pub key: Vec<u8>,
    #[id(2)]
    pub value: Vec<u8>,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct HgChangesetContent {
    #[id(1)]
    pub parents: Parents,
    #[id(2)]
    pub manifestid: HgId,
    #[id(3)]
    pub user: Vec<u8>,
    #[id(4)]
    pub time: i64,
    #[id(5)]
    pub tz: i32,
    #[id(6)]
    pub extras: Vec<Extra>,
    #[id(7)]
    pub files: Vec<RepoPathBuf>,
    #[id(8)]
    pub message: Vec<u8>,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct UploadHgChangeset {
    #[id(1)]
    pub node_id: HgId,
    #[id(2)]
    pub changeset_content: HgChangesetContent,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct HgMutationEntryContent {
    #[id(1)]
    pub successor: HgId,
    #[id(2)]
    pub predecessors: Vec<HgId>,
    #[id(3)]
    pub split: Vec<HgId>,
    #[id(4)]
    pub op: String,
    #[id(5)]
    pub user: Vec<u8>,
    #[id(6)]
    pub time: i64,
    #[id(7)]
    pub tz: i32,
    #[id(8)]
    pub extras: Vec<Extra>,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct UploadHgChangesetsRequest {
    /// list of changesets to upload, changesets must be sorted topologically (use dag.sort)
    #[id(1)]
    pub changesets: Vec<UploadHgChangeset>,
    /// list of mutation entries for the uploading changesets
    #[id(2)]
    pub mutations: Vec<HgMutationEntryContent>,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct BonsaiExtra {
    #[id(1)]
    pub key: String,
    #[id(2)]
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub enum BonsaiFileChange {
    Change {
        /// Token proving the file was uploaded, and containing its content id and size
        upload_token: UploadToken,
        file_type: FileType,
    },
    Deletion,
    UntrackedChange {
        upload_token: UploadToken,
        file_type: FileType,
    },
    UntrackedDeletion,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct BonsaiChangesetContent {
    pub hg_parents: Parents,
    pub author: String,
    pub time: i64,
    pub tz: i32,
    pub extra: Vec<BonsaiExtra>,
    pub file_changes: Vec<(RepoPathBuf, BonsaiFileChange)>,
    pub message: String,
    pub is_snapshot: bool,
}

#[auto_wire]
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct UploadBonsaiChangesetRequest {
    /// changeset to upload
    #[id(1)]
    pub changeset: BonsaiChangesetContent,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct SnapshotRawFiles {
    /// Absolute root of the repository, where all files are
    /// relative to. Can be different from cwd.
    pub root: RepoPathBuf,
    /// Tracked files modified in local changes
    pub modified: Vec<(RepoPathBuf, FileType)>,
    /// Files added with "hg add"
    pub added: Vec<(RepoPathBuf, FileType)>,
    /// Files that are not tracked but are in the local changes
    pub untracked: Vec<(RepoPathBuf, FileType)>,
    /// Files removed with "hg rm"
    pub removed: Vec<RepoPathBuf>,
    /// Files that are not in the local changes but were tracked
    pub missing: Vec<RepoPathBuf>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct SnapshotRawData {
    pub hg_parents: Parents,
    pub files: SnapshotRawFiles,
    pub time: i64,
    pub tz: i32,
    pub author: String,
}

#[auto_wire]
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct FetchSnapshotRequest {
    #[id(1)]
    pub cs_id: BonsaiChangesetId,
}

#[auto_wire]
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct FetchSnapshotResponse {
    #[id(1)]
    pub hg_parents: Parents,
    #[id(2)]
    pub file_changes: Vec<(RepoPathBuf, BonsaiFileChange)>,
    #[id(3)]
    pub author: String,
    #[id(4)]
    pub time: i64,
    #[id(5)]
    pub tz: i32,
    #[id(6)]
    pub bubble_id: Option<NonZeroU64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct UploadSnapshotResponse {
    pub changeset_token: UploadToken,
    pub bubble_id: NonZeroU64,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct EphemeralPrepareRequest {
    #[id(1)]
    pub custom_duration_secs: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct EphemeralPrepareResponse {
    pub bubble_id: NonZeroU64,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for BonsaiFileChange {
    fn arbitrary(g: &mut Gen) -> Self {
        match u64::arbitrary(g) % 100 {
            0..=49 => Self::Change {
                upload_token: Arbitrary::arbitrary(g),
                file_type: Arbitrary::arbitrary(g),
            },
            50..=79 => Self::Deletion,
            80..=94 => Self::UntrackedChange {
                upload_token: Arbitrary::arbitrary(g),
                file_type: Arbitrary::arbitrary(g),
            },
            95..=99 => Self::UntrackedDeletion,
            _ => unreachable!(),
        }
    }
}

#[auto_wire]
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct CommitMutationsRequest {
    #[id(1)]
    pub commits: Vec<HgId>,
}

#[auto_wire]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct CommitMutationsResponse {
    #[id(1)]
    pub mutation: HgMutationEntryContent,
}

#[auto_wire]
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct CommitTranslateIdRequest {
    #[id(1)]
    pub commits: Vec<CommitId>,
    #[id(2)]
    pub scheme: CommitIdScheme,
}

#[auto_wire]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct CommitTranslateIdResponse {
    #[id(1)]
    pub commit: CommitId,
    #[id(2)]
    pub translated: CommitId,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_range(start: &[u8], end: &[u8]) -> Result<CommitHashLookupRequest> {
        let start_id = HgId::from_hex(start)?;
        let end_id = HgId::from_hex(end)?;
        Ok(CommitHashLookupRequest::InclusiveRange(start_id, end_id))
    }

    #[test]
    fn test_make_hash_lookup_request() -> Result<()> {
        let short_hash = String::from("3f54ab22d87423966e");
        let prefix_req = make_hash_lookup_request(short_hash)?;
        let expected_req = make_range(
            b"3f54ab22d87423966e0000000000000000000000",
            b"3f54ab22d87423966effffffffffffffffffffff",
        )?;
        assert_eq!(prefix_req, expected_req);

        let empty_hash = String::new();
        let empty_req = make_hash_lookup_request(empty_hash)?;
        let expected_empty_req = make_range(
            b"0000000000000000000000000000000000000000",
            b"ffffffffffffffffffffffffffffffffffffffff",
        )?;
        assert_eq!(empty_req, expected_empty_req);

        let full_hash = String::from("3f54ab22d87423966e27374e6c0f8112a269999b");
        let full_req = make_hash_lookup_request(full_hash)?;
        let expected_full_req = make_range(
            b"3f54ab22d87423966e27374e6c0f8112a269999b",
            b"3f54ab22d87423966e27374e6c0f8112a269999b",
        )?;
        assert_eq!(full_req, expected_full_req);

        Ok(())
    }
}
