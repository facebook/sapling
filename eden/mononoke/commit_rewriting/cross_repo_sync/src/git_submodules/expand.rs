/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::clone::Clone;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;

use anyhow::anyhow;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use async_recursion::async_recursion;
use blobstore::Storable;
use cloned::cloned;
use commit_transformation::copy_file_contents;
use commit_transformation::rewrite_commit;
use commit_transformation::RewriteOpts;
use context::CoreContext;
use either::Either;
use either::Either::*;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use manifest::BonsaiDiffFileChange;
use maplit::hashmap;
use mononoke_types::BlobstoreValue;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::FileChange;
use mononoke_types::FileContents;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use mononoke_types::RepositoryId;
use mononoke_types::TrackedFileChange;
use movers::Mover;
use slog::debug;
use sorted_vector_map::SortedVectorMap;

use crate::commit_syncers_lib::mover_to_multi_mover;
use crate::git_submodules::utils::get_git_hash_from_submodule_file;
use crate::git_submodules::utils::get_submodule_file_content_id;
use crate::git_submodules::utils::get_submodule_repo;
use crate::git_submodules::utils::get_x_repo_submodule_metadata_file_path;
use crate::git_submodules::utils::is_path_git_submodule;
use crate::git_submodules::utils::list_all_paths;
use crate::git_submodules::utils::list_non_submodule_files_under;
use crate::git_submodules::utils::submodule_diff;
use crate::git_submodules::validation::validate_all_submodule_expansions;
use crate::types::Large;
use crate::types::Repo;

/// Wrapper to differentiate submodule paths from file changes paths at the
/// type level.
#[derive(Eq, Clone, Debug, PartialEq, Hash, PartialOrd, Ord)]
pub(crate) struct SubmodulePath(pub(crate) NonRootMPath);

impl std::fmt::Display for SubmodulePath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

// TODO(T174902563): support expansion of git submodules
/// Everything needed to expand submodule changes
#[derive(Clone)]
pub struct SubmoduleExpansionData<'a, R: Repo> {
    // Submodule dependencies of from the small repo, which have to be loaded
    // and available to (a) expand submodule file changes or (b) validate
    // that a bonsai in the large repo doesn't break the consistency of submodule
    // expansions.
    pub submodule_deps: &'a HashMap<NonRootMPath, R>,
    pub x_repo_submodule_metadata_file_prefix: &'a str,
    // TODO(T179530927): remove this once backsync is supported
    /// Used to ensure that trying to backsync from large to small repos that
    /// have submodule expansion enabled crashes while backsync is not supported.
    pub large_repo_id: Large<RepositoryId>,
}
pub async fn expand_and_validate_all_git_submodule_file_changes<'a, R: Repo>(
    ctx: &'a CoreContext,
    bonsai: BonsaiChangesetMut,
    small_repo: &'a R,
    sm_exp_data: SubmoduleExpansionData<'a, R>,
    mover: Mover,
    // Parameters needed to generate a bonsai for the large repo using `rewrite_commit`
    remapped_parents: &'a HashMap<ChangesetId, ChangesetId>,
    rewrite_opts: RewriteOpts,
) -> Result<BonsaiChangesetMut> {
    ensure!(
        small_repo.repo_identity().id() != *sm_exp_data.large_repo_id,
        "Can't sync changes from large to small repo if small repo has submodule expansion enabled"
    );

    let new_bonsai =
        expand_all_git_submodule_file_changes(ctx, bonsai, small_repo, sm_exp_data.clone())
            .await
            .context("Failed to expand submodule file changes from bonsai")?;

    let rewritten_bonsai = rewrite_commit(
        ctx,
        new_bonsai,
        remapped_parents,
        mover_to_multi_mover(mover.clone()),
        small_repo,
        None,
        rewrite_opts,
    )
    .await
    .context("Failed to create bonsai to be synced")?
    .ok_or(anyhow!("No bonsai to be synced was returned"))?;

    let rewritten_bonsai = rewritten_bonsai.freeze()?;

    // TODO(T179533620): validate that all changes are consistent with submodule
    // metadata file.
    let validated_bonsai =
        validate_all_submodule_expansions(ctx, sm_exp_data, rewritten_bonsai, mover).await?;

    Ok(validated_bonsai.into_mut())
}

