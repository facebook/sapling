/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::clone::Clone;

use anyhow::anyhow;
use anyhow::Result;
use cloned::cloned;
use context::CoreContext;
use futures::stream;
use futures::stream::TryStreamExt;
use mononoke_types::BonsaiChangeset;
use mononoke_types::NonRootMPath;
use movers::Mover;
use slog::debug;

use crate::git_submodules::expand::SubmoduleExpansionData;
use crate::git_submodules::expand::SubmodulePath;
use crate::git_submodules::utils::get_x_repo_submodule_metadata_file_path;
use crate::types::Repo;

/// Validate that a given bonsai **from the large repo** keeps all submodule
/// expansions valid.
pub(crate) async fn validate_all_submodule_expansions<'a, R: Repo>(
    ctx: &'a CoreContext,
    sm_exp_data: SubmoduleExpansionData<'a, R>,
    // Bonsai from the large repo that should have all submodule expansions
    // validated
    bonsai: BonsaiChangeset,
    mover: Mover,
) -> Result<BonsaiChangeset> {
    // For every submodule dependency, get all changes in their directories.

    // Iterate over the submodule dependency paths.
    // Create a map grouping the file changes per submodule dependency.

    let bonsai: BonsaiChangeset = stream::iter(sm_exp_data.submodule_deps.iter().map(anyhow::Ok))
        .try_fold(bonsai, |bonsai, (submodule_path, _submodule_repo)| {
            cloned!(mover, sm_exp_data);

            // We only need to create a submodule metadata file for the expansion
            // of the submodules used directly by the small repo. i.e. we don't
            // need to create one for the recursive submodules, because changing
            // them means changing the submodule that contains it.
            //
            // So before calling the validation function for a submodule, let's
            // check if it's a recursive one by counting the number of submodule
            // paths in the submodule deps map that are prefix of this submodule.
            let is_recursive_submodule = sm_exp_data
                .submodule_deps
                .keys()
                .filter(|sm_path| sm_path.is_prefix_of(submodule_path))
                .count()
                > 1;

            async move {
                if is_recursive_submodule {
                    return Ok(bonsai);
                };
                validate_submodule_expansion(ctx, sm_exp_data, bonsai, submodule_path, mover).await
            }
        })
        .await?;

    Ok(bonsai)
}

/// Validate that a bonsai in the large repo is valid for a given submodule repo
/// repo.
/// Among other things, it will assert that
/// 1. If the submodule expansion is changed, the submodule metadata file (i.e.
/// pointer) is updated as well.
/// 2. The submoldule metadata file exists, contains a valid git commit hash
/// and that commit exists in the submodule repo.
/// 3. The working copy of the commit in the submodule repo is exactly the same
/// as its expansion in the large repo.
///
/// NOTE: this function will derive fsnodes for the provided bonsais, so it
/// requires access to the large repo's blobstore and that the parent commits
/// have fsnodes already derived.
async fn validate_submodule_expansion<'a, R: Repo>(
    ctx: &'a CoreContext,
    sm_exp_data: SubmoduleExpansionData<'a, R>,
    // Bonsai from the large repo
    bonsai: BonsaiChangeset,
    submodule_path: &'a NonRootMPath,
    mover: Mover,
) -> Result<BonsaiChangeset> {
    debug!(
        ctx.logger(),
        "Validating expansion of submodule {0} while syncing commit {1:?}",
        submodule_path,
        bonsai.get_changeset_id()
    );

    // STEP 1: Check if any changes were made to this submodule's expansion
    // or metadata file.
    //
    // The logic behind this is:
    // - If a submodule expansion is changed, the metadata file must be changed
    // as well, because 2 different working copies can't map to the same git
    // commit hash.
    // - However, if the submodule metadata file changes, the working copy does
    // **not necessarily need to change**. e.g. two commits can have the same
    // working copy, but different metadata, leading to different commit hashes.

    // Submodule path in the large repo, after calling the mover(e.g. to prepend
    // the small repo's path).
    let synced_submodule_path = mover(submodule_path)?.ok_or(anyhow!(
        "Mover failed to provide submodule path in the large repo"
    ))?;

    // TODO(gustavoavena): make this more efficient using `range`
    let submodule_expansion_changed = bonsai
        .file_changes()
        .any(|(p, _fc)| synced_submodule_path.is_prefix_of(p));

    // TODO(T179533620): confirm that the submodule expansion actually
    // exists in this path OR stop using submodule dependencies from all
    // commit sync config versions in history (T184633369)

    let synced_submodule_path = SubmodulePath(synced_submodule_path);

    let metadata_file_path = get_x_repo_submodule_metadata_file_path(
        &synced_submodule_path,
        sm_exp_data.x_repo_submodule_metadata_file_prefix,
    )?;
    let _synced_submodule_path = synced_submodule_path.0;

    let fc_map = bonsai.file_changes_map();
    let mb_metadata_file_fc = fc_map.get(&metadata_file_path);

    let _metadata_file_fc = match mb_metadata_file_fc {
        Some(fc) => fc,
        None => {
            // This means that the metadata file wasn't modified
            if submodule_expansion_changed {
                // Submodule expansion changed, but the metadata file wasn't updated
                return Err(anyhow!(
                    "Expansion of submodule {submodule_path} changed without updating its metadata file {metadata_file_path}"
                ));
            };

            // Metadata file didn't change but its submodule expansion also wasn't
            // changed.
            return Ok(bonsai);
        }
    };

    // ------------------------------------------------------------------------
    // STEP 2: Get the fsnode from the commit in the submodule repo, by reading
    // the the submodule metadata file.
    //
    // In the process, assert that:
    // 1. The file content blob exists in the large repo
    // 2. The file has a valid git commit hash
    // 3. This commit exists in the submodule repo.

    // TODO(T179533620): get the fsnode from the commit in submodule repo

    // ------------------------------------------------------------------------
    // STEP 3: Get the fsnode from the expansion of the submodule in the large
    // repo and compare it with the fsnode from the submodule commit.

    // Get the root fsnodes from the parent commits, so the one from this commit
    // can be derived.

    // TODO(T179533620): derive fsnode for the bonsai and compare it with
    // the fsnode from the submodule commit.

    Ok(bonsai)
}
