/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use serde_derive::{Deserialize, Serialize};

use types::HgId;

use crate::wire::{ToApi, ToWire, WireHgId, WireToApiConversionError};

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct WirePullFastForwardRequest {
    #[serde(rename = "1")]
    pub old_master: WireHgId,
    #[serde(rename = "2")]
    pub new_master: WireHgId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PullFastForwardRequest {
    pub old_master: HgId,
    pub new_master: HgId,
}

impl ToWire for PullFastForwardRequest {
    type Wire = WirePullFastForwardRequest;

    fn to_wire(self) -> Self::Wire {
        WirePullFastForwardRequest {
            old_master: self.old_master.to_wire(),
            new_master: self.new_master.to_wire(),
        }
    }
}

impl ToApi for WirePullFastForwardRequest {
    type Api = PullFastForwardRequest;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(PullFastForwardRequest {
            old_master: self.old_master.to_api()?,
            new_master: self.new_master.to_api()?,
        })
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WirePullFastForwardRequest {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        PullFastForwardRequest::arbitrary(g).to_wire()
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for PullFastForwardRequest {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        PullFastForwardRequest {
            old_master: HgId::arbitrary(g),
            new_master: HgId::arbitrary(g),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::wire::tests::auto_wire_tests;

    auto_wire_tests!(WirePullFastForwardRequest);
}