/// Iterate over all file changes from the bonsai being synced and expand any
/// changes to git submodule files, generating the bonsai that will be synced
/// to the large repo.
async fn expand_all_git_submodule_file_changes<'a, R: Repo>(
    ctx: &'a CoreContext,
    cs: BonsaiChangesetMut,
    small_repo: &'a R,
    sm_exp_data: SubmoduleExpansionData<'a, R>,
) -> Result<BonsaiChangesetMut> {
    let fcs: SortedVectorMap<NonRootMPath, FileChange> = cs.file_changes;
    let parents = cs.parents.as_slice();

    let expanded_fcs: SortedVectorMap<NonRootMPath, FileChange> = stream::iter(fcs)
        .then(|(p, fc)| {
            cloned!(sm_exp_data);
            async move {
                match &fc {
                    FileChange::Change(tfc) => match &tfc.file_type() {
                        FileType::GitSubmodule => {
                            expand_git_submodule_file_change(
                                ctx,
                                small_repo,
                                sm_exp_data.clone(),
                                parents,
                                p,
                                tfc.content_id(),
                            )
                            .await
                        }
                        _ => {
                            if sm_exp_data.submodule_deps.contains_key(&p) {
                                // A normal file is replacing a submodule in the
                                // small repo, which means that the submodule
                                // expansion is being implicitly deleted in the
                                // large repo.
                                // If the expansion is deleted, we also need to
                                // delete the submodule metadata file.
                                let x_repo_sm_metadata_path =
                                    get_x_repo_submodule_metadata_file_path(
                                        &SubmodulePath(p.clone()),
                                        sm_exp_data.x_repo_submodule_metadata_file_prefix,
                                    )?;
                                return Ok(vec![
                                    (p, fc),
                                    // Explicit deletion for the submodule
                                    // metadata file
                                    (x_repo_sm_metadata_path, FileChange::Deletion),
                                ]);
                            };
                            Ok(vec![(p, fc)])
                        }
                    },
                    FileChange::Deletion => {
                        let paths_to_delete =
                            handle_submodule_deletion(ctx, small_repo, sm_exp_data, parents, p)
                                .await?;
                        Ok(paths_to_delete
                            .into_iter()
                            .map(|p| (p, FileChange::Deletion))
                            .collect())
                    }
                    _ => Ok(vec![(p, fc)]),
                }
            }
        })
        .try_collect::<Vec<_>>()
        .await?
        .into_iter()
        .flatten()
        .map(|(p, fc)| {
            // Make sure that no submodule file changes go through to the large repo
            match &fc {
                FileChange::Change(tfc) => {
                    ensure!(
                        tfc.file_type() != FileType::GitSubmodule,
                        "Submodule file type should not be present in the file changes"
                    );
                    Ok((p, fc))
                }
                _ => Ok((p, fc)),
            }
        })
        .collect::<Result<_>>()?;

    let new_cs = BonsaiChangesetMut {
        file_changes: expanded_fcs,
        ..cs
    };

    Ok(new_cs)
}

