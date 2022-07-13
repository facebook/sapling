/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use metaconfig_types::Identity;
use repos::RawAllowlistIdentity;

use crate::convert::Convert;
use crate::errors::ConfigurationError;

impl Convert for RawAllowlistIdentity {
    type Output = Identity;

    fn convert(self) -> Result<Self::Output> {
        if self.identity_type.is_empty() || self.identity_data.is_empty() {
            return Err(ConfigurationError::InvalidFileStructure(
                "identity type and data must be specified".into(),
            )
            .into());
        }
        Ok(Identity {
            id_type: self.identity_type,
            id_data: self.identity_data,
        })
    }
}
