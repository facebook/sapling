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
use mononoke_types::MPathHash;
use mononoke_types::ThriftConvert;
use serde::Deserialize;
use serde::Serialize;

/// Logical, path-based identity of a pipeline stage, used by the derivation
/// methods. The `Finalize` variant is reserved for a future finalize step and
/// is never constructed yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StageId {
    Manifest(MPath),
    Finalize,
}

/// Identity, hash-based key of a pipeline stage, used as the derivation-queue
/// key. Hashing the path is one-way, so there is deliberately no
/// `StageKey -> StageId` conversion; the path is recovered from the payload.
/// The `Finalize` variant is reserved for a future finalize step and is never
/// constructed yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StageKey {
    Manifest(MPathHash),
    Finalize,
}

impl StageId {
    pub fn to_key(&self) -> StageKey {
        match self {
            StageId::Manifest(path) => StageKey::Manifest(path.get_path_hash()),
            StageId::Finalize => StageKey::Finalize,
        }
    }
}

/// Self-describing stage payload embedded in a queued dag item. The `Finalize`
/// variant is reserved for a future finalize step and is never constructed yet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DerivationStagePayload {
    Manifest(ManifestStagePayload),
    Finalize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestStagePayload {
    pub path: MPath,
    pub deps: Vec<MPathElement>,
}

impl DerivationStagePayload {
    /// The absolute path prefix this stage covers, or `None` for the finalize
    /// stage which is not path-scoped.
    pub fn path(&self) -> Option<&MPath> {
        match self {
            DerivationStagePayload::Manifest(p) => Some(&p.path),
            DerivationStagePayload::Finalize => None,
        }
    }

    pub fn to_thrift(&self) -> derivation_queue_thrift::DerivationStagePayload {
        match self {
            DerivationStagePayload::Manifest(payload) => {
                derivation_queue_thrift::DerivationStagePayload::manifest(payload.to_thrift())
            }
            DerivationStagePayload::Finalize => {
                derivation_queue_thrift::DerivationStagePayload::finalize(
                    derivation_queue_thrift::FinalizeStagePayload {},
                )
            }
        }
    }

    pub fn from_thrift(payload: derivation_queue_thrift::DerivationStagePayload) -> Result<Self> {
        match payload {
            derivation_queue_thrift::DerivationStagePayload::manifest(payload) => Ok(
                DerivationStagePayload::Manifest(ManifestStagePayload::from_thrift(payload)?),
            ),
            derivation_queue_thrift::DerivationStagePayload::finalize(_) => {
                Ok(DerivationStagePayload::Finalize)
            }
            derivation_queue_thrift::DerivationStagePayload::UnknownField(x) => {
                Err(anyhow!("Unknown DerivationStagePayload variant: {x}"))
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
