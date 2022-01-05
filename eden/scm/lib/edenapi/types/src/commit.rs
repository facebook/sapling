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
use serde_derive::Deserialize;
use serde_derive::Serialize;
use type_macros::auto_wire;
use types::hgid::HgId;
use types::Parents;
use types::RepoPathBuf;

use crate::BonsaiChangesetId;
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
pub struct CommitLocationToHashRequest {
    #[id(1)]
    pub location: Location<HgId>,
    #[id(2)]
    pub count: u64,
}

#[auto_wire]
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
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
pub struct CommitLocationToHashRequestBatch {
    #[id(1)]
    pub requests: Vec<CommitLocationToHashRequest>,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitLocationToHashRequest {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        CommitLocationToHashRequest {
            location: Arbitrary::arbitrary(g),
            count: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitLocationToHashResponse {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        CommitLocationToHashResponse {
            location: Arbitrary::arbitrary(g),
            count: Arbitrary::arbitrary(g),
            hgids: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitLocationToHashRequestBatch {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        CommitLocationToHashRequestBatch {
            requests: Arbitrary::arbitrary(g),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct CommitHashToLocationRequestBatch {
    pub master_heads: Vec<HgId>,
    pub hgids: Vec<HgId>,
    pub unfiltered: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[derive(Serialize)] // used to convert to Python
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
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[derive(Serialize)]
pub struct CommitGraphEntry {
    #[id(1)]
    pub hgid: HgId,
    #[id(2)]
    pub parents: Vec<HgId>,
}

#[auto_wire]
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct CommitGraphRequest {
    #[id(1)]
    pub common: Vec<HgId>,
    #[id(2)]
    pub heads: Vec<HgId>,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitGraphEntry {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        CommitGraphEntry {
            hgid: Arbitrary::arbitrary(g),
            parents: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitGraphRequest {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        CommitGraphRequest {
            common: Arbitrary::arbitrary(g),
            heads: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitHashToLocationRequestBatch {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        CommitHashToLocationRequestBatch {
            master_heads: Arbitrary::arbitrary(g),
            hgids: Arbitrary::arbitrary(g),
            unfiltered: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitHashToLocationResponse {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        CommitHashToLocationResponse {
            hgid: Arbitrary::arbitrary(g),
            result: Arbitrary::arbitrary(g),
        }
    }
}

/// The list of Mercurial commit identifiers for which we want the commit data to be returned.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct CommitRevlogDataRequest {
    pub hgids: Vec<HgId>,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitRevlogDataRequest {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        CommitRevlogDataRequest {
            hgids: Arbitrary::arbitrary(g),
        }
    }
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

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitHashLookupRequest {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        CommitHashLookupRequest::InclusiveRange(Arbitrary::arbitrary(g), Arbitrary::arbitrary(g))
    }
}

/// Commit hashes that are known to the server in the described range.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct CommitHashLookupResponse {
    pub request: CommitHashLookupRequest,
    pub hgids: Vec<HgId>,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitHashLookupResponse {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        CommitHashLookupResponse {
            request: Arbitrary::arbitrary(g),
            hgids: Arbitrary::arbitrary(g),
        }
    }
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct Extra {
    #[id(1)]
    pub key: Vec<u8>,
    #[id(2)]
    pub value: Vec<u8>,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
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
pub struct UploadHgChangeset {
    #[id(1)]
    pub node_id: HgId,
    #[id(2)]
    pub changeset_content: HgChangesetContent,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
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
pub struct UploadHgChangesetsRequest {
    /// list of changesets to upload, changesets must be sorted topologically (use dag.sort)
    #[id(1)]
    pub changesets: Vec<UploadHgChangeset>,
    /// list of mutation entries for the uploading changesets
    #[id(2)]
    pub mutations: Vec<HgMutationEntryContent>,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for HgMutationEntryContent {
    fn arbitrary(g: &mut Gen) -> Self {
        Self {
            successor: Arbitrary::arbitrary(g),
            predecessors: Arbitrary::arbitrary(g),
            split: Arbitrary::arbitrary(g),
            op: Arbitrary::arbitrary(g),
            user: Arbitrary::arbitrary(g),
            time: Arbitrary::arbitrary(g),
            tz: Arbitrary::arbitrary(g),
            extras: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for UploadHgChangesetsRequest {
    fn arbitrary(g: &mut Gen) -> Self {
        Self {
            changesets: Arbitrary::arbitrary(g),
            mutations: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for UploadHgChangeset {
    fn arbitrary(g: &mut Gen) -> Self {
        Self {
            node_id: Arbitrary::arbitrary(g),
            changeset_content: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for HgChangesetContent {
    fn arbitrary(g: &mut Gen) -> Self {
        Self {
            parents: Arbitrary::arbitrary(g),
            manifestid: Arbitrary::arbitrary(g),
            user: Arbitrary::arbitrary(g),
            time: Arbitrary::arbitrary(g),
            tz: Arbitrary::arbitrary(g),
            extras: Arbitrary::arbitrary(g),
            files: Arbitrary::arbitrary(g),
            message: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for Extra {
    fn arbitrary(g: &mut Gen) -> Self {
        Self {
            key: Arbitrary::arbitrary(g),
            value: Arbitrary::arbitrary(g),
        }
    }
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
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
pub struct UploadBonsaiChangesetRequest {
    /// changeset to upload
    #[id(1)]
    pub changeset: BonsaiChangesetContent,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for BonsaiChangesetContent {
    fn arbitrary(g: &mut Gen) -> Self {
        Self {
            hg_parents: Arbitrary::arbitrary(g),
            author: Arbitrary::arbitrary(g),
            time: Arbitrary::arbitrary(g),
            tz: Arbitrary::arbitrary(g),
            extra: Arbitrary::arbitrary(g),
            file_changes: Arbitrary::arbitrary(g),
            message: Arbitrary::arbitrary(g),
            is_snapshot: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for UploadBonsaiChangesetRequest {
    fn arbitrary(g: &mut Gen) -> Self {
        Self {
            changeset: Arbitrary::arbitrary(g),
        }
    }
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
pub struct FetchSnapshotRequest {
    #[id(1)]
    pub cs_id: BonsaiChangesetId,
}

#[auto_wire]
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
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
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct UploadSnapshotResponse {
    pub changeset_token: UploadToken,
    pub bubble_id: NonZeroU64,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct EphemeralPrepareRequest {
    #[id(1)]
    pub custom_duration_secs: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct EphemeralPrepareResponse {
    pub bubble_id: NonZeroU64,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for EphemeralPrepareRequest {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        EphemeralPrepareRequest {
            custom_duration_secs: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for EphemeralPrepareResponse {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        EphemeralPrepareResponse {
            bubble_id: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for BonsaiExtra {
    fn arbitrary(g: &mut Gen) -> Self {
        Self {
            key: Arbitrary::arbitrary(g),
            value: Arbitrary::arbitrary(g),
        }
    }
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

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FetchSnapshotRequest {
    fn arbitrary(g: &mut Gen) -> Self {
        Self {
            cs_id: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FetchSnapshotResponse {
    fn arbitrary(g: &mut Gen) -> Self {
        Self {
            author: Arbitrary::arbitrary(g),
            time: Arbitrary::arbitrary(g),
            tz: Arbitrary::arbitrary(g),
            hg_parents: Arbitrary::arbitrary(g),
            file_changes: Arbitrary::arbitrary(g),
        }
    }
}

#[auto_wire]
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct CommitMutationsRequest {
    #[id(1)]
    pub commits: Vec<HgId>,
}

#[auto_wire]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommitMutationsResponse {
    #[id(1)]
    pub mutation: HgMutationEntryContent,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitMutationsRequest {
    fn arbitrary(g: &mut Gen) -> Self {
        Self {
            commits: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitMutationsResponse {
    fn arbitrary(g: &mut Gen) -> Self {
        Self {
            mutation: Arbitrary::arbitrary(g),
        }
    }
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
