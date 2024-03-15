/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;

use anyhow::anyhow;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_recursion::async_recursion;
use blobstore::Storable;
use cloned::cloned;
use commit_transformation::copy_file_contents;
use context::CoreContext;
use either::Either;
use either::Either::*;
use fsnodes::RootFsnodeId;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use git_types::ObjectKind;
use manifest::bonsai_diff;
use manifest::BonsaiDiffFileChange;
use manifest::Entry::*;
use manifest::ManifestOps;
use maplit::hashmap;
use mononoke_types::hash::GitSha1;
use mononoke_types::hash::RichGitSha1;
use mononoke_types::BlobstoreValue;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::FileChange;
use mononoke_types::FileContents;
use mononoke_types::FileType;
use mononoke_types::FsnodeId;
use mononoke_types::NonRootMPath;
use mononoke_types::TrackedFileChange;
use slog::debug;
use sorted_vector_map::SortedVectorMap;

use crate::commit_syncers_lib::Repo;

/// Wrapper to differentiate submodule paths from file changes paths at the
/// type level.
#[derive(Eq, Clone, Debug, PartialEq, Hash, PartialOrd, Ord)]
struct SubmodulePath(pub(crate) NonRootMPath);

impl std::fmt::Display for SubmodulePath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Iterate over all file changes from the bonsai being synced and expand any
/// changes to git submodule files, generating the bonsai that will be synced
/// to the large repo.
pub async fn expand_all_git_submodule_file_changes<'a, R: Repo>(
    ctx: &'a CoreContext,
    cs: BonsaiChangesetMut,
    source_repo: &'a R,
    source_repo_deps: &'a HashMap<NonRootMPath, R>,
    x_repo_submodule_metadata_file_prefix: String,
) -> Result<BonsaiChangesetMut> {
    let fcs: SortedVectorMap<NonRootMPath, FileChange> = cs.file_changes;
    let parents = cs.parents.as_slice();
    let x_repo_submodule_metadata_prefix_str = x_repo_submodule_metadata_file_prefix.as_str();

    let expanded_fcs: SortedVectorMap<NonRootMPath, FileChange> = stream::iter(fcs)
        .then(|(p, fc)| async move {
            match &fc {
                FileChange::Change(tfc) => match &tfc.file_type() {
                    FileType::GitSubmodule => {
                        expand_git_submodule_file_change(
                            ctx,
                            source_repo,
                            source_repo_deps,
                            parents,
                            p,
                            tfc.content_id(),
                            x_repo_submodule_metadata_prefix_str,
                        )
                        .await
                    }
                    _ => Ok(vec![(p, fc)]),
                },
                FileChange::Deletion => {
                    let paths_to_delete = handle_submodule_deletion(
                        ctx,
                        source_repo,
                        source_repo_deps,
                        parents,
                        p,
                        x_repo_submodule_metadata_prefix_str,
                    )
                    .await?;
                    Ok(paths_to_delete
                        .into_iter()
                        .map(|p| (p, FileChange::Deletion))
                        .collect())
                }
                _ => Ok(vec![(p, fc)]),
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
/// `source_repo_deps`. It will crash if that's not the case.
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
    source_repo: &'a R,
    source_repo_deps: &'a HashMap<NonRootMPath, R>,
    parents: &'a [ChangesetId],
    submodule_file_path: NonRootMPath,
    submodule_file_content_id: ContentId,
    x_repo_submodule_metadata_file_prefix: &str,
) -> Result<Vec<(NonRootMPath, FileChange)>> {
    let submodule_path = SubmodulePath(submodule_file_path);
    // Contains lists of file changes along
    // with the submodule these file changes are
    // from, so that the file content blobs are
    // copied from each submodule's blobstore into
    // the source repo's blobstore.
    let exp_results = expand_git_submodule(
        ctx,
        source_repo,
        parents,
        submodule_path.clone(),
        source_repo_deps,
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
            let submodule_repo = get_submodule_repo(&sm_path, source_repo_deps)?;

            // The commit rewrite crate copies the file content blobs from the
            // source repo to the target repo, so all the blobs from the submodule
            // repos need to be copied to the source repo.
            copy_file_contents(
                ctx,
                submodule_repo,
                source_repo,
                content_ids_to_copy,
                |_| {},
            )
            .await
            .with_context(|| format!("Failed to copy file blobs from submodule {}", &sm_path.0))
        })
        .try_collect()
        .await?;

    // After expanding the submodule, we also need to generate the x-repo
    // submodule metadata file, to keep track of the git hash that this expansion
    // corresponds to.
    let x_repo_sm_metadata_path = get_x_repo_submodule_metadata_file_path(
        &submodule_path,
        x_repo_submodule_metadata_file_prefix,
    )?;

    // File changes generated for the expanded submodule and changes to its
    // x-repo submodule metadata file
    let all_file_changes = {
        let mut all_changes = expanded_file_changes;
        let git_submodule_sha1 = get_git_hash_from_submodule_file(
            ctx,
            source_repo,
            submodule_file_content_id,
            &submodule_path,
        )
        .await?;
        let metadata_file_content = FileContents::new_bytes(git_submodule_sha1.to_string());
        let metadata_file_size = metadata_file_content.size();
        let metadata_file_content_id = metadata_file_content
            .into_blob()
            .store(ctx, source_repo.repo_blobstore())
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
        all_changes.push((x_repo_sm_metadata_path, x_repo_sm_metadata_fc));
        all_changes
    };

    anyhow::Ok(all_file_changes)
}

#[async_recursion]
async fn expand_git_submodule<'a, R: Repo>(
    ctx: &'a CoreContext,
    source_repo: &'a R,
    // Parents from the **source repo commmit** being rewritten.
    // This is needed to get the hash of the previous commit of the submodule
    // being expanded.
    parents: &'a [ChangesetId],
    // Path of the submodule file in the source repo, which contains the encoded
    // git hash of the submodule's commit that the source repo depends on.
    submodule_path: SubmodulePath,
    // Map of submodule file paths to their corresponding Mononoke repo instances.
    source_repo_deps: &'a HashMap<NonRootMPath, R>,
    // The
    submodule_file_content_id: ContentId,
    // Returns a map from submodule path to a list of file changes, so that
    // before the file changes are rewritten, the file content blobs are copied
    // from the appropriate submodule repo into the source repo's blobstore.
) -> Result<HashMap<SubmodulePath, Vec<(NonRootMPath, FileChange)>>> {
    debug!(ctx.logger(), "Expanding submodule {}", &submodule_path);

    let submodule_repo = get_submodule_repo(&submodule_path, source_repo_deps)?;
    let git_submodule_sha1 = get_git_hash_from_submodule_file(
        ctx,
        source_repo,
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

    let sm_manifest_id = id_to_fsnode_manifest_id(ctx, submodule_repo, sm_changeset_id)
        .await
        .context(format!(
            "Failed to get fsnode id from changeset id {}",
            &sm_changeset_id
        ))?;

    let sm_parents = get_previous_submodule_commits(
        ctx,
        parents,
        source_repo,
        submodule_path.clone(),
        submodule_repo,
    )
    .await?;

    let sm_parent_manifest_ids = stream::iter(sm_parents)
        .then(|cs_id| async move {
            id_to_fsnode_manifest_id(ctx, submodule_repo, cs_id)
                .await
                .with_context(|| {
                    format!(
                        "Failed to get parent's fsnode id from its changeset id: {}",
                        &cs_id
                    )
                })
        })
        .try_collect::<HashSet<_>>()
        .await?;

    // `sm_file_changes` are the changes in the submodule being processed
    // that should be expanded.
    //  `recursive_sm_file_changes` are the changes from all submodules that
    // the current submodule depends on.
    // The latter need to be stored separately because all the file content
    // blobs will need to be copied from the appropriate repository after
    // generating the file changes.
    let (sm_file_changes, recursive_sm_file_changes) = bonsai_diff(
        ctx.clone(),
        submodule_repo.repo_blobstore_arc().clone(),
        sm_manifest_id,
        sm_parent_manifest_ids,
    )
    .map_ok(|diff| {
        cloned!(submodule_path);

        async move {
            match diff {
                BonsaiDiffFileChange::Changed(path, file_type, (content_id, size))
                | BonsaiDiffFileChange::ChangedReusedId(path, file_type, (content_id, size)) => {
                    if file_type != FileType::GitSubmodule {
                        // Non-submodule file changes just need to have the submodule
                        // path in the source repo pre-pended to their path.
                        let new_tfc = TrackedFileChange::new(content_id, file_type, size, None);
                        let path_in_sm = submodule_path.0.join(&path);

                        let fcs = vec![(path_in_sm, FileChange::Change(new_tfc))];
                        return Ok(Left(fcs));
                    }

                    let previous_submodule_commits = get_previous_submodule_commits(
                        ctx,
                        parents,
                        source_repo,
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
                        source_repo_deps,
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
    source_repo_deps: &'a HashMap<NonRootMPath, R>,
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
    let rec_source_repo_deps: HashMap<NonRootMPath, R> = source_repo_deps
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
        &rec_source_repo_deps,
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
    source_repo: &'a R,
    // Path of submodule `A` within repo `source`.
    submodule_path: SubmodulePath,
    // Submodule repo in Mononoke
    submodule_repo: &'a R,
) -> Result<Vec<ChangesetId>> {
    let source_repo_blobstore = source_repo.repo_blobstore_arc().clone();

    let parents_vec = parents
        .iter()
        .map(|cs_id| anyhow::Ok(*cs_id))
        .collect::<Vec<_>>();

    // Get the changeset ids of the previous revision of the submodule that the
    // source repo depended on, if the submodule is being updated. If the
    // submodule is being added, this set will be empty.
    let sm_parents: Vec<ChangesetId> = stream::iter(parents_vec)
        .try_filter_map(|cs_id| {
            cloned!(ctx, submodule_path, source_repo_blobstore);

            async move {
                let fsnode_id = id_to_fsnode_manifest_id(&ctx, source_repo, cs_id).await?;
                let entry = fsnode_id
                    .find_entry(ctx.clone(), source_repo_blobstore, submodule_path.0.clone().into())
                    .await?;
                match entry {
                    Some(Leaf(fsnode_file)) => {
                        let git_sha1 = get_git_hash_from_submodule_file(
                            &ctx,
                            source_repo,
                            *fsnode_file.content_id(),
                            &submodule_path,
                        )
                        .await?;

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
                    Some(Tree(_)) | None => Ok(None),
                }
            // Get content id of the file
        }})
        .try_collect::<Vec<_>>()
        .await?;
    Ok(sm_parents)
}

/// Get the git hash from a submodule file, which represents the commit from the
/// given submodule that the source repo depends on at that revision.
async fn get_git_hash_from_submodule_file<'a, R: Repo>(
    ctx: &'a CoreContext,
    repo: &'a R,
    submodule_file_content_id: ContentId,
    submodule_path: &'a SubmodulePath,
) -> Result<GitSha1> {
    let blobstore = repo.repo_blobstore_arc();

    let bytes = filestore::fetch_concat_exact(&blobstore, ctx, submodule_file_content_id, 20)
        .await
        .with_context(|| {
            format!(
                "Failed to fetch content of submodule {} file containing the submodule's git commit hash",
                &submodule_path
            )
        })?;

    let git_submodule_hash = RichGitSha1::from_bytes(&bytes, ObjectKind::Commit.as_str(), 0)?;
    let git_submodule_sha1 = git_submodule_hash.sha1();

    anyhow::Ok(git_submodule_sha1)
}

fn get_submodule_repo<'a, 'b, R: Repo>(
    sm_path: &'a SubmodulePath,
    source_repo_deps: &'b HashMap<NonRootMPath, R>,
) -> Result<&'b R> {
    source_repo_deps
        .get(&sm_path.0)
        .ok_or_else(|| anyhow!("Mononoke repo from submodule {} not available", sm_path.0))
}

async fn id_to_fsnode_manifest_id(
    ctx: &CoreContext,
    repo: &impl Repo,
    bcs_id: ChangesetId,
) -> Result<FsnodeId, Error> {
    let repo_derived_data = repo.repo_derived_data();

    let root_fsnode_id = repo_derived_data
        .derive::<RootFsnodeId>(ctx, bcs_id)
        .await?;

    Ok(root_fsnode_id.into_fsnode_id())
}

/// If a submodule is being deleted from the source repo, we should delete its
/// entire expanded copy in the large repo.
async fn handle_submodule_deletion<'a, R: Repo>(
    ctx: &'a CoreContext,
    source_repo: &'a R,
    source_repo_deps: &'a HashMap<NonRootMPath, R>,
    parents: &'a [ChangesetId],
    submodule_file_path: NonRootMPath,
    x_repo_submodule_metadata_file_prefix: &str,
) -> Result<Vec<NonRootMPath>> {
    // If the path is in the source_repo_deps keys, it's almost
    // certainly a submodule being deleted.
    if source_repo_deps.contains_key(&submodule_file_path) {
        // However, to be certain, let's verify that this file
        // was indeed of type `GitSubmodule` by getting the parent fsnodes
        // and checking the FileType of the submodule file path.
        let parent_fsnode_ids: Vec<_> = stream::iter(parents)
            .then(|cs_id| id_to_fsnode_manifest_id(ctx, source_repo, *cs_id))
            .try_collect()
            .await?;

        // Checks if in any of the parents that path corresponds
        // to a file of type `GitSubmodule`.
        let is_git_submodule_file = stream::iter(parent_fsnode_ids)
            .then(|fsnode_id| {
                cloned!(ctx, submodule_file_path);
                let source_repo_blobstore = source_repo.repo_blobstore_arc();

                async move {
                    let entry = fsnode_id
                        .find_entry(ctx, source_repo_blobstore, submodule_file_path.into())
                        .await?;
                    match entry {
                        Some(manifest::Entry::Leaf(fsnode_file)) => {
                            Ok(*fsnode_file.file_type() == FileType::GitSubmodule)
                        }
                        _ => anyhow::Ok(false),
                    }
                }
            })
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
            source_repo,
            source_repo_deps,
            parents,
            submodule_file_path,
            x_repo_submodule_metadata_file_prefix,
        )
        .await;
    };

    Ok(vec![submodule_file_path])
}

/// After confirming that the path being deleted is indeed a submodule file,
/// generate the deletion for its entire expanded directory.
async fn delete_submodule_expansion<'a, R: Repo>(
    ctx: &'a CoreContext,
    source_repo: &'a R,
    source_repo_deps: &'a HashMap<NonRootMPath, R>,
    parents: &'a [ChangesetId],
    submodule_file_path: NonRootMPath,
    x_repo_submodule_metadata_file_prefix: &str,
) -> Result<Vec<NonRootMPath>> {
    let submodule_path = SubmodulePath(submodule_file_path.clone());
    let submodule_repo = get_submodule_repo(&submodule_path, source_repo_deps)?;

    // Gets the submodule revision that the source repo is currently pointing to.
    let sm_parents = get_previous_submodule_commits(
        ctx,
        parents,
        source_repo,
        submodule_path.clone(),
        submodule_repo,
    )
    .await?;

    let parent_fsnode_ids: Vec<_> = stream::iter(sm_parents)
        .then(|cs_id| id_to_fsnode_manifest_id(ctx, submodule_repo, cs_id))
        .try_collect()
        .await?;

    let submodule_blobstore = submodule_repo.repo_blobstore_arc().clone();

    // Get the entire working copy of the submodule in those revisions, so we
    // can generate the proper paths to be deleted.
    let submodule_leaves = stream::iter(parent_fsnode_ids)
        .map(|fsnode_id| fsnode_id.list_leaf_entries(ctx.clone(), submodule_blobstore.clone()))
        .flatten_unordered(None)
        .try_collect::<Vec<_>>()
        .await?;

    // Make sure we delete the x-repo submodule metadata file as well
    let paths_to_delete: Vec<_> = {
        let mut paths_to_delete: Vec<_> = submodule_leaves
            .into_iter()
            .map(|(path, _)| submodule_file_path.join(&path))
            .collect();
        let x_repo_sm_metadata_path = get_x_repo_submodule_metadata_file_path(
            &submodule_path,
            x_repo_submodule_metadata_file_prefix,
        )?;
        paths_to_delete.push(x_repo_sm_metadata_path);
        paths_to_delete
    };

    Ok(paths_to_delete)
}

/// Builds the full path of the x-repo submodule metadata file for a given
/// submodule.
fn get_x_repo_submodule_metadata_file_path(
    submodule_file_path: &SubmodulePath,
    // Prefix used to generate the metadata file basename. Obtained from
    // the small repo sync config.
    x_repo_submodule_metadata_file_prefix: &str,
) -> Result<NonRootMPath> {
    let (mb_sm_parent_dir, sm_basename) = submodule_file_path.0.split_dirname();

    let x_repo_sm_metadata_file: NonRootMPath = NonRootMPath::new(
        format!(".{x_repo_submodule_metadata_file_prefix}-{sm_basename}")
            .to_string()
            .into_bytes(),
    )?;

    let x_repo_sm_metadata_path = match mb_sm_parent_dir {
        Some(sm_parent_dir) => sm_parent_dir.join(&x_repo_sm_metadata_file),
        None => x_repo_sm_metadata_file,
    };
    Ok(x_repo_sm_metadata_path)
}