/// Expand a single file change from a git submodule.
///
/// In the source repo, the git submodule is a file containing the git hash of
/// the submodule's commit that the source repo depends on (let's call this
/// commit `X`).
///
/// In the target repo, the submodule path will be a directory containing the
/// contents of the submodule's working copy at commit `X`.
///
/// **EXAMPLE:** let's consider repos `source` and `A`, where `A` is a git submodule
/// dependency of `source` mirrored in Mononoke.
/// If A has commits X-Y-Z, a file change being expanded here would be, for example,
/// modifying the contents of the submodule file from `X` to `Z`.
///
/// This function will generate all the file changes to bring the working copy
/// of subdirectory `A` in `source` to the working copy of commit `Z` in `A`.
///
/// **IMPORTANT NOTES**
///
/// This function assumes that all the submodules (direct or recursive) are
/// mirrored in a Mononoke repo and these repos are loaded and available inside
/// `submodule_deps`. It will crash if that's not the case.
///
/// This depends on fsnodes from the commits in the source repo and the
/// submodule repos, so if they aren't already derived, they will be during the
/// expansion process.
///
/// All the file content blobs from all the submodule repos will be copied into
/// the source repo's blobstore, so that the commit rewrite crate can copy them
/// into the target repo.
async fn expand_git_submodule_file_change<'a, R: Repo>(
    ctx: &'a CoreContext,
    small_repo: &'a R,
    sm_exp_data: SubmoduleExpansionData<'a, R>,
    parents: &'a [ChangesetId],
    submodule_file_path: NonRootMPath,
    submodule_file_content_id: ContentId,
) -> Result<Vec<(NonRootMPath, FileChange)>> {
    let submodule_path = SubmodulePath(submodule_file_path);
    // Contains lists of file changes along
    // with the submodule these file changes are
    // from, so that the file content blobs are
    // copied from each submodule's blobstore into
    // the source repo's blobstore.
    let exp_results = expand_git_submodule(
        ctx,
        small_repo,
        parents,
        submodule_path.clone(),
        sm_exp_data.submodule_deps,
        submodule_file_content_id,
    )
    .await?;

    // Build the list of file changes to be returned as well as a map of content
    // ids that have to be copied from each submodule's blobstore.
    //
    // This is performed as a fold to dedup duplicate content ids in file
    // changes from different submodules, which can lead to errors when copying
    // the blobs to the source repo.
    let (copy_per_submodule, expanded_file_changes, _) = exp_results.into_iter().fold(
        (HashMap::new(), Vec::new(), HashSet::new()),
        move |(mut copy_per_submodule, mut acc_fcs, already_copied), (sm_path, fcs)| {
            let (submodule_ids, already_copied) = fcs.iter().fold(
                (HashSet::new(), already_copied),
                move |(mut submodule_ids, mut already_copied), (_, fc)| {
                    match fc {
                        FileChange::Change(tfc) => {
                            let cid = tfc.content_id();
                            if !already_copied.contains(&cid) {
                                // File wasn't set to be copied yet, so insert
                                // it into the set to copy from the current submodule
                                already_copied.insert(cid);
                                submodule_ids.insert(cid);
                            }
                        }
                        _ => (),
                    };
                    (submodule_ids, already_copied)
                },
            );

            // Insert the files that will be copied from this submodule's blobstore
            copy_per_submodule.insert(sm_path, submodule_ids);
            acc_fcs.extend(fcs);
            (copy_per_submodule, acc_fcs, already_copied)
        },
    );

    stream::iter(copy_per_submodule)
        .then(|(sm_path, content_ids_to_copy)| async move {
            let submodule_repo = get_submodule_repo(&sm_path, sm_exp_data.submodule_deps)?;

            // The commit rewrite crate copies the file content blobs from the
            // source repo to the target repo, so all the blobs from the submodule
            // repos need to be copied to the source repo.
            copy_file_contents(ctx, submodule_repo, small_repo, content_ids_to_copy, |_| {})
                .await
                .with_context(|| format!("Failed to copy file blobs from submodule {}", &sm_path.0))
        })
        .try_collect()
        .await?;

    // File changes generated for the expanded submodule and changes to its
    // x-repo submodule metadata file
    let all_file_changes = generate_additional_file_changes(
        ctx,
        small_repo,
        parents,
        submodule_path,
        submodule_file_content_id,
        sm_exp_data.x_repo_submodule_metadata_file_prefix,
        expanded_file_changes,
    )
    .await?;

    anyhow::Ok(all_file_changes)
}

