/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use anyhow::Result;
use metaconfig_types::AclRegion;
use metaconfig_types::AclRegionConfig;
use metaconfig_types::AclRegionRule;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use repos::RawAclRegion;
use repos::RawAclRegionConfig;
use repos::RawAclRegionRule;

use crate::convert::Convert;

impl Convert for RawAclRegion {
    type Output = AclRegion;

    fn convert(self) -> Result<Self::Output> {
        Ok(AclRegion {
            roots: self
                .roots
                .into_iter()
                .map(|s| ChangesetId::from_str(&s))
                .collect::<Result<Vec<_>>>()?,
            heads: self
                .heads
                .into_iter()
                .map(|s| ChangesetId::from_str(&s))
                .collect::<Result<Vec<_>>>()?,
            path_prefixes: self
                .path_prefixes
                .into_iter()
                .map(|b| MPath::new_opt(&*b))
                .collect::<Result<Vec<_>>>()?,
        })
    }
}

impl Convert for RawAclRegionRule {
    type Output = AclRegionRule;

    fn convert(self) -> Result<Self::Output> {
        Ok(AclRegionRule {
            name: self.name,
            regions: self.regions.convert()?,
            hipster_acl: self.hipster_acl,
        })
    }
}

impl Convert for RawAclRegionConfig {
    type Output = AclRegionConfig;

    fn convert(self) -> Result<Self::Output> {
        Ok(AclRegionConfig {
            allow_rules: self.allow_rules.convert()?,
        })
    }
}
