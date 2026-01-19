/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::clone::Clone;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use async_recursion::async_recursion;
use blobstore::Loadable;
use borrowed::borrowed;
use cloned::cloned;
use context::CoreContext;
use derived_data::macro_export::BonsaiDerivable;
use either::Either;
use fsnodes::RootFsnodeId;
use futures::StreamExt;
use futures::stream;
use futures::stream::TryStreamExt;
use futures_stats::TimedFutureExt;
use itertools::Itertools;
use manifest::Entry;
use manifest::ManifestOps;
use mononoke_types::BonsaiChangeset;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::FsnodeId;
use mononoke_types::MPathElement;
use mononoke_types::NonRootMPath;
use mononoke_types::fsnode::Fsnode;
use mononoke_types::fsnode::FsnodeDirectory;
use mononoke_types::fsnode::FsnodeEntry;
use mononoke_types::fsnode::FsnodeFile;
use movers::Mover;
use reporting::log_error;
use scuba_ext::FutureStatsScubaExt;
use tracing::trace;

use crate::git_submodules::expand::SubmoduleExpansionData;
use crate::git_submodules::utils::build_recursive_submodule_deps;
use crate::git_submodules::utils::content_id_of_file_with_type;
use crate::git_submodules::utils::get_git_hash_from_submodule_file;
use crate::git_submodules::utils::get_x_repo_submodule_metadata_file_path;
use crate::git_submodules::utils::git_hash_from_submodule_metadata_file;
use crate::git_submodules::utils::list_non_submodule_files_under;
use crate::git_submodules::utils::root_fsnode_id_from_submodule_git_commit;
use crate::git_submodules::utils::x_repo_submodule_metadata_file_basename;
use crate::types::Repo;
use crate::types::SubmodulePath;

/// A wrapper over BonsaiChangeset that can only be created by running submodule
/// expansion validation on a bonsai.
/// This type will be used as input of any functions that require a bonsai
/// to have all its submodule expansions already validated (e.g. backsyncing).
///
/// A token is also generated that can be passed to downstream calls that mutate
/// the bonsai but also want to ensure that it was previously validated.
pub struct ValidSubmoduleExpansionBonsai(BonsaiChangeset, SubmoduleExpansionValidationToken);
/// A type that can only be constructed in this module due to the private field
#[derive(Debug, Clone, Copy)]
pub struct SubmoduleExpansionValidationToken(());

impl ValidSubmoduleExpansionBonsai {
    /// Validate that a given bonsai **from the large repo** keeps all submodule
    /// expansions valid.
    pub async fn validate_all_submodule_expansions<'a, R: Repo>(
        ctx: &'a CoreContext,
        sm_exp_data: SubmoduleExpansionData<'a, R>,
        // Bonsai from the large repo that should have all submodule expansions
        // validated
        bonsai: BonsaiChangeset,
        // TODO(T179533620): fetch mover from commit sync config, instead of
        // requiring it to be provided by callers.
        mover: Arc<dyn Mover>,
    ) -> Result<ValidSubmoduleExpansionBonsai> {
        // For every submodule dependency, get all changes in their directories.

        // Iterate over the submodule dependency paths.
        // Create a map grouping the file changes per submodule dependency.

        let bonsai_res: Result<BonsaiChangeset> =
            stream::iter(sm_exp_data.submodule_deps.iter().map(anyhow::Ok))
                .try_fold(bonsai, |bonsai, (submodule_path, submodule_repo)| {
                    cloned!(mover, sm_exp_data);
                    async move {
                        validate_submodule_expansion(
                            ctx,
                            sm_exp_data,
                            bonsai,
                            submodule_path,
                            submodule_repo.as_ref(),
                            mover,
                        )
                        .timed()
                        .await
                        .log_future_stats(
                            ctx.scuba().clone(),
                            "Validating submodule expansion",
                            format!("Submodule path: {submodule_path}"),
                        )
                        .with_context(|| format!("Validation of submodule {submodule_path} failed"))
                    }
                })
                .await;

        if let Err(err) = &bonsai_res {
            log_error(ctx, format!("Submodule validation failed: {err:#?}"));
        }

        bonsai_res.map(|bonsai| {
            ValidSubmoduleExpansionBonsai(bonsai, SubmoduleExpansionValidationToken(()))
        })
    }

    pub fn into_inner(self) -> BonsaiChangeset {
        self.0
    }
    pub fn into_inner_with_token(self) -> (BonsaiChangeset, SubmoduleExpansionValidationToken) {
        (self.0, self.1)
    }
}

