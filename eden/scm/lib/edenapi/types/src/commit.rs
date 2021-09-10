/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use bytes::Bytes;
use dag_types::Location;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use serde_derive::{Deserialize, Serialize};
use std::iter;
use std::num::NonZeroU64;
use types::{hgid::HgId, Parents, RepoPathBuf};

use crate::{BonsaiChangesetId, FileType, ServerError, UploadToken};

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
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct CommitLocationToHashRequest {
    pub location: Location<HgId>,
    pub count: u64,
}

#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct CommitLocationToHashResponse {
    pub location: Location<HgId>,
    pub count: u64,
    pub hgids: Vec<HgId>,
}

#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct CommitLocationToHashRequestBatch {
    pub requests: Vec<CommitLocationToHashRequest>,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitLocationToHashRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        CommitLocationToHashRequest {
            location: Arbitrary::arbitrary(g),
            count: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitLocationToHashResponse {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        CommitLocationToHashResponse {
            location: Arbitrary::arbitrary(g),
            count: Arbitrary::arbitrary(g),
            hgids: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitLocationToHashRequestBatch {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
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

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[derive(Serialize)]
pub struct CommitGraphEntry {
    pub hgid: HgId,
    pub parents: Vec<HgId>,
}

#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct CommitGraphRequest {
    pub common: Vec<HgId>,
    pub heads: Vec<HgId>,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitGraphEntry {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        CommitGraphEntry {
            hgid: Arbitrary::arbitrary(g),
            parents: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitGraphRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        CommitGraphRequest {
            common: Arbitrary::arbitrary(g),
            heads: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitHashToLocationRequestBatch {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        CommitHashToLocationRequestBatch {
            master_heads: Arbitrary::arbitrary(g),
            hgids: Arbitrary::arbitrary(g),
            unfiltered: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitHashToLocationResponse {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
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
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
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
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
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
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        CommitHashLookupResponse {
            request: Arbitrary::arbitrary(g),
            hgids: Arbitrary::arbitrary(g),
        }
    }
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct Extra {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct HgChangesetContent {
    pub parents: Parents,
    pub manifestid: HgId,
    pub user: Vec<u8>,
    pub time: i64,
    pub tz: i32,
    pub extras: Vec<Extra>,
    pub files: Vec<RepoPathBuf>,
    pub message: Vec<u8>,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct UploadHgChangeset {
    pub node_id: HgId,
    pub changeset_content: HgChangesetContent,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct HgMutationEntryContent {
    pub successor: HgId,
    pub predecessors: Vec<HgId>,
    pub split: Vec<HgId>,
    pub op: String,
    pub user: Vec<u8>,
    pub time: i64,
    pub tz: i32,
    pub extras: Vec<Extra>,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct UploadHgChangesetsRequest {
    /// list of changesets to upload, changesets must be sorted topologically (use dag.sort)
    pub changesets: Vec<UploadHgChangeset>,
    /// list of mutation entries for the uploading changesets
    pub mutations: Vec<HgMutationEntryContent>,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct BonsaiExtra {
    pub key: String,
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

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct UploadBonsaiChangesetRequest {
    /// changeset to upload
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

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct FetchSnapshotRequest {
    pub cs_id: BonsaiChangesetId,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct FetchSnapshotResponse {
    pub hg_parents: Parents,
    pub file_changes: Vec<(RepoPathBuf, BonsaiFileChange)>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct UploadSnapshotResponse {
    pub changeset_token: UploadToken,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct EphemeralPrepareRequest {}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct EphemeralPrepareResponse {
    pub bubble_id: NonZeroU64,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for EphemeralPrepareRequest {
    fn arbitrary<G: quickcheck::Gen>(_g: &mut G) -> Self {
        EphemeralPrepareRequest {}
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for EphemeralPrepareResponse {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        EphemeralPrepareResponse {
            bubble_id: Arbitrary::arbitrary(g),
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
        let expected__full_req = make_range(
            b"3f54ab22d87423966e27374e6c0f8112a269999b",
            b"3f54ab22d87423966e27374e6c0f8112a269999b",
        )?;
        assert_eq!(full_req, expected__full_req);

        Ok(())
    }
}
