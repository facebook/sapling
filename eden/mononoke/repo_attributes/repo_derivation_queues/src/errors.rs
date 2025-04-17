/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(fbcode_build)]
use delos_zk_common::ZelosExceptionType;
use derived_data_manager::DerivationError;
use derived_data_manager::SharedDerivationError;
use mononoke_types::RepositoryId;
use thiserror::Error;

use crate::DagItemId;
use crate::DerivationDagItem;

#[derive(Error, Debug)]
pub enum InternalError {
    #[error("Got item for non-existing repo: {0}")]
    RepoNotFound(RepositoryId),
    #[error("Provided config_name: {0} was not found in the available configs")]
    DerivationConfigNotFound(String),
    #[error("Derivation requested for unknown data type: {0}")]
    UnknownDerivedDataType(String),
    #[error("Item with this root_cs_id and derived_data_type is already in queue {0:#?}")]
    ItemExists(Box<DerivationDagItem>),
    #[error("Item with not derived and not present dependencies in Derivation DAG {0:#?}")]
    MissingDependencies(Box<DerivationDagItem>),
    #[error("While querying Derivation DAG item was deleted: {0}")]
    ItemDeleted(String),
    #[error("Attepmt to create Derivation Item with dependency on itself {0:#?}")]
    CircularDependency(DagItemId),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl From<DerivationError> for InternalError {
    fn from(e: DerivationError) -> Self {
        InternalError::Other(e.into())
    }
}

impl From<SharedDerivationError> for InternalError {
    fn from(e: SharedDerivationError) -> Self {
        InternalError::Other(e.into())
    }
}

#[cfg(fbcode_build)]
impl From<zeus_client::ZeusError> for InternalError {
    fn from(e: zeus_client::ZeusError) -> Self {
        match e {
            zeus_client::ZeusError::RuntimeError {
                message: msg,
                exception_type: ZelosExceptionType::ZNONODE,
            } => InternalError::ItemDeleted(msg),
            _ => InternalError::Other(e.into()),
        }
    }
}