/// Validate that a bonsai in the large repo is valid for a given submodule repo
/// repo.
/// Among other things, it will assert that
/// 1. If the submodule expansion is changed, the submodule metadata file (i.e.
///    pointer) is updated as well.
/// 2. The submoldule metadata file exists, contains a valid git commit hash
///    and that commit exists in the submodule repo.
/// 3. The working copy of the commit in the submodule repo is exactly the same
///    as its expansion in the large repo.
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
    submodule_repo: &'a R,
    mover: Arc<dyn Mover>,
) -> Result<BonsaiChangeset> {
    trace!(
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
    let synced_submodule_path = mover.move_path(submodule_path)?.ok_or(anyhow!(
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
    let synced_submodule_path = synced_submodule_path.0;

    let fc_map = bonsai.file_changes_map();
    let mb_metadata_file_fc = fc_map.get(&metadata_file_path);

    let metadata_file_fc = match mb_metadata_file_fc {
        Some(fc) => fc,
        None => {
            if !submodule_expansion_changed {
                // Metadata file didn't change but its submodule expansion also
                // wasn't changed.
                // Return early in this case to avoid deriving fsnodes for
                // the large repo bonsai
                return Ok(bonsai);
            }

            // Check if the submodule metadata file existed in any of the
            // parents. If it did, it means that a submodule expansion is
            // being modified without properly updating the metadata file.
            let submodule_metadata_file_exists = stream::iter(bonsai.parents())
                .map(|cs_id| {
                    content_id_of_file_with_type(
                        ctx,
                        &sm_exp_data.large_repo,
                        cs_id,
                        &metadata_file_path,
                        FileType::Regular,
                    )
                })
                .buffer_unordered(10)
                .boxed()
                .try_collect::<Vec<_>>()
                .await?
                .into_iter()
                // If a content id is returned, the submodule metadata file
                // existed in the parent changeset
                .any(|mb_content_id| mb_content_id.is_some());

            // This means that the metadata file wasn't modified
            if submodule_metadata_file_exists {
                // Submodule expansion changed, but the metadata file wasn't updated
                return Err(anyhow!(
                    "Expansion of submodule {submodule_path} changed without updating its metadata file {metadata_file_path}"
                ));
            };

            // Path that might have been a submodule expansion before was
            // changed, but there wasn't a metadata file in a parent revision
            // so this path is not an expansion.
            return Ok(bonsai);
        }
    };

    let metadata_file_content_id = match metadata_file_fc {
        FileChange::Change(tfc) => tfc.content_id(),
        FileChange::UntrackedChange(bfc) => bfc.content_id(),
        FileChange::Deletion | FileChange::UntrackedDeletion => {
            // TODO(T187241943): ensure that submodule expansion is always
            // deleted when the metadata file is deleted during backsyncing.
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

    let large_repo = sm_exp_data.large_repo.clone();

    let git_hash =
        git_hash_from_submodule_metadata_file(ctx, &large_repo, metadata_file_content_id).await?;

    // This is the root fsnode from the submodule at the commit the submodule
    // metadata file points to.

    let submodule_fsnode_id = root_fsnode_id_from_submodule_git_commit(
        ctx,
        submodule_repo,
        git_hash,
        &sm_exp_data.dangling_submodule_pointers,
    )
    .timed()
    .await
    .log_future_stats(
        ctx.scuba().clone(),
        "Getting root fsnode id from submodule git commit",
        format!("Submodule repo: {}", &submodule_repo.repo_identity().name()),
    )?;

    // ------------------------------------------------------------------------
    // STEP 3: Get the fsnode from the expansion of the submodule in the large
    // repo and compare it with the fsnode from the submodule commit.

    let expansion_fsnode_id = get_submodule_expansion_fsnode_id(
        ctx,
        sm_exp_data.clone(),
        &bonsai,
        &synced_submodule_path,
    )
    .timed()
    .await
    .log_future_stats(
        ctx.scuba().clone(),
        "Get submodule expansion fsnode id",
        format!("Synced submodule path: {}", &synced_submodule_path),
    )
    .context("Failed to get submodule expansion fsnode id")?;

    if submodule_fsnode_id == expansion_fsnode_id {
        // If fsnodes are an exact match, there are no recursive submodules and the
        // working copy is the same.
        trace!("Root submodule expansion fsnode is the same as submodule repo's fsnode",);
        return Ok(bonsai);
    };

    // Build a new submodule deps map, removing the prefix of the submodule path
    // being validated, so it can be used to validate any recursive submodule
    // being expanded in it.
    let adjusted_submodule_deps =
        build_recursive_submodule_deps(sm_exp_data.submodule_deps, submodule_path);

    // The submodule roots fsnode and the fsnode from its expansion in the large
    // repo should be exactly the same.
    validate_working_copy_of_expansion_with_recursive_submodules(
        ctx,
        sm_exp_data,
        adjusted_submodule_deps,
        submodule_repo,
        expansion_fsnode_id,
        submodule_fsnode_id,
    )
    .timed()
    .await
    .log_future_stats(
        ctx.scuba().clone(),
        "Validate working copy of submodule expansion with recursive submodules",
        format!(
            "Recursive submodule: {}",
            submodule_repo.repo_identity().name()
        ),
    )?;

    Ok(bonsai)
}

// TODO(T187241943): ensure submodule expansion is always deleted when metadata
// file is deleted during backsyncing.
/// Ensures that, when the x-repo submodule metadata file was deleted, the
/// entire submodule expansion is deleted as well.
///
/// The submodule expansion can be deleted in two ways:
/// 1. Manually deleting the entire directory, in which case there must be
///    `FileChange::Deletion` for all the files in the expansion.
/// 2. Implicitly deleted by adding a file in the path of the expansion
///    directory.
async fn _ensure_submodule_expansion_deletion<'a, R: Repo>(
    ctx: &'a CoreContext,
    sm_exp_data: SubmoduleExpansionData<'a, R>,
    // Bonsai from the large repo
    bonsai: BonsaiChangeset,
    // Path of the submodule expansion in the large repo
    synced_submodule_path: NonRootMPath,
) -> Result<BonsaiChangeset> {
    let fc_map = bonsai.file_changes_map();

    // First check if the submodule was deleted implicitly, because it's quicker
    // than checking the deletion of the entire expansion directory.
    let was_expansion_deleted_implicitly = fc_map
        .get(&synced_submodule_path)
        .and_then(|fc| {
            match fc {
                // Submodule expansion is being implicitly deleted by adding a file
                // in the exact same place as the expansion
                FileChange::Change(_) | FileChange::UntrackedChange(_) => Some(fc),
                FileChange::Deletion | FileChange::UntrackedDeletion => None,
            }
        })
        .is_some();

    if was_expansion_deleted_implicitly {
        // Submodule expansion was deleted implicit, so bonsai should be valid
        return Ok(bonsai);
    }

    // Get all the files under the submodule expansion path in the parent
    // changesets.
    // A `FileChange::Deletion` should exist in the bonsai for all of these
    // paths.
    let entire_submodule_expansion_was_deleted = stream::iter(bonsai.parents())
        .map(|parent_cs_id| {
            cloned!(synced_submodule_path);
            let large_repo = sm_exp_data.large_repo.clone();
            async move {
                list_non_submodule_files_under(
                    ctx,
                    &large_repo,
                    parent_cs_id,
                    SubmodulePath(synced_submodule_path),
                )
                .await
            }
        })
        .buffer_unordered(10)
        .boxed()
        .try_flatten()
        .try_all(|path| {
            borrowed!(fc_map);
            async move {
                // Check if the path is being deleted in the bonsai
                if let Some(fc) = fc_map.get(&path) {
                    return fc.is_removed();
                }
                // Submodule expansion wasn't entirely deleted because at least
                // one file in it wasn't deleted.
                false
            }
        })
        .await?;

    if !entire_submodule_expansion_was_deleted {
        return Err(anyhow!(
            "Submodule metadata file is being deleted without removing the entire submodule expansion"
        ));
    }

    Ok(bonsai)
}

/// Get the fsnode of a submodule expansion in the large repo.
/// It will be used to compare it with the one from the submodule commit
/// being expanded.
async fn get_submodule_expansion_fsnode_id<'a, R: Repo>(
    ctx: &'a CoreContext,
    sm_exp_data: SubmoduleExpansionData<'a, R>,
    // Bonsai from the large repo
    bonsai: &'a BonsaiChangeset,
    synced_submodule_path: &NonRootMPath,
) -> Result<FsnodeId> {
    let large_repo = sm_exp_data.large_repo.clone();

    let large_repo_blobstore = large_repo.repo_blobstore_arc();
    let large_repo_derived_data = large_repo.repo_derived_data();

    // Get the root fsnodes from the parent commits, so the one from this commit
    // can be derived.
    let parent_root_fsnodes = stream::iter(bonsai.parents())
        .then(|cs_id| large_repo_derived_data.derive::<RootFsnodeId>(ctx, cs_id))
        .boxed()
        .try_collect::<Vec<_>>()
        .timed()
        .await
        .log_future_stats(
            ctx.scuba().clone(),
            "Deriving large repo bonsai parent's fsnode ids",
            format!("Synced submodule path: {}", synced_submodule_path),
        )
        .context("Failed to derive parent fsnodes in large repo")?;

    let large_derived_data_ctx = large_repo_derived_data.manager().derivation_context(None);

    let new_root_fsnode_id = RootFsnodeId::derive_single(
        ctx,
        &large_derived_data_ctx,
        // NOTE: deriving directly from the bonsai requires an owned type, so
        // the bonsai needs to be cloned.
        bonsai.clone(),
        parent_root_fsnodes,
        None,
    )
    .timed()
    .await
    .log_future_stats(
        ctx.scuba().clone(),
        "Deriving large repo bonsai root fsnode id",
        format!("Synced submodule path: {}", synced_submodule_path),
    )
    .context("Deriving root fsnode for new bonsai")?
    .into_fsnode_id();

    let expansion_fsnode_entry = new_root_fsnode_id
        .find_entry(
            ctx.clone(),
            large_repo_blobstore.clone(),
            synced_submodule_path.clone().into(),
        )
        .await
        .context("Getting fsnode entry for submodule expansion in target repo")?;

    let expansion_fsnode_id = match expansion_fsnode_entry {
        Some(Entry::Tree(fsnode_id)) => fsnode_id,
        Some(Entry::Leaf(_)) => {
            return Err(anyhow!(
                "Path of submodule expansion in large repo contains a file, not a directory"
            ));
        }
        None => {
            return Err(anyhow!(
                "No fsnode entry found in submodule expansion path in large repo"
            ));
        }
    };

    Ok(expansion_fsnode_id)
}

/// This will take the fsnode of a submodule expansion and the fsnode from the
/// commit that it's expanding from the submodule repo and will assert that
/// they're equivalent, accounting for expansion of any submodules.
#[async_recursion]
pub async fn validate_working_copy_of_expansion_with_recursive_submodules<'a, R>(
    ctx: &'a CoreContext,
    sm_exp_data: SubmoduleExpansionData<'a, R>,
    // TODO(T186874619): build recursive sm_exp_data and pass
    // `adjusted_submodule_deps`, as it's done in expansion module.
    // Small repo submodule dependencies, but with their paths adjusted to
    // account for recursive submodules.
    adjusted_submodule_deps: HashMap<NonRootMPath, Arc<R>>,
    submodule_repo: &'a R,
    expansion_fsnode_id: FsnodeId,
    submodule_fsnode_id: FsnodeId,
) -> Result<()>
where
    R: Repo,
{
    trace!(
        "Validating expansion working copy of submodule repo {0}",
        submodule_repo.repo_identity().name()
    );
    let large_repo = sm_exp_data.large_repo.clone();
    let large_repo_blobstore = large_repo.repo_blobstore_arc();
    let submodule_blobstore = submodule_repo.repo_blobstore_arc();

    let submodule_fsnode: Fsnode = submodule_fsnode_id
        .load(ctx, &submodule_blobstore)
        .await
        .context("Failed to load fsnode")?;
    let expansion_fsnode: Fsnode = expansion_fsnode_id
        .load(ctx, &large_repo_blobstore)
        .await
        .context("Failed to load fsnode")?;

    // STEP 1: get all the entries in each fsnode.
    let all_expansion_entries: HashSet<(MPathElement, FsnodeEntry)> =
        expansion_fsnode.into_subentries().into_iter().collect();

    let all_submodule_entries: HashSet<(MPathElement, FsnodeEntry)> =
        submodule_fsnode.into_subentries().into_iter().collect();

    // Remove all the entries that are exact match in both sides, which means
    // they pass validation.
    let submodule_only_entries = all_submodule_entries
        .difference(&all_expansion_entries)
        .cloned();

    let expansion_only_entries: HashMap<MPathElement, FsnodeEntry> = all_expansion_entries
        .difference(&all_submodule_entries)
        .cloned()
        .collect();

    // At this point we only have the entries that are not exact match

    // STEP 2: assert that there are no paths are in the submodule manifest
    // that are NOT in the expansion's manifest. This should never happen.
    // In the process, split all the submodule manifest entries into files and
    // directories, because the validation is different for each one.
    let (submodule_dirs, submodule_files): (HashMap<_, _>, HashMap<_, _>) = submodule_only_entries
        .into_iter()
        .map(|(path, entry)| {
            if !expansion_only_entries.contains_key(&path) {
                return Err(anyhow!(
                    "Path {path} is in submodule manifest but not in expansion"
                ));
            };
            Ok((path, entry))
        })
        .process_results(|iter| {
            iter.partition_map(|(path, entry)| match entry {
                FsnodeEntry::Directory(fsnode_dir) => Either::Left((path, fsnode_dir)),
                FsnodeEntry::File(fsnode_file) => Either::Right((path, fsnode_file)),
            })
        })?;

    // STEP 3: split expansion entries from paths that are present in submodule
    // manifest from the ones that aren't.
    let (should_contain_submodule_expansions, should_be_metadata_files): (
        Vec<(MPathElement, FsnodeEntry)>,
        Vec<(MPathElement, FsnodeEntry)>,
    ) = expansion_only_entries
        .into_iter()
        .partition(|(path, _entry)| {
            submodule_dirs.contains_key(path) || submodule_files.contains_key(path)
        });

    // The paths that are NOT in the submodule's manifest CAN ONLY BE submodule
    // metadata files.
    let (expected_metadata_files, unexpected_paths): (HashMap<_, _>, Vec<_>) =
        should_be_metadata_files
            .into_iter()
            .map(|(path, entry)| match entry {
                FsnodeEntry::File(fsnode_file) => Ok((path, fsnode_file)),
                FsnodeEntry::Directory(_) => Err(path),
            })
            .partition_result();

    if !unexpected_paths.is_empty() {
        log_error(
            ctx,
            format!(
                "Unexpected files in the expansion that are not in the submodule: {unexpected_paths:#?}",
            ),
        );
        return Err(anyhow!(
            "Found files in the expansion that are not in the submodule",
        ));
    }

    // The paths are are present in both, but their content doesn't match can be
    // either a submodule expansion or a directory that contains an expansion.
    // Either way, these paths can't be files.
    let expansion_directories = should_contain_submodule_expansions
        .into_iter()
        .map(|(path, entry)| match entry {
            FsnodeEntry::Directory(fsnode_file) => Ok((path, fsnode_file)),
            FsnodeEntry::File(_) => Err(anyhow!(
                "Path present in submodule manifest can't be a file in expansion"
            )),
        })
        .collect::<Result<HashMap<_, _>>>()?;

    // STEP 4: iterate over the expansion directories that don't match with the
    // the submodule and ensure that they fall in one of two expected scenarios:
    //
    // 1. They are submodule expansions
    // In this case we load the submodule repo being expanded, get its manifest
    // and call this function to validate its working copy.
    //
    // 2. They are a normal directory that contains an expansion
    // In this case, we just call this function passing that path, to repeat the
    // process until we get to the submodule expansion.
    //
    // In this process, every submodule expansion should have its metadata file,
    // so we keep track of all the files in the expansion that were not yet
    // processed.
    //
    // **All the files and directories from both the expansion and the submodule
    // manifest should be consumed (thus accounted for) in this step**.
    let EntryValidationData {
        remaining_sm_dirs: final_submodule_dirs,
        remaining_sm_files: final_submodule_files,
        remaining_md_files: final_expansion_only_files,
        entries_to_validate,
    } = stream::iter(expansion_directories.into_iter().map(anyhow::Ok))
        .try_fold(
            EntryValidationData {
                remaining_sm_dirs: submodule_dirs,
                remaining_sm_files: submodule_files,
                remaining_md_files: expected_metadata_files,
                entries_to_validate: Vec::new(),
            },
            |iteration_data: EntryValidationData<R>,
             (exp_path, exp_directory): (MPathElement, FsnodeDirectory)| {
                cloned!(sm_exp_data, adjusted_submodule_deps);
                borrowed!(submodule_repo);

                async move {
                    validate_expansion_directory_against_submodule_manifest_entry(
                        ctx,
                        sm_exp_data,
                        submodule_repo,
                        adjusted_submodule_deps,
                        iteration_data,
                        exp_path.clone(),
                        exp_directory,
                    )
                    .timed()
                    .await
                    .log_future_stats(
                        ctx.scuba().clone(),
                        "Validate expansion directory against submodule manifest entry",
                        format!(
                            "submodule_repo: {} / exp_path: {}",
                            submodule_repo.repo_identity().name(),
                            exp_path
                        ),
                    )
                    .context(
                        "Failed to validate expansion directory against submodule manifest entry",
                    )
                }
            },
        )
        .await?;

    trace!(
        "Remaining directories in submodule's manifest: {:#?}",
        final_submodule_dirs
    );
    trace!(
        "Remaining files in submodule's manifest: {:#?}",
        final_submodule_files
    );
    trace!(
        "Remaining files in expansion's manifest: {:#?}",
        final_expansion_only_files
    );

    /// Helper to assert that there are no unexpected files/directories in
    /// the submodule manifest or expansion manifests, and log/display these
    /// entries if they're there.
    fn check_for_unexpected_entries<T>(
        ctx: &CoreContext,
        entries: HashMap<MPathElement, T>,
        entry_kind: &str,
        location: &str,
    ) -> Result<()>
    where
        T: std::fmt::Debug,
    {
        if entries.is_empty() {
            // No unexpected entries
            return Ok(());
        }

        let unexpected_entries = entries.keys().sorted().collect::<Vec<_>>();
        log_error(
            ctx,
            format!(
                "{entry_kind} unaccounted for in {location}: {:#?}",
                unexpected_entries
            ),
        );

        Err(anyhow!(
            "{entry_kind} present in {location} are unaccounted for"
        ))
    }

    // STEP 5: ensure that all the paths in the submodule manifest were accounted
    // for.
    check_for_unexpected_entries(
        ctx,
        final_submodule_dirs,
        "Directories",
        "submodule manifest",
    )?;

    check_for_unexpected_entries(ctx, final_submodule_files, "Files", "submodule manifest")?;

    // Do the same for all the files in the expansion that don't exist in
    // the submodule manifest, because they should be metadata files that were
    // fetched to expand their submodule.
    check_for_unexpected_entries(ctx, final_expansion_only_files, "Files", "expansion")?;

    // STEP 6: actually perform the recursive validation calls
    stream::iter(entries_to_validate)
        .map(|entry_to_validate| {
            cloned!(sm_exp_data);
            let EntriesToValidate {
                rec_submodule_repo_deps,
                submodule_repo,
                expansion_fsnode_id,
                submodule_repo_fsnode_id,
            } = entry_to_validate;

            async move {
                validate_working_copy_of_expansion_with_recursive_submodules(
                    ctx,
                    sm_exp_data,
                    rec_submodule_repo_deps,
                    &submodule_repo,
                    expansion_fsnode_id,
                    submodule_repo_fsnode_id,
                )
                .await
            }
        })
        .buffer_unordered(100)
        .try_collect::<()>()
        .await?;

    Ok(())
}

// All the entries need to be processed sequentially, but we can store all
// the necessary arguments for a recursive validation call in this struct,
// so the actual validation calls can be done concurrently.
// Doing this means that any unnaccounted file will be flagged right away,
// without having to do the recursive calls.
struct EntriesToValidate<R: Repo> {
    rec_submodule_repo_deps: HashMap<NonRootMPath, Arc<R>>,
    submodule_repo: Arc<R>,
    expansion_fsnode_id: FsnodeId,
    submodule_repo_fsnode_id: FsnodeId,
}

/// Stores all the data for an iteration of the validation fold.
/// The data consists of the files and directories from expansion or submodule
/// manifest that haven't been matched yet and the recursive validation calls
/// that have to be made.
struct EntryValidationData<R: Repo> {
    /// Submodule directory entries that haven't been matched yet.
    remaining_sm_dirs: HashMap<MPathElement, FsnodeDirectory>,
    /// Submodule file entries that haven't been matched yet.
    remaining_sm_files: HashMap<MPathElement, FsnodeFile>,
    /// Expansion file entries that haven't been matched yet **and should be
    /// submodule metadata files**.
    remaining_md_files: HashMap<MPathElement, FsnodeFile>,
    /// Result of the manifest validation: recursive calls that should be made
    /// to validate either a recursive submodule or go further down in a
    /// directory to find a recursive submodule.
    entries_to_validate: Vec<EntriesToValidate<R>>,
}

// Extracted this to a separate function to avoid making the closure inside
// the fold statement too nested.
/// Validate a directory from the submodule expansion's manifest.
async fn validate_expansion_directory_against_submodule_manifest_entry<'a, R: Repo>(
    ctx: &'a CoreContext,
    sm_exp_data: SubmoduleExpansionData<'a, R>,
    submodule_repo: &'a R,
    // Small repo submodule dependencies, but with their paths adjusted to
    // account for recursive submodules.
    adjusted_submodule_deps: HashMap<NonRootMPath, Arc<R>>,
    entry_validation_res: EntryValidationData<R>,
    exp_path: MPathElement,
    exp_directory: FsnodeDirectory,
) -> Result<EntryValidationData<R>> {
    let EntryValidationData {
        mut remaining_sm_dirs,
        mut remaining_sm_files,
        mut remaining_md_files,
        mut entries_to_validate,
    } = entry_validation_res;

    let rec_submodule_repo_deps = build_recursive_submodule_deps(
        &adjusted_submodule_deps,
        &Into::<NonRootMPath>::into(exp_path.clone()),
    );

    let exp_dir_fsnode_id = *exp_directory.id();

    if let Some(submodule_dir) = remaining_sm_dirs.remove(&exp_path) {
        // This path in the expansion corresponds to a directory
        // in the submodule manifest.
        // This means that it must contain an expansion inside it,
        // so we just call the validation for it.
        entries_to_validate.push(EntriesToValidate {
            rec_submodule_repo_deps,
            submodule_repo: submodule_repo.clone().into(),
            expansion_fsnode_id: exp_dir_fsnode_id,
            submodule_repo_fsnode_id: *submodule_dir.id(),
        });

        return Ok(EntryValidationData {
            remaining_sm_dirs,
            remaining_sm_files,
            remaining_md_files,
            entries_to_validate,
        });
    };

    // If the path wasn't a directory in the submodule manifest,
    // it MUST be a file of type GitSubmodule.
    // This means that this path is a recursive submodule expansion,
    // so we load this submodule repo, get its manifest and
    // call the working copy validation for its expansion.
    let submodule_file = remaining_sm_files.remove(&exp_path).ok_or(anyhow!(
        "Path should be a GitSubmodule file in the submodule's manifest"
    ))?;

    // The file has to be of type GitSubmodule
    if *submodule_file.file_type() != FileType::GitSubmodule {
        return Err(anyhow!(
            "Submodule entry for the same path has to be a submodule file"
        ));
    };

    // If this path is an expansion, there MUST BE a submodule
    // metadata file for it. This would be its basename.
    let expected_metadata_basename: MPathElement = x_repo_submodule_metadata_file_basename(
        &exp_path,
        sm_exp_data.x_repo_submodule_metadata_file_prefix,
    )?;

    let metadata_file = remaining_md_files
        .remove(&expected_metadata_basename)
        .ok_or(
            anyhow!(
                "Metadata file {expected_metadata_basename} not found in path {exp_path} where expansion should be"
            ),
        )?;

    // Get the git hash from the metadata file , which represents
    // a pointer to the recursive submodule's commit being expanded.
    let exp_metadata_git_hash = git_hash_from_submodule_metadata_file(
        ctx,
        &sm_exp_data.large_repo,
        *metadata_file.content_id(),
    )
    .await?;

    // Get the git hash from the submodule file change and
    // ensure that it matches the one stored in the metadata file.
    let git_hash_from_sm_entry =
        get_git_hash_from_submodule_file(ctx, submodule_repo, *submodule_file.content_id()).await?;

    assert_eq!(
        exp_metadata_git_hash, git_hash_from_sm_entry,
        "Hash from submodule metadata file doesn't match the one in git submodule file in submodule repo"
    );

    let non_root_path: NonRootMPath = Into::<NonRootMPath>::into(exp_path.clone());

    let recursive_submodule_repo = adjusted_submodule_deps
        .get(&non_root_path)
        .ok_or(anyhow!("Recursive submodule not loaded"))?
        .clone();

    let rec_submodule_fsnode_id: FsnodeId = root_fsnode_id_from_submodule_git_commit(
        ctx,
        recursive_submodule_repo.as_ref(),
        exp_metadata_git_hash,
        &sm_exp_data.dangling_submodule_pointers,
    )
    .await?;

    // Validate the expansion of the recursive submodule
    entries_to_validate.push(EntriesToValidate {
        rec_submodule_repo_deps,
        submodule_repo: recursive_submodule_repo,
        expansion_fsnode_id: exp_dir_fsnode_id,
        submodule_repo_fsnode_id: rec_submodule_fsnode_id,
    });

    let result = EntryValidationData {
        remaining_sm_dirs,
        remaining_sm_files,
        remaining_md_files,
        entries_to_validate,
    };
    Ok(result)
}