#[async_recursion]
async fn expand_git_submodule<'a, R: Repo>(
    ctx: &'a CoreContext,
    small_repo: &'a R,
    // Parents from the **source repo commmit** being rewritten.
    // This is needed to get the hash of the previous commit of the submodule
    // being expanded.
    parents: &'a [ChangesetId],
    // Path of the submodule file in the source repo, which contains the encoded
    // git hash of the submodule's commit that the source repo depends on.
    submodule_path: SubmodulePath,
    // Map of submodule file paths to their corresponding Mononoke repo instances.
    submodule_deps: &'a HashMap<NonRootMPath, R>,
    // The
    submodule_file_content_id: ContentId,
    // Returns a map from submodule path to a list of file changes, so that
    // before the file changes are rewritten, the file content blobs are copied
    // from the appropriate submodule repo into the source repo's blobstore.
) -> Result<HashMap<SubmodulePath, Vec<(NonRootMPath, FileChange)>>> {
    debug!(ctx.logger(), "Expanding submodule {}", &submodule_path);

    let submodule_repo = get_submodule_repo(&submodule_path, submodule_deps)?;
    let git_submodule_sha1 = get_git_hash_from_submodule_file(
        ctx,
        small_repo,
        submodule_file_content_id,
        &submodule_path,
    )
    .await?;

    debug!(
        ctx.logger(),
        "submodule_path: {} | git_submodule_hash: {} | submodule_repo name: {}",
        &submodule_path,
        &git_submodule_sha1,
        &submodule_repo.repo_identity().name()
    );

    let sm_changeset_id = submodule_repo
        .bonsai_git_mapping()
        .get_bonsai_from_git_sha1(ctx, git_submodule_sha1)
        .await?
        .ok_or_else(|| {
            anyhow!(
                "Failed to get changeset id from git submodule commit hash {} in repo {}",
                &git_submodule_sha1,
                &submodule_repo.repo_identity().name()
            )
        })?;

    let sm_parents = get_previous_submodule_commits(
        ctx,
        parents,
        small_repo,
        submodule_path.clone(),
        submodule_repo,
    )
    .await?;

    // `sm_file_changes` are the changes in the submodule being processed
    // that should be expanded.
    //  `recursive_sm_file_changes` are the changes from all submodules that
    // the current submodule depends on.
    // The latter need to be stored separately because all the file content
    // blobs will need to be copied from the appropriate repository after
    // generating the file changes.
    let (sm_file_changes, recursive_sm_file_changes) =
        submodule_diff(ctx, submodule_repo, sm_changeset_id, sm_parents)
            .await?
            .map_ok(|diff| {
                cloned!(submodule_path);

                async move {
                    match diff {
                        BonsaiDiffFileChange::Changed(path, file_type, (content_id, size))
                        | BonsaiDiffFileChange::ChangedReusedId(
                            path,
                            file_type,
                            (content_id, size),
                        ) => {
                            if file_type != FileType::GitSubmodule {
                                // Non-submodule file changes just need to have the submodule
                                // path in the source repo pre-pended to their path.
                                let new_tfc =
                                    TrackedFileChange::new(content_id, file_type, size, None);
                                let path_in_sm = submodule_path.0.join(&path);

                                let fcs = vec![(path_in_sm, FileChange::Change(new_tfc))];
                                return Ok(Left(fcs));
                            }

                            let previous_submodule_commits = get_previous_submodule_commits(
                                ctx,
                                parents,
                                small_repo,
                                submodule_path.clone(),
                                submodule_repo,
                            )
                            .await?;

                            process_recursive_submodule_file_change(
                                ctx,
                                // Use the previous commits of the submodule as parents
                                // when expanding any recursive submodules.
                                previous_submodule_commits.as_slice(),
                                submodule_path,
                                submodule_repo,
                                submodule_deps,
                                SubmodulePath(path),
                                content_id,
                            )
                            .await
                        }
                        BonsaiDiffFileChange::Deleted(path) => {
                            let path_in_sm = submodule_path.0.join(&path);
                            let fcs = vec![(path_in_sm, FileChange::Deletion)];
                            Ok(Left(fcs))
                        }
                    }
                }
            })
            .map_err(|e| anyhow!("Failed to generate a BonsaiDiffFileChange: {e}"))
            .try_buffer_unordered(100)
            .try_collect::<Vec<_>>()
            .await?
            .into_iter()
            .fold((vec![], HashMap::new()), |(mut fcs, mut sm_fcs), res| {
                match res {
                    Left(res_fcs) => {
                        fcs.extend(res_fcs);
                    }
                    Right(sm_fcs_map) => {
                        sm_fcs_map.into_iter().for_each(|(path, res_fcs)| {
                            let fcs = sm_fcs.entry(path).or_insert(vec![]);
                            fcs.extend(res_fcs);
                        });
                    }
                }

                (fcs, sm_fcs)
            });

    let mut fc_map = hashmap![submodule_path => sm_file_changes];
    recursive_sm_file_changes
        .into_iter()
        .for_each(|(sm_path, sm_fcs)| {
            let fcs = fc_map.entry(sm_path).or_insert(vec![]);
            fcs.extend(sm_fcs);
        });
    Ok(fc_map)
}

