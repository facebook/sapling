/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use serde_derive::{Deserialize, Serialize};

use dag_types::Location;
use types::HgId;

use crate::commit::{
    BonsaiChangesetContent, BonsaiExtra, BonsaiFileChange, CommitHashLookupRequest,
    CommitHashLookupResponse, CommitHashToLocationRequestBatch, CommitHashToLocationResponse,
    CommitLocationToHashRequest, CommitLocationToHashRequestBatch, CommitLocationToHashResponse,
    EphemeralPrepareRequest, EphemeralPrepareResponse, Extra, HgChangesetContent,
    HgMutationEntryContent, UploadBonsaiChangeset, UploadBonsaiChangesetsRequest,
    UploadHgChangeset, UploadHgChangesetsRequest,
};
use crate::wire::{
    is_default, ToApi, ToWire, WireFileType, WireHgId, WireParents, WireRepoPathBuf, WireResult,
    WireToApiConversionError, WireUploadToken,
};

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireCommitLocation {
    #[serde(rename = "1")]
    pub descendant: WireHgId,
    #[serde(rename = "2")]
    pub distance: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireCommitLocationToHashRequest {
    #[serde(rename = "1")]
    pub location: WireCommitLocation,
    #[serde(rename = "2")]
    pub count: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireCommitLocationToHashResponse {
    #[serde(rename = "1")]
    pub location: WireCommitLocation,
    #[serde(rename = "2")]
    pub count: u64,
    #[serde(rename = "3")]
    pub hgids: Vec<WireHgId>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireCommitLocationToHashRequestBatch {
    #[serde(rename = "1")]
    pub requests: Vec<WireCommitLocationToHashRequest>,
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
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Location::arbitrary(g).to_wire()
    }
}

impl ToWire for CommitLocationToHashRequest {
    type Wire = WireCommitLocationToHashRequest;

    fn to_wire(self) -> Self::Wire {
        Self::Wire {
            location: self.location.to_wire(),
            count: self.count,
        }
    }
}

impl ToApi for WireCommitLocationToHashRequest {
    type Api = CommitLocationToHashRequest;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        let api = Self::Api {
            location: self.location.to_api()?,
            count: self.count,
        };
        Ok(api)
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireCommitLocationToHashRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        CommitLocationToHashRequest::arbitrary(g).to_wire()
    }
}

impl ToWire for CommitLocationToHashResponse {
    type Wire = WireCommitLocationToHashResponse;

    fn to_wire(self) -> Self::Wire {
        Self::Wire {
            location: self.location.to_wire(),
            count: self.count,
            hgids: self.hgids.to_wire(),
        }
    }
}

impl ToApi for WireCommitLocationToHashResponse {
    type Api = CommitLocationToHashResponse;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        let api = Self::Api {
            location: self.location.to_api()?,
            count: self.count,
            hgids: self.hgids.to_api()?,
        };
        Ok(api)
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireCommitLocationToHashResponse {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        CommitLocationToHashResponse::arbitrary(g).to_wire()
    }
}

impl ToWire for CommitLocationToHashRequestBatch {
    type Wire = WireCommitLocationToHashRequestBatch;

    fn to_wire(self) -> Self::Wire {
        Self::Wire {
            requests: self.requests.to_wire(),
        }
    }
}

impl ToApi for WireCommitLocationToHashRequestBatch {
    type Api = CommitLocationToHashRequestBatch;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        let api = Self::Api {
            requests: self.requests.to_api()?,
        };
        Ok(api)
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireCommitLocationToHashRequestBatch {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        CommitLocationToHashRequestBatch::arbitrary(g).to_wire()
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
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
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
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
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
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
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
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        CommitHashLookupResponse::arbitrary(g).to_wire()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireExtra {
    #[serde(rename = "1")]
    pub key: Vec<u8>,

    #[serde(rename = "2")]
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireHgChangesetContent {
    #[serde(rename = "1")]
    pub parents: WireParents,

    #[serde(rename = "2")]
    pub manifestid: WireHgId,

    #[serde(rename = "3")]
    pub user: Vec<u8>,

    #[serde(rename = "4")]
    pub time: i64,

    #[serde(rename = "5")]
    pub tz: i32,

    #[serde(rename = "6")]
    pub extras: Vec<WireExtra>,

    #[serde(rename = "7")]
    pub files: Vec<WireRepoPathBuf>,

    #[serde(rename = "8")]
    pub message: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireUploadHgChangeset {
    #[serde(rename = "1")]
    pub node_id: WireHgId,

    #[serde(rename = "2")]
    pub changeset_content: WireHgChangesetContent,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireHgMutationEntryContent {
    #[serde(rename = "1")]
    pub successor: WireHgId,

    #[serde(rename = "2")]
    pub predecessors: Vec<WireHgId>,

    #[serde(rename = "3")]
    pub split: Vec<WireHgId>,

    #[serde(rename = "4")]
    pub op: String,

    #[serde(rename = "5")]
    pub user: Vec<u8>,

    #[serde(rename = "6")]
    pub time: i64,

    #[serde(rename = "7")]
    pub tz: i32,

    #[serde(rename = "8")]
    pub extras: Vec<WireExtra>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireUploadHgChangesetsRequest {
    /// list of changesets to upload, changesets must be sorted topologically (use dag.sort)
    #[serde(rename = "1")]
    pub changesets: Vec<WireUploadHgChangeset>,

    /// list of mutation entries for the uploading changesets
    #[serde(rename = "2")]
    pub mutations: Vec<WireHgMutationEntryContent>,
}

impl ToWire for Extra {
    type Wire = WireExtra;

    fn to_wire(self) -> Self::Wire {
        WireExtra {
            key: self.key.to_wire(),
            value: self.value.to_wire(),
        }
    }
}

impl ToApi for WireExtra {
    type Api = Extra;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(Extra {
            key: self.key.to_api()?,
            value: self.value.to_api()?,
        })
    }
}

impl ToWire for HgChangesetContent {
    type Wire = WireHgChangesetContent;

    fn to_wire(self) -> Self::Wire {
        WireHgChangesetContent {
            parents: self.parents.to_wire(),
            manifestid: self.manifestid.to_wire(),
            user: self.user.to_wire(),
            time: self.time,
            tz: self.tz,
            extras: self.extras.to_wire(),
            files: self.files.to_wire(),
            message: self.message.to_wire(),
        }
    }
}

impl ToApi for WireHgChangesetContent {
    type Api = HgChangesetContent;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(HgChangesetContent {
            parents: self.parents.to_api()?,
            manifestid: self.manifestid.to_api()?,
            user: self.user.to_api()?,
            time: self.time,
            tz: self.tz,
            extras: self.extras.to_api()?,
            files: self.files.to_api()?,
            message: self.message.to_api()?,
        })
    }
}

impl ToWire for HgMutationEntryContent {
    type Wire = WireHgMutationEntryContent;

    fn to_wire(self) -> Self::Wire {
        WireHgMutationEntryContent {
            successor: self.successor.to_wire(),
            predecessors: self.predecessors.to_wire(),
            split: self.split.to_wire(),
            op: self.op,
            user: self.user.to_wire(),
            time: self.time,
            tz: self.tz,
            extras: self.extras.to_wire(),
        }
    }
}

impl ToApi for WireHgMutationEntryContent {
    type Api = HgMutationEntryContent;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(HgMutationEntryContent {
            successor: self.successor.to_api()?,
            predecessors: self.predecessors.to_api()?,
            split: self.split.to_api()?,
            op: self.op,
            user: self.user.to_api()?,
            time: self.time,
            tz: self.tz,
            extras: self.extras.to_api()?,
        })
    }
}

impl ToWire for UploadHgChangeset {
    type Wire = WireUploadHgChangeset;

    fn to_wire(self) -> Self::Wire {
        WireUploadHgChangeset {
            node_id: self.node_id.to_wire(),
            changeset_content: self.changeset_content.to_wire(),
        }
    }
}

impl ToApi for WireUploadHgChangeset {
    type Api = UploadHgChangeset;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(UploadHgChangeset {
            node_id: self.node_id.to_api()?,
            changeset_content: self.changeset_content.to_api()?,
        })
    }
}

impl ToWire for UploadHgChangesetsRequest {
    type Wire = WireUploadHgChangesetsRequest;

    fn to_wire(self) -> Self::Wire {
        WireUploadHgChangesetsRequest {
            changesets: self.changesets.to_wire(),
            mutations: self.mutations.to_wire(),
        }
    }
}

impl ToApi for WireUploadHgChangesetsRequest {
    type Api = UploadHgChangesetsRequest;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(UploadHgChangesetsRequest {
            changesets: self.changesets.to_api()?,
            mutations: self.mutations.to_api()?,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireBonsaiExtra {
    #[serde(rename = "1")]
    pub key: String,

    #[serde(rename = "2")]
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireBonsaiFileChange {
    #[serde(rename = "1")]
    pub file_type: WireFileType,

    #[serde(rename = "2")]
    pub upload_token: WireUploadToken,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
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
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireUploadBonsaiChangeset {
    #[serde(rename = "1")]
    pub hg_changeset_id: WireHgId,

    #[serde(rename = "2")]
    pub changeset_content: WireBonsaiChangesetContent,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireUploadBonsaiChangesetsRequest {
    /// list of changesets to upload, changesets must be sorted topologically (use dag.sort)
    #[serde(rename = "1")]
    pub changesets: Vec<WireUploadBonsaiChangeset>,

    /// list of mutation entries for the uploading changesets
    #[serde(rename = "2")]
    pub mutations: Vec<WireHgMutationEntryContent>,
}

impl ToWire for BonsaiExtra {
    type Wire = WireBonsaiExtra;

    fn to_wire(self) -> Self::Wire {
        WireBonsaiExtra {
            key: self.key,
            value: self.value,
        }
    }
}

impl ToWire for BonsaiFileChange {
    type Wire = WireBonsaiFileChange;
    fn to_wire(self) -> Self::Wire {
        WireBonsaiFileChange {
            file_type: self.file_type.to_wire(),
            upload_token: self.upload_token.to_wire(),
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
        }
    }
}

impl ToWire for UploadBonsaiChangeset {
    type Wire = WireUploadBonsaiChangeset;

    fn to_wire(self) -> Self::Wire {
        WireUploadBonsaiChangeset {
            hg_changeset_id: self.hg_changeset_id.to_wire(),
            changeset_content: self.changeset_content.to_wire(),
        }
    }
}

impl ToApi for WireBonsaiExtra {
    type Api = BonsaiExtra;
    type Error = std::convert::Infallible;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(BonsaiExtra {
            key: self.key,
            value: self.value,
        })
    }
}

impl ToApi for WireBonsaiFileChange {
    type Api = BonsaiFileChange;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(BonsaiFileChange {
            file_type: self.file_type.to_api()?,
            upload_token: self.upload_token.to_api()?,
        })
    }
}

impl ToApi for WireUploadBonsaiChangeset {
    type Api = UploadBonsaiChangeset;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(UploadBonsaiChangeset {
            hg_changeset_id: self.hg_changeset_id.to_api()?,
            changeset_content: self.changeset_content.to_api()?,
        })
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
        })
    }
}

impl ToWire for UploadBonsaiChangesetsRequest {
    type Wire = WireUploadBonsaiChangesetsRequest;

    fn to_wire(self) -> Self::Wire {
        WireUploadBonsaiChangesetsRequest {
            changesets: self.changesets.to_wire(),
            mutations: self.mutations.to_wire(),
        }
    }
}

impl ToApi for WireUploadBonsaiChangesetsRequest {
    type Api = UploadBonsaiChangesetsRequest;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(UploadBonsaiChangesetsRequest {
            changesets: self.changesets.to_api()?,
            mutations: self.mutations.to_api()?,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireEphemeralPrepareRequest {}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WireEphemeralPrepareResponse {
    #[serde(rename = "1")]
    pub bubble_id: Option<std::num::NonZeroU64>,
}

impl ToWire for EphemeralPrepareRequest {
    type Wire = WireEphemeralPrepareRequest;

    fn to_wire(self) -> Self::Wire {
        Self::Wire {}
    }
}

impl ToApi for WireEphemeralPrepareRequest {
    type Api = EphemeralPrepareRequest;
    type Error = std::convert::Infallible;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(Self::Api {})
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireEphemeralPrepareRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        EphemeralPrepareRequest::arbitrary(g).to_wire()
    }
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
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        EphemeralPrepareResponse::arbitrary(g).to_wire()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::wire::tests::{check_serialize_roundtrip, check_wire_roundtrip};

    use quickcheck::quickcheck;

    quickcheck! {
        fn test_roundtrip_serialize_location(v: WireCommitLocation) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_roundtrip_wire_location(v: Location<HgId>) -> bool {
            check_wire_roundtrip(v)
        }

        fn test_roundtrip_serialize_location_to_hash_request(
            v: WireCommitLocationToHashRequest
        ) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_roundtrip_wire_location_to_hash_request(
            v: CommitLocationToHashRequest
        ) -> bool {
            check_wire_roundtrip(v)
        }

        fn test_roundtrip_serialize_location_to_hash_response(
            v: WireCommitLocationToHashResponse
        ) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_roundtrip_wire_location_to_hash_response(
            v: CommitLocationToHashResponse
        ) -> bool {
            check_wire_roundtrip(v)
        }

        fn test_roundtrip_serialize_location_to_hash_request_batch(
            v: WireCommitLocationToHashRequestBatch
        ) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_roundtrip_wire_location_to_hash_request_batch(
            v: CommitLocationToHashRequestBatch
        ) -> bool {
            check_wire_roundtrip(v)
        }

        fn test_roundtrip_serialize_hash_to_location_request_batch(
            v: WireCommitHashToLocationRequestBatch
        ) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_roundtrip_hash_to_location_request_batch(
            v: CommitHashToLocationRequestBatch
        ) -> bool {
            check_wire_roundtrip(v)
        }

        fn test_roundtrip_serialize_hash_to_location_response(
            v: WireCommitHashToLocationResponse
        ) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_roundtrip_wire_hash_to_location_response(
            v: CommitHashToLocationResponse
        ) -> bool {
            check_wire_roundtrip(v)
        }

        fn test_roundtrip_serialize_hash_lookup_request(
            v: WireCommitHashLookupRequest
        ) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_roundtrip_wire_hash_lookup_request(
            v: CommitHashLookupRequest
        ) -> bool {
            check_wire_roundtrip(v)
        }

        fn test_roundtrip_serialize_hash_lookup_response(
            v: WireCommitHashLookupResponse
        ) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_roundtrip_wire_hash_lookup_response(
            v: CommitHashLookupResponse
        ) -> bool {
            check_wire_roundtrip(v)
        }

        fn test_roundtrip_ephemeral_prepare_req(
            v: WireEphemeralPrepareRequest
        ) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_roundtrip_wire_ephemeral_prepare_req(
            v: EphemeralPrepareRequest
        ) -> bool {
            check_wire_roundtrip(v)
        }

        fn test_roundtrip_ephemeral_prepare_res(
            v: WireEphemeralPrepareResponse
        ) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_roundtrip_wire_ephemeral_prepare_res(
            v: EphemeralPrepareResponse
        ) -> bool {
            check_wire_roundtrip(v)
        }
    }
}
