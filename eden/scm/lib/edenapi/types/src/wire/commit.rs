/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU64;

use dag_types::Location;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use types::HgId;

use crate::commit::BonsaiChangesetContent;
use crate::commit::BonsaiFileChange;
use crate::commit::CommitHashLookupRequest;
use crate::commit::CommitHashLookupResponse;
use crate::commit::CommitHashToLocationRequestBatch;
use crate::commit::CommitHashToLocationResponse;
use crate::commit::EphemeralPrepareResponse;
pub use crate::commit::WireBonsaiExtra;
pub use crate::commit::WireCommitGraphEntry;
pub use crate::commit::WireCommitGraphRequest;
pub use crate::commit::WireCommitLocationToHashRequest;
pub use crate::commit::WireCommitLocationToHashRequestBatch;
pub use crate::commit::WireCommitLocationToHashResponse;
pub use crate::commit::WireCommitMutationsRequest;
pub use crate::commit::WireCommitMutationsResponse;
pub use crate::commit::WireEphemeralPrepareRequest;
pub use crate::commit::WireExtra;
pub use crate::commit::WireFetchSnapshotRequest;
pub use crate::commit::WireFetchSnapshotResponse;
pub use crate::commit::WireHgChangesetContent;
pub use crate::commit::WireHgMutationEntryContent;
pub use crate::commit::WireUploadBonsaiChangesetRequest;
pub use crate::commit::WireUploadHgChangeset;
pub use crate::commit::WireUploadHgChangesetsRequest;
use crate::wire::is_default;
use crate::wire::ToApi;
use crate::wire::ToWire;
use crate::wire::WireFileType;
use crate::wire::WireHgId;
use crate::wire::WireParents;
use crate::wire::WireRepoPathBuf;
use crate::wire::WireResult;
use crate::wire::WireToApiConversionError;
use crate::wire::WireUploadToken;

#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireCommitLocation {
    #[serde(rename = "1")]
    pub descendant: WireHgId,
    #[serde(rename = "2")]
    pub distance: u64,
}

impl ToWire for Location<HgId> {
    type Wire = WireCommitLocation;

    fn to_wire(self) -> Self::Wire {
        Self::Wire {
            descendant: self.descendant.to_wire(),
            distance: self.distance,
        }
    }
}

