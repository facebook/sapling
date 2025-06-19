/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use context::CoreContext;
use futures_stats::TimedFutureExt;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use movers::Movers;
use reporting::set_scuba_logger_fields;
use scuba_ext::FutureStatsScubaExt;

use crate::git_submodules::compact::compact_all_submodule_expansion_file_changes;
use crate::git_submodules::expand::SubmoduleExpansionData;
use crate::git_submodules::expand::expand_all_git_submodule_file_changes;
use crate::git_submodules::validation::ValidSubmoduleExpansionBonsai;
use crate::rewrite_commit_with_file_changes_filter;
use crate::types::CommitRewriteResult;
use crate::types::Repo;
use crate::types::RewriteOpts;

// TODO(T182311609): rename this to rewrite_commit
/// Sync a commit to/from a small repo with submodule expansion enabled.
pub async fn sync_commit_with_submodule_expansion<'a, R: Repo>(
    ctx: &'a CoreContext,
    bonsai: BonsaiChangesetMut,
    source_repo: &'a R,
    sm_exp_data: SubmoduleExpansionData<'a, R>,
    movers: Movers,
    // Parameters needed to generate a bonsai for the large repo using `rewrite_commit`
    remapped_parents: &'a HashMap<ChangesetId, ChangesetId>,
    rewrite_opts: RewriteOpts,
) -> Result<CommitRewriteResult> {
    let is_forward_sync =
        source_repo.repo_identity().id() != sm_exp_data.large_repo.repo_identity().id();

    if !is_forward_sync {
        let ctx = &set_scuba_logger_fields(
            ctx,
            [
                (
                    "source_repo",
                    sm_exp_data.large_repo.repo_identity().id().id(),
                ),
                ("target_repo", sm_exp_data.small_repo_id.id()),
            ],
        );

        // If any submodule expansion is being modified, run validation to make
        // sure the expansion remains valid and its metadata file was updated.
        // Then remove the expansion changes and generate a file change of type
        // GitSubmodule that can be backsynced to the small repo.
        let compacted_sm_bonsai = compact_all_submodule_expansion_file_changes(
            ctx,
            bonsai,
            sm_exp_data,
            source_repo,
            // Since this is backsyncing, forward sync mover is the reverse
            // mover
            movers.reverse_mover.clone(),
        )
        .await?;

        let mb_rewritten = compacted_sm_bonsai
            .rewrite_to_small_repo(
                ctx,
                remapped_parents,
                movers.mover.clone(),
                source_repo,
                rewrite_opts,
            )
            .await?;

        return Ok(CommitRewriteResult::new(mb_rewritten, HashMap::new()));
    };

    let ctx = &set_scuba_logger_fields(
        ctx,
        [
            ("source_repo", sm_exp_data.small_repo_id.id()),
            (
                "target_repo",
                sm_exp_data.large_repo.repo_identity().id().id(),
            ),
        ],
    );

    let (new_bonsai, submodule_expansion_content_ids) =
        expand_all_git_submodule_file_changes(ctx, bonsai, source_repo, sm_exp_data.clone())
            .timed()
            .await
            .log_future_stats(
                ctx.scuba().clone(),
                "Expanding all git submodule file changes",
                None,
            )
            .context("Failed to expand submodule file changes from bonsai")?;

    let mb_rewritten_bonsai = rewrite_commit_with_file_changes_filter(
        ctx,
        new_bonsai,
        remapped_parents,
        Arc::new(movers.mover.clone()),
        source_repo,
        None,
        rewrite_opts,
        vec![], // File change filters
    )
    .timed()
    .await
    .log_future_stats(ctx.scuba().clone(), "Rewriting commit", None)
    .context("Failed to rewrite commit")?;

    match mb_rewritten_bonsai {
        Some(rewritten_bonsai) => {
            let rewritten_bonsai = rewritten_bonsai.freeze()?;

            let validated_bonsai =
                ValidSubmoduleExpansionBonsai::validate_all_submodule_expansions(
                    ctx,
                    sm_exp_data,
                    rewritten_bonsai,
                    movers.mover,
                )
                .timed()
                .await
                .log_future_stats(
                    ctx.scuba().clone(),
                    "Validating all submodule expansions",
                    None,
                )
                // TODO(gustavoavena): print some identifier of changeset that failed
                .context("Validation of submodule expansion failed")?;

            let rewritten = Some(validated_bonsai.into_inner().into_mut());

            Ok(CommitRewriteResult::new(
                rewritten,
                submodule_expansion_content_ids,
            ))
        }
        None => Ok(CommitRewriteResult::new(None, HashMap::new())),
    }
}
