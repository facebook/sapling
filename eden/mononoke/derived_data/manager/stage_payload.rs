/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Self-describing stage payload embedded in queued dag items so workers
//! never have to consult the live `DerivationPipelineConfig`.

use anyhow::Result;
use anyhow::anyhow;
use mononoke_types::MPath;
use mononoke_types::MPathElement;
use mononoke_types::ThriftConvert;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DerivationStagePayload {
    Manifest(ManifestStagePayload),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestStagePayload {
    pub path: MPath,
    pub deps: Vec<MPathElement>,
}

impl DerivationStagePayload {
    pub fn path(&self) -> &MPath {
        match self {
            DerivationStagePayload::Manifest(p) => &p.path,
        }
    }

    pub fn to_thrift(&self) -> derivation_queue_thrift::DerivationStagePayload {
        match self {
            DerivationStagePayload::Manifest(payload) => {
                derivation_queue_thrift::DerivationStagePayload::manifest(payload.to_thrift())
            }
        }
    }

    pub fn from_thrift(payload: derivation_queue_thrift::DerivationStagePayload) -> Result<Self> {
        match payload {
            derivation_queue_thrift::DerivationStagePayload::manifest(payload) => Ok(
                DerivationStagePayload::Manifest(ManifestStagePayload::from_thrift(payload)?),
            ),
            derivation_queue_thrift::DerivationStagePayload::UnknownField(x) => {
                Err(anyhow!("Unknown DerivationStagePayload variant: {}", x))
            }
        }
    }
}

impl ManifestStagePayload {
    pub fn to_thrift(&self) -> derivation_queue_thrift::ManifestStagePayload {
        derivation_queue_thrift::ManifestStagePayload {
            path: self.path.clone().into_thrift(),
            deps: self.deps.iter().map(|e| e.clone().into_thrift()).collect(),
        }
    }

    pub fn from_thrift(payload: derivation_queue_thrift::ManifestStagePayload) -> Result<Self> {
        Ok(Self {
            path: MPath::from_thrift(payload.path)?,
            deps: payload
                .deps
                .into_iter()
                .map(MPathElement::from_thrift)
                .collect::<Result<Vec<_>>>()?,
        })
    }
}
