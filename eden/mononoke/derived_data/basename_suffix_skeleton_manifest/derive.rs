/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use mononoke_types::BonsaiChangeset;

use crate::mapping::RootBasenameSuffixSkeletonManifest;

pub(crate) async fn derive_single(
    _ctx: &CoreContext,
    _derivation_ctx: &DerivationContext,
    _bonsai: BonsaiChangeset,
    _parents: Vec<RootBasenameSuffixSkeletonManifest>,
) -> Result<RootBasenameSuffixSkeletonManifest> {
    unimplemented!()
}
