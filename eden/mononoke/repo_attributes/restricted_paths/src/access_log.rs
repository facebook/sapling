/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// Log access to restricted paths

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use context::CoreContext;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use mononoke_types::NonRootMPath;
use mononoke_types::RepositoryId;
use permission_checker::AclProvider;
use permission_checker::MononokeIdentity;
use permission_checker::PermissionCheckerBuilder;
use scuba_ext::MononokeScubaSampleBuilder;

use crate::manifest_id_store::ManifestId;
use crate::manifest_id_store::ManifestType;

pub const ACCESS_LOG_SCUBA_TABLE: &str = "mononoke_restricted_paths_access_test";

pub(crate) enum RestrictedPathAccessData {
    /// When the tree is accessed by manifest id
    Manifest(ManifestId, ManifestType),
    /// When the tree is accessed by path
    FullPath { full_path: NonRootMPath },
}

/// Check if the caller has access to paths protected by the given ACLs.
pub async fn has_access_to_acl(
    ctx: &CoreContext,
    acl_provider: &Arc<dyn AclProvider>,
    acls: &[&MononokeIdentity],
) -> Result<bool> {
    if acls.is_empty() {
        return Ok(true);
    }

    let permission_checker = stream::iter(acls.iter().cloned())
        .map(anyhow::Ok)
        .try_fold(PermissionCheckerBuilder::new(), async |builder, acl| {
            Ok(builder.allow(
                acl_provider
                    .repo_region_acl(acl.id_data())
                    .await
                    .with_context(|| {
                        format!("Failed to create PermissionChecker for {}", acl.id_data())
                    })?,
            ))
        })
        .await
        .context("creating PermissionCheckerBuilder")?
        .build();

    Ok(permission_checker
        .check_set(ctx.metadata().identities(), &["read"])
        .await)
}

pub(crate) async fn log_access_to_restricted_path(
    ctx: &CoreContext,
    repo_id: RepositoryId,
    restricted_paths: Vec<NonRootMPath>,
    acls: Vec<&MononokeIdentity>,
    access_data: RestrictedPathAccessData,
    acl_provider: Arc<dyn AclProvider>,
    scuba: MononokeScubaSampleBuilder,
) -> Result<bool> {
    // TODO(T239041722): store permission checkers in RestrictedPaths to improve
    // performance if needed.
    let has_authorization = has_access_to_acl(ctx, &acl_provider, &acls).await?;

    log_access_to_scuba(
        ctx,
        repo_id,
        restricted_paths,
        access_data,
        has_authorization,
        acls,
        scuba,
    )?;

    Ok(has_authorization)
}

fn log_access_to_scuba(
    ctx: &CoreContext,
    repo_id: RepositoryId,
    restricted_paths: Vec<NonRootMPath>,
    access_data: RestrictedPathAccessData,
    has_authorization: bool,
    acls: Vec<&MononokeIdentity>,
    mut scuba: MononokeScubaSampleBuilder,
) -> Result<()> {
    scuba.add_metadata(ctx.metadata());

    scuba.add_common_server_data();

    // We want to log all samples
    scuba.unsampled();

    scuba.add("repo_id", repo_id.id());
    scuba.add(
        "restricted_paths",
        restricted_paths
            .into_iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>(),
    );

    scuba.add("has_authorization", has_authorization);
    scuba.add(
        "acls",
        acls.into_iter()
            .map(|acl| acl.to_string())
            .collect::<Vec<_>>(),
    );

    // Log access data based on the type
    match access_data {
        RestrictedPathAccessData::Manifest(manifest_id, manifest_type) => {
            scuba.add("manifest_id", manifest_id.to_string());
            scuba.add("manifest_type", manifest_type.to_string());
        }
        RestrictedPathAccessData::FullPath { full_path, .. } => {
            scuba.add("full_path", full_path.to_string());
        }
    }

    scuba.log();

    Ok(())
}