impl ToApi for WireCommitLocation {
    type Api = Location<HgId>;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        let api = Self::Api {
            descendant: self.descendant.to_api()?,
            distance: self.distance,
        };
        Ok(api)
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireCommitLocation {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        Location::arbitrary(g).to_wire()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireCommitHashToLocationRequestBatch {
    #[serde(rename = "1")]
    pub client_head: Option<WireHgId>,
    #[serde(rename = "2")]
    pub hgids: Vec<WireHgId>,
    #[serde(rename = "3", default)]
    pub master_heads: Vec<WireHgId>,
    #[serde(rename = "4")]
    pub unfiltered: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireCommitHashToLocationResponse {
    #[serde(rename = "1")]
    pub hgid: WireHgId,
    #[serde(rename = "2")]
    pub location: Option<WireCommitLocation>,
    #[serde(rename = "3")]
    pub result: Option<WireResult<Option<WireCommitLocation>>>,
}

impl ToWire for CommitHashToLocationRequestBatch {
    type Wire = WireCommitHashToLocationRequestBatch;

    fn to_wire(self) -> Self::Wire {
        let client_head = self.master_heads.get(0).copied().to_wire();
        Self::Wire {
            client_head,
            hgids: self.hgids.to_wire(),
            master_heads: self.master_heads.to_wire(),
            unfiltered: self.unfiltered,
        }
    }
}

impl ToApi for WireCommitHashToLocationRequestBatch {
    type Api = CommitHashToLocationRequestBatch;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        let mut master_heads = self.master_heads.to_api()?;
        if master_heads.is_empty() {
            let client_head = self.client_head.to_api()?;
            if let Some(head) = client_head {
                master_heads = vec![head];
            }
        }
        let api = Self::Api {
            master_heads,
            hgids: self.hgids.to_api()?,
            unfiltered: self.unfiltered,
        };
        Ok(api)
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireCommitHashToLocationRequestBatch {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        CommitHashToLocationRequestBatch::arbitrary(g).to_wire()
    }
}

impl ToWire for CommitHashToLocationResponse {
    type Wire = WireCommitHashToLocationResponse;

    fn to_wire(self) -> Self::Wire {
        let location = match self.result {
            Ok(Some(x)) => Some(x.to_wire()),
            _ => None,
        };
        Self::Wire {
            hgid: self.hgid.to_wire(),
            location,
            result: Some(self.result.to_wire()),
        }
    }
}

impl ToApi for WireCommitHashToLocationResponse {
    type Api = CommitHashToLocationResponse;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        let result = match self.result {
            Some(x) => x.to_api()?,
            None => match self.location {
                None => Ok(None),
                Some(l) => Ok(Some(l.to_api()?)),
            },
        };
        let api = Self::Api {
            hgid: self.hgid.to_api()?,
            result,
        };
        Ok(api)
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireCommitHashToLocationResponse {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        CommitHashToLocationResponse::arbitrary(g).to_wire()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireCommitHashLookupRequest {
    #[serde(rename = "1", default, skip_serializing_if = "is_default")]
    pub inclusive_range: Option<(WireHgId, WireHgId)>,
}

impl ToWire for CommitHashLookupRequest {
    type Wire = WireCommitHashLookupRequest;

    fn to_wire(self) -> Self::Wire {
        use crate::CommitHashLookupRequest::*;
        match self {
            InclusiveRange(low, high) => WireCommitHashLookupRequest {
                inclusive_range: Some((low.to_wire(), high.to_wire())),
            },
        }
    }
}

impl ToApi for WireCommitHashLookupRequest {
    type Api = CommitHashLookupRequest;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        let ir =
            self.inclusive_range
                .ok_or(WireToApiConversionError::CannotPopulateRequiredField(
                    "inclusive_range",
                ))?;
        let api = CommitHashLookupRequest::InclusiveRange(ir.0.to_api()?, ir.1.to_api()?);
        Ok(api)
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireCommitHashLookupRequest {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        CommitHashLookupRequest::arbitrary(g).to_wire()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireCommitHashLookupResponse {
    #[serde(rename = "1")]
    pub request: Option<WireCommitHashLookupRequest>,
    #[serde(rename = "2", default, skip_serializing_if = "is_default")]
    pub hgids: Option<Vec<WireHgId>>,
}

impl ToWire for CommitHashLookupResponse {
    type Wire = WireCommitHashLookupResponse;

    fn to_wire(self) -> Self::Wire {
        Self::Wire {
            request: Some(self.request.to_wire()),
            hgids: Some(self.hgids.to_wire()),
        }
    }
}

impl ToApi for WireCommitHashLookupResponse {
    type Api = CommitHashLookupResponse;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        let request = self
            .request
            .ok_or(WireToApiConversionError::CannotPopulateRequiredField(
                "request",
            ))?
            .to_api()?;
        let hgids = self
            .hgids
            .ok_or(WireToApiConversionError::CannotPopulateRequiredField(
                "hgids",
            ))?
            .to_api()?;
        let api = Self::Api { request, hgids };
        Ok(api)
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireCommitHashLookupResponse {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        CommitHashLookupResponse::arbitrary(g).to_wire()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub enum WireBonsaiFileChange {
    #[serde(rename = "1")]
    Change(WireUploadToken, WireFileType),

    #[serde(rename = "2")]
    UntrackedChange(WireUploadToken, WireFileType),

    #[serde(rename = "3")]
    UntrackedDeletion,

    #[serde(rename = "4")]
    Deletion,

    #[serde(other, rename = "0")]
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireSnapshotState {}

#[derive(Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireBonsaiChangesetContent {
    #[serde(rename = "1")]
    pub hg_parents: WireParents,

    #[serde(rename = "2")]
    pub author: String,

    #[serde(rename = "3")]
    pub time: i64,

    #[serde(rename = "4")]
    pub tz: i32,

    #[serde(rename = "5")]
    pub extra: Vec<WireBonsaiExtra>,

    #[serde(rename = "6")]
    pub file_changes: Vec<(WireRepoPathBuf, WireBonsaiFileChange)>,

    #[serde(rename = "7")]
    pub message: String,

    #[serde(rename = "8")]
    pub snapshot_state: Option<WireSnapshotState>,
}

impl ToWire for BonsaiFileChange {
    type Wire = WireBonsaiFileChange;
    fn to_wire(self) -> Self::Wire {
        match self {
            Self::Change {
                upload_token,
                file_type,
            } => WireBonsaiFileChange::Change(upload_token.to_wire(), file_type.to_wire()),
            Self::UntrackedChange {
                upload_token,
                file_type,
            } => WireBonsaiFileChange::UntrackedChange(upload_token.to_wire(), file_type.to_wire()),
            Self::UntrackedDeletion => WireBonsaiFileChange::UntrackedDeletion,
            Self::Deletion => WireBonsaiFileChange::Deletion,
        }
    }
}

impl ToWire for BonsaiChangesetContent {
    type Wire = WireBonsaiChangesetContent;

    fn to_wire(self) -> Self::Wire {
        WireBonsaiChangesetContent {
            hg_parents: self.hg_parents.to_wire(),
            author: self.author,
            time: self.time,
            tz: self.tz,
            extra: self.extra.to_wire(),
            file_changes: self
                .file_changes
                .into_iter()
                .map(|(a, b)| (a.to_wire(), b.to_wire()))
                .collect(),
            message: self.message,
            snapshot_state: self.is_snapshot.then(|| WireSnapshotState {}),
        }
    }
}

impl ToApi for WireBonsaiFileChange {
    type Api = BonsaiFileChange;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        match self {
            Self::Change(upload_token, file_type) => Ok(BonsaiFileChange::Change {
                upload_token: upload_token.to_api()?,
                file_type: file_type.to_api()?,
            }),
            Self::UntrackedChange(upload_token, file_type) => {
                Ok(BonsaiFileChange::UntrackedChange {
                    upload_token: upload_token.to_api()?,
                    file_type: file_type.to_api()?,
                })
            }
            Self::UntrackedDeletion => Ok(BonsaiFileChange::UntrackedDeletion),
            Self::Deletion => Ok(BonsaiFileChange::Deletion),
            Self::Unknown => Err(WireToApiConversionError::UnrecognizedEnumVariant(
                "WireBonsaiFileChange",
            )),
        }
    }
}

impl ToApi for WireBonsaiChangesetContent {
    type Api = BonsaiChangesetContent;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(BonsaiChangesetContent {
            hg_parents: self.hg_parents.to_api()?,
            author: self.author,
            time: self.time,
            tz: self.tz,
            extra: self.extra.to_api()?,
            file_changes: self
                .file_changes
                .into_iter()
                .map(|(a, b)| Ok((a.to_api()?, b.to_api()?)))
                .collect::<Result<_, Self::Error>>()?,
            message: self.message,
            is_snapshot: self.snapshot_state.is_some(),
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireEphemeralPrepareResponse {
    #[serde(rename = "1")]
    pub bubble_id: Option<NonZeroU64>,
}

impl ToWire for EphemeralPrepareResponse {
    type Wire = WireEphemeralPrepareResponse;

    fn to_wire(self) -> Self::Wire {
        Self::Wire {
            bubble_id: Some(self.bubble_id),
        }
    }
}

impl ToApi for WireEphemeralPrepareResponse {
    type Api = EphemeralPrepareResponse;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(Self::Api {
            bubble_id: self.bubble_id.ok_or(
                WireToApiConversionError::CannotPopulateRequiredField("bubble_id"),
            )?,
        })
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireEphemeralPrepareResponse {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        EphemeralPrepareResponse::arbitrary(g).to_wire()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::tests::auto_wire_tests;

    auto_wire_tests!(
        WireCommitLocation,
        WireCommitLocationToHashRequest,
        WireCommitLocationToHashResponse,
        WireCommitLocationToHashRequestBatch,
        WireCommitHashToLocationRequestBatch,
        WireCommitHashToLocationResponse,
        WireCommitHashLookupRequest,
        WireCommitHashLookupResponse,
        WireEphemeralPrepareRequest,
        WireEphemeralPrepareResponse,
        WireCommitGraphRequest,
        WireUploadHgChangeset,
        WireUploadHgChangesetsRequest,
        WireFetchSnapshotRequest,
        WireFetchSnapshotResponse,
        WireCommitMutationsRequest,
        WireCommitMutationsResponse,
    );
}
