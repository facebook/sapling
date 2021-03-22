/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Repo derived data
//!
//! Stores configuration and state for data derivation.

use std::sync::Arc;

use cacheblob::LeaseOps;
use metaconfig_types::DerivedDataConfig;

/// Repository derived data management.
#[facet::facet]
pub struct RepoDerivedData {
    /// Configuration for derived data.
    config: DerivedDataConfig,

    /// Derived data lease, to prevent thundering herds of data derivation.
    lease: Arc<dyn LeaseOps>,
}

impl RepoDerivedData {
    /// Construct a new RepoDerivedData.
    pub fn new(config: DerivedDataConfig, lease: Arc<dyn LeaseOps>) -> RepoDerivedData {
        RepoDerivedData { config, lease }
    }

    /// Current derived data configuration for this repo.
    pub fn config(&self) -> &DerivedDataConfig {
        &self.config
    }

    /// Derived data lease for this repo.
    pub fn lease(&self) -> &Arc<dyn LeaseOps> {
        &self.lease
    }
}