/// Small helper function mostly to avoid confusion with nomenclature.
///
/// Consider the following repos `source -> A -> B`, where `source` is the repo
/// being exported, `A` is a direct submodule dependency and `B` is a submodule
/// inside repo `A`.
/// This will generate the file changes to expand `B` inside `A` and will
/// then modify them so they're placed inside the copy of `A` in `source`.
async fn process_recursive_submodule_file_change<'a, R: Repo>(
    ctx: &'a CoreContext,
    // Parents that should be used to generate the proper delta file changes.
    parents: &'a [ChangesetId],
    // Path of submodule `A` within repo `source`.
    submodule_path: SubmodulePath,
    submodule_repo: &'a R,
    submodule_deps: &'a HashMap<NonRootMPath, R>,
    // Path of submodule `B` within repo `A`.
    recursive_submodule_path: SubmodulePath,
    // Content id of the submodule `B` file inside repo `A`.
    // It contains the git hash of the `B` commit that `A` depends on.
    recursive_sm_file_content_id: ContentId,
) -> Result<
    Either<
        Vec<(NonRootMPath, FileChange)>,
        HashMap<SubmodulePath, Vec<(NonRootMPath, FileChange)>>,
    >,
> {
    // Create a new source_deps_map for the recursive call, removing the
    // submodule prefix from the keys that have it and ignoring the ones that
    // don't, because they're not relevant to the submodule being processed.
    // This prefix is added back to the results, before they're returned.
    let rec_small_repo_deps: HashMap<NonRootMPath, R> = submodule_deps
        .iter()
        .filter_map(|(p, repo)| {
            p.remove_prefix_component(&submodule_path.0)
                .map(|relative_p| (relative_p, repo.clone()))
        })
        .collect();

    let rec_sm_file_changes = expand_git_submodule(
        ctx,
        submodule_repo,
        parents,
        recursive_submodule_path,
        &rec_small_repo_deps,
        recursive_sm_file_content_id,
    )
    .await
    .with_context(|| {
        format!(
            "Failed to expand recursive submodule inside submodule {}",
            &submodule_repo.repo_identity().name()
        )
    })?;

    let sm_fcs_map = rec_sm_file_changes
        .into_iter()
        .map(|(rec_sm, rec_fcs)| {
            let mod_fcs = rec_fcs
                .into_iter()
                .map(|(p, fc)| (submodule_path.0.join(&p), fc))
                .collect::<Vec<_>>();
            // Add back the prefix of the root submodule to the recursive one
            (SubmodulePath(submodule_path.0.join(&rec_sm.0)), mod_fcs)
        })
        .collect::<HashMap<_, _>>();
    Ok(Right(sm_fcs_map))
}

