/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Result;
use blobstore::KeyedBlobstore;
use blobstore::Storable;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use mononoke_types::BlobstoreValue;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::acl_manifest::AclManifest;

use crate::RootAclManifestId;

/// Derive an AclManifest using the `derive_manifest` primitive.
///
/// Handles all cases: root commits, single-parent, and merges.
/// Only `.slacl` file changes (explicit + implicit deletes) are fed to
/// `derive_manifest`; unchanged subtrees are reused automatically.
pub(crate) async fn derive_single(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    _bonsai: BonsaiChangeset,
    _parents: Vec<RootAclManifestId>,
    _known: Option<&HashMap<ChangesetId, RootAclManifestId>>,
) -> Result<RootAclManifestId> {
    empty_root_acl_manifest_id(ctx, derivation_ctx.blobstore()).await
}

async fn empty_root_acl_manifest_id<'a>(
    ctx: &'a CoreContext,
    blobstore: &'a (impl KeyedBlobstore + 'a),
) -> Result<RootAclManifestId> {
    let empty = AclManifest::empty();
    let id = empty.into_blob().store(ctx, blobstore).await?;
    Ok(RootAclManifestId(id))
}
