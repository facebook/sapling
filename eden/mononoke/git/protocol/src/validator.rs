/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use context::CoreContext;
use rustc_hash::FxHashSet;

use crate::Repo;
use crate::mapping::git_shas_to_bonsais;
use crate::types::ShallowInfoRequest;

/// Validates that a shallow fetch without deepen arguments is not trying to
/// indirectly unshallow the repository. This is detected by comparing
/// `ancestors_difference(WANTS, SHALLOW)` against `ancestors_difference(WANTS, [])`.
/// If they differ, it means some ancestors of WANTS are also ancestors of SHALLOW,
/// i.e., they are below the shallow boundary and fetching them would unshallow the repo.
///
/// Prerequisites (checked by the caller):
/// 1. The client has shallow commits (non-empty shallow list)
/// 2. The client is not using any deepen argument (variant is None)
///
/// The fetch would fail at runtime with a broken commit graph,
/// so we proactively return an error asking the user to use --unshallow.
pub async fn validate_shallow_fetch_without_deepen(
    ctx: &CoreContext,
    repo: &impl Repo,
    request: &ShallowInfoRequest,
) -> Result<()> {
    let want_bonsais = git_shas_to_bonsais(ctx, repo, request.heads.iter())
        .await
        .context("Failed to convert WANT commits to bonsais")?;
    let shallow_bonsais = git_shas_to_bonsais(ctx, repo, request.shallow.iter())
        .await
        .context("Failed to convert SHALLOW commits to bonsais")?;

    // Compute ancestors_difference(WANTS, SHALLOW) and ancestors_difference(WANTS, []).
    // If they differ, some ancestors of WANTS are also ancestors of SHALLOW,
    // meaning the fetch would cross the shallow boundary.
    let ancestors_diff_shallow = repo
        .commit_graph()
        .ancestors_difference(
            ctx,
            want_bonsais.bonsais.clone(),
            shallow_bonsais.bonsais.clone(),
        )
        .await
        .context("Failed to compute ancestors difference with SHALLOW")?;

    let all_want_ancestors = repo
        .commit_graph()
        .ancestors_difference(ctx, want_bonsais.bonsais.clone(), vec![])
        .await
        .context("Failed to compute all ancestors of WANTS")?;

    let ancestors_diff_shallow_set: FxHashSet<_> = ancestors_diff_shallow.into_iter().collect();
    let all_want_ancestors_set: FxHashSet<_> = all_want_ancestors.into_iter().collect();

    if ancestors_diff_shallow_set != all_want_ancestors_set {
        anyhow::bail!(
            "You are indirectly trying to unshallow the repo without using unshallow \
            or deepen argument. This can lead to broken repo state and hence is not \
            supported. Please fetch with --unshallow argument instead."
        );
    }

    Ok(())
}