/// Get the previous commit of the submodule that the source repo depended on.
/// These changesets will be used to generate the best delta.
///
/// **Example:**
/// The source repo depends on submodule A. A has commits X-Y-Z.
/// The source repo initially depended on X, but now we're syncing a commit that
/// updates the submodule straight to Z.
///
/// When generating the delta (i.e. file changes to the submodule directory),
/// we should generate the differences between commit X and Z, instead of
/// copying the entire working copy in commit Z (which is a lot of unnecessary work).
///
/// Using the example from above, the output of this funciton would be `[X]`.
async fn get_previous_submodule_commits<'a, R: Repo>(
    ctx: &'a CoreContext,
    // Parents of the changeset being synced from the source repo. We get the
    // contents of the submodule file in those revisions to get the previous
    // submodule commit the source repo dependend on.
    parents: &'a [ChangesetId],
    small_repo: &'a R,
    // Path of submodule `A` within repo `source`.
    submodule_path: SubmodulePath,
    // Submodule repo in Mononoke
    submodule_repo: &'a R,
) -> Result<Vec<ChangesetId>> {
    let parents_vec = parents
        .iter()
        .map(|cs_id| anyhow::Ok(*cs_id))
        .collect::<Vec<_>>();

    // Get the changeset ids of the previous revision of the submodule that the
    // source repo depended on, if the submodule is being updated. If the
    // submodule is being added, this set will be empty.
    let sm_parents: Vec<ChangesetId> = stream::iter(parents_vec)
        .try_filter_map(|cs_id| {
            cloned!(ctx, submodule_path);

            async move {
                // Check the submodule path on that revision
                match get_submodule_file_content_id(&ctx, small_repo, cs_id, &submodule_path.0).await? {
                    // If it's a submodule file, the submodule is being updated
                    Some(submodule_file_content_id) => {
                        // File is a submodule, so get the git hash that it stored
                        // which represents the pointer to that submodule.
                        let git_sha1 = get_git_hash_from_submodule_file(
                            &ctx,
                            small_repo,
                            submodule_file_content_id,
                            &submodule_path,
                        )
                        .await?;

                        // From the git hash, get the bonsai changeset it in the
                        // submodule Mononoke repo.
                        let sm_parent_cs_id = submodule_repo
                            .bonsai_git_mapping()
                            .get_bonsai_from_git_sha1(&ctx, git_sha1)
                            .await?
                            .ok_or_else(|| {
                                anyhow!(
                                    "Failed to get changeset id from git submodule parent commit hash {} in repo {}",
                                    &git_sha1,
                                    &submodule_repo.repo_identity().name()
                                )
                            })?;
                        Ok(Some(sm_parent_cs_id))
                    }
                    // If it doesn't exist, or is a directory, skip it because
                    // it's not a revision that can be used as a parent to
                    // generate delta for the submodule expansion.  If it is a
                    // file of type other than GitSubmodule, it means that a
                    // submodule is being added in the place of a regular
                    // file, so this revision didn't have a dependency on the
                    // submodule, and can also be skipped.
                    None => Ok(None),
                }
            // Get content id of the file
        }})
        .try_collect::<Vec<_>>()
        .await?;
    Ok(sm_parents)
}

/// If a submodule is being deleted from the source repo, we should delete its
/// entire expanded copy in the large repo.
async fn handle_submodule_deletion<'a, R: Repo>(
    ctx: &'a CoreContext,
    small_repo: &'a R,
    sm_exp_data: SubmoduleExpansionData<'a, R>,
    parents: &'a [ChangesetId],
    submodule_file_path: NonRootMPath,
) -> Result<Vec<NonRootMPath>> {
    // If the path is in the submodule_deps keys, it's almost
    // certainly a submodule being deleted.
    if sm_exp_data
        .submodule_deps
        .contains_key(&submodule_file_path)
    {
        // However, to be certain, let's verify that this file
        // was indeed of type `GitSubmodule` by getting the checking
        // the FileType of the submodule file path in each of the parents.
        let is_git_submodule_file = stream::iter(parents)
            .map(|cs_id| is_path_git_submodule(ctx, small_repo, *cs_id, &submodule_file_path))
            .buffered(10)
            .boxed()
            .try_collect::<Vec<_>>()
            .await?
            .into_iter()
            .any(|is_git_submodule| is_git_submodule);

        // Not a submodule file, just a file at the same path
        // where a git submodule used to be, so just delete the file normally.
        if !is_git_submodule_file {
            return Ok(vec![submodule_file_path]);
        }

        // This is a submodule file, so delete its entire expanded directory.
        return delete_submodule_expansion(
            ctx,
            small_repo,
            sm_exp_data,
            parents,
            submodule_file_path,
        )
        .await;
    };

    Ok(vec![submodule_file_path])
}

/// After confirming that the path being deleted is indeed a submodule file,
/// generate the deletion for its entire expanded directory.
async fn delete_submodule_expansion<'a, R: Repo>(
    ctx: &'a CoreContext,
    small_repo: &'a R,
    sm_exp_data: SubmoduleExpansionData<'a, R>,
    parents: &'a [ChangesetId],
    submodule_file_path: NonRootMPath,
) -> Result<Vec<NonRootMPath>> {
    let submodule_path = SubmodulePath(submodule_file_path.clone());
    let submodule_repo = get_submodule_repo(&submodule_path, sm_exp_data.submodule_deps)?;

    // Gets the submodule revision that the source repo is currently pointing to.
    let sm_parents = get_previous_submodule_commits(
        ctx,
        parents,
        small_repo,
        submodule_path.clone(),
        submodule_repo,
    )
    .await?;

    // Get the entire working copy of the submodule in those revisions, so we
    // can generate the proper paths to be deleted.
    let submodule_leaves = stream::iter(sm_parents)
        .map(|cs_id| list_all_paths(ctx, submodule_repo, cs_id))
        .buffered(10)
        .try_flatten_unordered(None)
        .try_collect::<Vec<_>>()
        .await?;

    // Make sure we delete the x-repo submodule metadata file as well
    let paths_to_delete: Vec<_> = {
        let mut paths_to_delete: Vec<_> = submodule_leaves
            .into_iter()
            .map(|path| submodule_file_path.join(&path))
            .collect();
        let x_repo_sm_metadata_path = get_x_repo_submodule_metadata_file_path(
            &submodule_path,
            sm_exp_data.x_repo_submodule_metadata_file_prefix,
        )?;
        paths_to_delete.push(x_repo_sm_metadata_path);
        paths_to_delete
    };

    Ok(paths_to_delete)
}

