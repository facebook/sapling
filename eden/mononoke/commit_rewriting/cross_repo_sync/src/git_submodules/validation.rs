/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use futures::stream;
use futures::stream::TryStreamExt;
use mononoke_types::BonsaiChangeset;

use crate::git_submodules::expand::SubmoduleExpansionData;
use crate::types::Repo;

/// Validate that a given bonsai **from the large repo** keeps all submodule
/// expansions valid.
pub(crate) async fn validate_all_submodule_expansions<'a, R: Repo>(
    bonsai: BonsaiChangeset,
    sm_exp_data: SubmoduleExpansionData<'a, R>,
) -> Result<BonsaiChangeset> {
    // For every submodule dependency, get all changes in their directories.

    // Iterate over the submodule dependency paths.
    // Create a map grouping the file changes per submodule dependency.

    let bonsai: BonsaiChangeset = stream::iter(sm_exp_data.submodule_deps.iter().map(anyhow::Ok))
        .try_fold(
            bonsai,
            |bonsai, (_submodule_path, _submodule_repo)| async move {
                validate_submodule_expansion(bonsai).await
            },
        )
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
async fn validate_submodule_expansion(bonsai: BonsaiChangeset) -> Result<BonsaiChangeset> {
    // TODO(T179533620): validate that all changes are consistent with submodule
    // metadata file.
    Ok(bonsai)
}