/**
 After getting the file changes from the submodule repo, generate any additional
 file changes needed to bring the bonsai into a healthy/consistent state.
 - Submodule metadata file, which stores the pointer to the submodule revision
 being expanded and is used to validate consistency between the revision and
 its expansion.
 - Deletions of files/directories that are being replaced by the creation of the
 submodule expansion.
*/
async fn generate_additional_file_changes<'a, R: Repo>(
    ctx: &'a CoreContext,
    small_repo: &'a R,
    parents: &'a [ChangesetId],
    submodule_path: SubmodulePath,
    submodule_file_content_id: ContentId,
    x_repo_submodule_metadata_file_prefix: &'a str,
    expanded_file_changes: Vec<(NonRootMPath, FileChange)>,
) -> Result<Vec<(NonRootMPath, FileChange)>> {
    // Step 1: Generate the submodule metadata file change

    // After expanding the submodule, we also need to generate the x-repo
    // submodule metadata file, to keep track of the git hash that this expansion
    // corresponds to.
    let x_repo_sm_metadata_path = get_x_repo_submodule_metadata_file_path(
        &submodule_path,
        x_repo_submodule_metadata_file_prefix,
    )?;

    let git_submodule_sha1 = get_git_hash_from_submodule_file(
        ctx,
        small_repo,
        submodule_file_content_id,
        &submodule_path,
    )
    .await?;
    let metadata_file_content = FileContents::new_bytes(git_submodule_sha1.to_string());
    let metadata_file_size = metadata_file_content.size();
    let metadata_file_content_id = metadata_file_content
        .into_blob()
        .store(ctx, small_repo.repo_blobstore())
        .await?;

    // The metadata file will have the same content as the submodule file
    // change in the source repo, but it will be a regular file, because in
    // the large repo we can never have file changes of type `GitSubmodule`.
    let x_repo_sm_metadata_fc = FileChange::tracked(
        metadata_file_content_id,
        FileType::Regular,
        metadata_file_size,
        None,
    );
    let mut all_changes = vec![(x_repo_sm_metadata_path, x_repo_sm_metadata_fc)];

    // Step 2: Generate the deletions of files/directories that are being
    // replaced by the creation of the submodule expansion.

    // Get the non submodule files underneath the submodule path in the parent commits
    let non_submodule_paths: Vec<NonRootMPath> = stream::iter(parents)
        .then(|cs_id| {
            // If there is an entry for a **non-GitSubmodule file type**, it
            // means that in the large repo we're replacing a regular file or
            // regular directory with the submodule expansion, so we need to
            // generate deletions for it.
            // The paths to be deleted, will be all the leaves under the
            // submodule path.
            list_non_submodule_files_under(ctx, small_repo, *cs_id, submodule_path.clone())
        })
        .boxed()
        .try_flatten()
        .try_collect::<Vec<_>>()
        .await?;

    // NOTE: Deletions must be added first, because the expanded changes take
    // precedence over the deletions. i.e. if we generate a Deletion and a Change
    // for the same path, it means that the submodule expansion is replacing
    // a regular directory with a file in the same path.
    // We don't want this file actually deleted.

    all_changes.extend(
        non_submodule_paths
            .into_iter()
            .map(|path| (path, FileChange::Deletion)),
    );

    all_changes.extend(expanded_file_changes);

    Ok(all_changes)
}
