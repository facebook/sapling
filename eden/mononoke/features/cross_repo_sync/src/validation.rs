/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::fmt::Debug;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use anyhow::anyhow;
use anyhow::format_err;
use blobstore::Loadable;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateReason;
use bookmarks::BookmarksMaybeStaleExt;
use bulk_derivation::BulkDerivation;
use cloned::cloned;
use commit_transformation::SubmoduleDeps;
use commit_transformation::SubmodulePath;
use commit_transformation::git_submodules::InMemoryRepo;
use commit_transformation::git_submodules::SubmoduleExpansionData;
use commit_transformation::git_submodules::build_recursive_submodule_deps;
use commit_transformation::git_submodules::get_git_hash_from_submodule_file;
use commit_transformation::git_submodules::get_submodule_repo;
use commit_transformation::git_submodules::get_x_repo_submodule_metadata_file_path;
use commit_transformation::git_submodules::git_hash_from_submodule_metadata_file;
use commit_transformation::git_submodules::root_fsnode_id_from_submodule_git_commit;
use commit_transformation::git_submodules::validate_working_copy_of_expansion_with_recursive_submodules;
use context::CoreContext;
use fsnodes::RootFsnodeId;
use futures::TryStreamExt;
use futures::future;
use futures::future::FutureExt;
use futures::stream;
use futures::stream::StreamExt;
use live_commit_sync_config::LiveCommitSyncConfig;
use manifest::Entry;
use manifest::ManifestOps;
use mercurial_types::FileType;
use mercurial_types::MPath;
use metaconfig_types::CommitSyncConfigVersion;
use metaconfig_types::CommitSyncDirection;
use metaconfig_types::DefaultSmallToLargeCommitSyncPathAction;
use metaconfig_types::GitSubmodulesChangesAction;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::MPathElement;
use mononoke_types::NonRootMPath;
use mononoke_types::fsnode::Fsnode;
use mononoke_types::fsnode::FsnodeDirectory;
use mononoke_types::fsnode::FsnodeEntry;
use mononoke_types::fsnode::FsnodeFile;
use mononoke_types::typed_hash::FsnodeId;
use movers::Mover;
use regex::Regex;
use slog::debug;
use slog::error;
use slog::info;
use slog::warn;
use sorted_vector_map::SortedVectorMap;

use crate::commit_sync_config_utils::get_git_submodule_action_by_version;
use crate::commit_syncers_lib::Syncers;
use crate::commit_syncers_lib::submodule_metadata_file_prefix_and_dangling_pointers;
use crate::sync_commit::CommitSyncData;
use crate::types::Repo;
use crate::types::Source;
use crate::types::Target;

// NOTE: Occurrences of Option<NonRootMPath> in this file have not been replaced with MPath since such a
// replacement is only possible in cases where Option<NonRootMPath> is used to represent a path that can also
// be root. However, in this case the Some(_) and None variant of Option<NonRootMPath> are used to represent
// conditional logic, i.e. the code either does something or skips it based on None or Some.

/// Fast path verification doesn't walk every file in the repository, instead
/// it leverages FSNodes to compare hashes of entire directories. This was if
/// the repository verifies OK the verification is very fast.
///
/// NOTE: The implementation is a bit hacky due to the path mover functions
/// being originally designed with moving file paths not, directory paths. The
/// hack is mostly contained to wrap_mover_result function.
pub async fn verify_working_copy<'a, R: Repo>(
    ctx: &'a CoreContext,
    commit_sync_data: &'a CommitSyncData<R>,
    source_hash: ChangesetId,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
) -> Result<(), Error> {
    let (target_hash, version) =
        get_synced_commit(ctx.clone(), commit_sync_data, source_hash).await?;
    verify_working_copy_with_version(
        ctx,
        commit_sync_data,
        Source(source_hash),
        Target(target_hash),
        &version,
        live_commit_sync_config,
    )
    .await
}

pub async fn verify_working_copy_with_version<'a, R: Repo>(
    ctx: &'a CoreContext,
    commit_sync_data: &'a CommitSyncData<R>,
    source_hash: Source<ChangesetId>,
    target_hash: Target<ChangesetId>,
    version: &'a CommitSyncConfigVersion,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
) -> Result<(), Error> {
    info!(
        ctx.logger(),
        "target repo cs id: {}, mapping version: {}", target_hash, version
    );

    let source_repo = commit_sync_data.get_source_repo();
    let target_repo = commit_sync_data.get_target_repo();

    let source_root_fsnode_id = source_repo
        .repo_derived_data()
        .derive::<RootFsnodeId>(ctx, source_hash.0)
        .await?
        .into_fsnode_id();
    let target_root_fsnode_id = target_repo
        .repo_derived_data()
        .derive::<RootFsnodeId>(ctx, target_hash.0)
        .await?
        .into_fsnode_id();

    let (small_repo, large_repo, small_root_fsnode_id, large_root_fsnode_id, commit_sync_data) =
        match commit_sync_data.repos.get_direction() {
            CommitSyncDirection::Forward => (
                source_repo,
                target_repo,
                source_root_fsnode_id,
                target_root_fsnode_id,
                commit_sync_data.clone(),
            ),
            CommitSyncDirection::Backwards => (
                target_repo,
                source_repo,
                target_root_fsnode_id,
                source_root_fsnode_id,
                commit_sync_data.reverse(),
            ),
        };
    let submodules_action = get_git_submodule_action_by_version(
        ctx,
        live_commit_sync_config.clone(),
        version,
        small_repo.repo_identity().id(),
        large_repo.repo_identity().id(),
    )
    .await?;

    let submodule_deps = commit_sync_data.get_submodule_deps();
    let (x_repo_submodule_metadata_file_prefix, dangling_submodule_pointers) =
        submodule_metadata_file_prefix_and_dangling_pointers(
            small_repo.repo_identity().id(),
            version,
            live_commit_sync_config.clone(),
        )
        .await?;
    let fallback_repos = vec![Arc::new(small_repo.clone())]
        .into_iter()
        .chain(submodule_deps.repos())
        .collect::<Vec<_>>();
    let large_in_memory_repo = InMemoryRepo::from_repo(large_repo, fallback_repos)?;
    let sm_exp_data = match submodule_deps {
        SubmoduleDeps::ForSync(deps) => Some(SubmoduleExpansionData {
            submodule_deps: deps,
            x_repo_submodule_metadata_file_prefix: &x_repo_submodule_metadata_file_prefix,
            small_repo_id: small_repo.repo_identity().id(),
            large_repo: large_in_memory_repo,
            dangling_submodule_pointers,
        }),
        SubmoduleDeps::NotNeeded | SubmoduleDeps::NotAvailable => None,
    };
    let movers = commit_sync_data.get_movers_by_version(version).await?;
    let exp_and_metadata_paths = list_possible_expansion_and_metadata_paths(
        movers.mover.as_ref(),
        submodules_action,
        &sm_exp_data,
    )?;

    let large_repo_prefixes_to_visit =
        get_large_repo_prefixes_to_visit(&commit_sync_data, version, live_commit_sync_config)
            .await?;

    info!(ctx.logger(), "###");
    info!(
        ctx.logger(),
        "### Checking that all the paths from the repo {} are properly rewritten to {}",
        large_repo.repo_identity().name(),
        small_repo.repo_identity().name(),
    );
    info!(ctx.logger(), "###");

    verify_working_copy_inner(
        ctx,
        CommitSyncDirection::Backwards,
        Source(large_repo),
        large_root_fsnode_id,
        Target(small_repo),
        small_root_fsnode_id,
        movers.reverse_mover.as_ref(),
        large_repo_prefixes_to_visit.clone().into_iter().collect(),
        submodules_action,
        &sm_exp_data,
        &exp_and_metadata_paths,
    )
    .await?;

    info!(ctx.logger(), "###");
    info!(
        ctx.logger(),
        "### Checking that all the paths from the repo {} are properly rewritten to {}",
        small_repo.repo_identity().name(),
        large_repo.repo_identity().name(),
    );
    info!(ctx.logger(), "###");
    let small_repo_prefixes_to_visit = large_repo_prefixes_to_visit
        .into_iter()
        .map(|prefix| wrap_mover_result(movers.reverse_mover.as_ref(), &prefix))
        .collect::<Result<Vec<Option<Option<NonRootMPath>>>, Error>>()?
        .into_iter()
        .flatten()
        .collect();
    verify_working_copy_inner(
        ctx,
        CommitSyncDirection::Forward,
        Source(small_repo),
        small_root_fsnode_id,
        Target(large_repo),
        large_root_fsnode_id,
        movers.mover.as_ref(),
        small_repo_prefixes_to_visit,
        submodules_action,
        &sm_exp_data,
        &exp_and_metadata_paths,
    )
    .await?;
    info!(ctx.logger(), "all is well!");
    Ok(())
}

#[derive(Debug, Eq, PartialEq)]
enum RewriteMismatchElement {
    File((ContentId, FileType)),
    Directory,
    Nothing,
}

impl RewriteMismatchElement {
    fn printable_type(&self) -> &'static str {
        match self {
            RewriteMismatchElement::File(_) => "a file",
            RewriteMismatchElement::Directory => "a directory",
            RewriteMismatchElement::Nothing => "nonexistent",
        }
    }
}

enum ValidationOutputElement {
    RewriteMismatch {
        source: (Option<NonRootMPath>, RewriteMismatchElement),
        target: (Option<NonRootMPath>, RewriteMismatchElement),
    },
    SubmoduleExpansionMismatch(String),
}
use ValidationOutputElement::*;

type ValidationOutput = Vec<ValidationOutputElement>;

struct PrintableValidationOutput(Source<String>, Target<String>, ValidationOutput);

impl fmt::Display for PrintableValidationOutput {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let Self(Source(source_name), Target(target_name), output) = self;
        for item in output {
            match item {
                RewriteMismatch {
                    source: (source_path, source_element),
                    target: (target_path, target_element),
                } if std::mem::discriminant(source_element)
                    != std::mem::discriminant(target_element) =>
                {
                    writeln!(
                        f,
                        "{:?} is {} in {}, but {} in {} (under {:?})",
                        source_path,
                        source_element.printable_type(),
                        source_name,
                        target_element.printable_type(),
                        target_name,
                        target_path,
                    )?;
                }
                RewriteMismatch {
                    source: (source_path, RewriteMismatchElement::File((source_id, source_type))),
                    target: (target_path, RewriteMismatchElement::File((target_id, target_type))),
                } => {
                    writeln!(
                        f,
                        "file differs between {} (path: {:?}, content_id: {:?}, type: {:?}) and {} (path: {:?}, content_id: {:?}, type: {:?})",
                        source_name,
                        source_path,
                        source_id,
                        source_type,
                        target_name,
                        target_path,
                        target_id,
                        target_type,
                    )?;
                }
                RewriteMismatch {
                    source: (source_path, _),
                    target: (target_path, _),
                } => {
                    writeln!(
                        f,
                        "path differs between {} (path: {:?}) and {} (path: {:?})",
                        source_name, source_path, target_name, target_path,
                    )?;
                }
                SubmoduleExpansionMismatch(msg) => {
                    writeln!(f, "submodule expansion mismatch: {}", msg)?;
                }
            }
        }
        Ok(())
    }
}

async fn verify_working_copy_inner<'a>(
    ctx: &'a CoreContext,
    direction: CommitSyncDirection,
    source_repo: Source<&'a impl Repo>,
    source_root_fsnode_id: FsnodeId,
    target_repo: Target<&'a impl Repo>,
    target_root_fsnode_id: FsnodeId,
    mover: &dyn Mover,
    prefixes_to_visit: Vec<Option<NonRootMPath>>,
    submodules_action: GitSubmodulesChangesAction,
    sm_exp_data: &Option<SubmoduleExpansionData<'a, impl Repo>>,
    exp_and_metadata_paths: &ExpansionAndMetadataPaths,
) -> Result<(), Error> {
    let prefix_set: HashSet<_> = prefixes_to_visit
        .iter()
        .cloned()
        .filter_map(|p| p)
        .collect();
    let out = stream::iter(prefixes_to_visit.into_iter().map(|path| {
        verify_dir(
            ctx,
            direction,
            source_repo,
            path,
            source_root_fsnode_id.clone(),
            target_repo,
            target_root_fsnode_id.clone(),
            mover,
            &prefix_set,
            submodules_action,
            sm_exp_data,
            exp_and_metadata_paths,
        )
    }))
    .buffer_unordered(100)
    .try_fold(vec![], |mut acc, new_out| {
        acc.extend(new_out);
        future::ready(Ok(acc))
    })
    .await?;

    let len = out.len();
    if !out.is_empty() {
        error!(
            ctx.logger(),
            "Verification failed!!!\n{}",
            PrintableValidationOutput(
                Source(source_repo.0.repo_identity().name().to_string()),
                Target(target_repo.0.repo_identity().name().to_string()),
                out
            ),
        );
        return Err(format_err!(
            "verification failed, found {} differences",
            len
        ));
    }
    Ok(())
}

// ACHTUNG, HACK AHEAD!
// Movers were originally created to map file paths to file paths.  In
// validators we're abusing them to map directory path, in that case the case
// where dir is rewritten into repo root is a valid case and needs to be handled
// properly rather than error.
//
// This function returns:
//  * None when the path shouldn't be present after rewrite
//  * Some(None) when the dir should be rewritten into repo root
//  * Some(Some(path)) when the dir should be rewritten into path
//
// This function contains the "directory" rewriting to just validation crate
// while keeping all other mover code strict and safe. The alternative would be
// to make moves more lax and be able to deal with root paths (large refactor).
//
// Also, the function assumes that the repo root always rewrites to repo root.
// (which is true in the only usecase here: preserve mode)
fn wrap_mover_result(
    mover: &dyn Mover,
    path: &Option<NonRootMPath>,
) -> Result<Option<Option<NonRootMPath>>, Error> {
    match path {
        Some(mpath) => match mover.move_path(mpath) {
            Ok(opt_mpath) => Ok(opt_mpath.map(Some)),
            Err(err) => {
                for cause in err.chain() {
                    if let Some(movers::ErrorKind::RemovePrefixWholePathFailure) =
                        cause.downcast_ref::<movers::ErrorKind>()
                    {
                        return Ok(Some(None));
                    }
                }
                Err(err)
            }
        },
        None => Ok(None),
    }
}

/// Verify that submodule expansion in the repo is correct in small->large direction
/// i.e. for given git submodule in the small repo  (identified by its path and fsnode)
/// whether the metadata and expansion directory exist in the target repo and their contents
/// match the submodule contents.
async fn verify_git_submodule_expansion_small_to_large<'a>(
    ctx: &'a CoreContext,
    small_repo: Source<&'a impl Repo>,
    large_repo: Target<&'a impl Repo>,
    sm_exp_data: &Option<SubmoduleExpansionData<'a, impl Repo>>,
    mover: &dyn Mover,
    submodule_path: NonRootMPath,
    submodule_fsnode_file_entry: FsnodeFile,
    large_root_fsnode_id: FsnodeId,
) -> Result<Option<ValidationOutputElement>, Error> {
    // STEP 1: Assert that the submodule expansion data is available
    let sm_exp_data = sm_exp_data
        .as_ref()
        .ok_or(anyhow!("submodule expansion data needed for validation"))?;
    // STEP 2: Compute the expansion path and find is fsnode in the large repo
    let expansion_path = mover
        .move_path(&submodule_path)?
        .ok_or(anyhow!("submodule path rewrites to nothing!"))?;
    let expansion_fsnode_entry = large_root_fsnode_id
        .find_entry(
            ctx.clone(),
            large_repo.repo_blobstore_arc(),
            expansion_path.clone().into(),
        )
        .await?;
    let expansion_fsnode_id = expansion_fsnode_entry
        .ok_or(anyhow!("No submodule expansion fsnode entry in large repo"))?
        .into_tree()
        .ok_or(anyhow!("submodule path doesn't rewrite to a directory"))?;

    // STEP 3: Adjust the submodule deps
    let adjusted_submodule_deps =
        build_recursive_submodule_deps(sm_exp_data.submodule_deps, &submodule_path);

    // STEP 4: Get submodule repo
    let submodule_repo = get_submodule_repo(
        &SubmodulePath(submodule_path.clone()),
        sm_exp_data.submodule_deps,
    )?;

    // STEP 5: Get the submodule metadata file
    let metadata_file_path = get_x_repo_submodule_metadata_file_path(
        &SubmodulePath(expansion_path),
        sm_exp_data.x_repo_submodule_metadata_file_prefix,
    )?;

    let metadata_file_entry = large_root_fsnode_id
        .find_entry(
            ctx.clone(),
            large_repo.repo_blobstore_arc(),
            metadata_file_path.clone().into(),
        )
        .await?
        .ok_or_else(|| {
            anyhow!(
                "submodule metadata file not found in large repo: {:?}",
                &metadata_file_path
            )
        })?;

    let metadata_file = match metadata_file_entry {
        Entry::Leaf(file) => file,
        _ => {
            return Err(anyhow!(
                "submodule metadata path doesn't represent a file: {:?}",
                &metadata_file_path
            ));
        }
    };

    // STEP 6: Load and compare the commit hashes from metadata file and submodule
    let exp_metadata_git_hash = match git_hash_from_submodule_metadata_file(
        ctx,
        &sm_exp_data.large_repo,
        *metadata_file.content_id(),
    )
    .await
    {
        Ok(exp_metadata_git_hash) => exp_metadata_git_hash,
        Err(err) => {
            // TODO: distinguish validation errors from other errors
            return Ok(Some(SubmoduleExpansionMismatch(err.to_string())));
        }
    };
    let git_hash = get_git_hash_from_submodule_file(
        ctx,
        small_repo.0,
        *submodule_fsnode_file_entry.content_id(),
    )
    .await?;

    if git_hash != exp_metadata_git_hash {
        return Err(anyhow!(
            "submodule metadata file git hash {:?} doesn't match the hash in metadata file {:?}",
            git_hash,
            exp_metadata_git_hash,
        ));
    }

    // STEP 7: Load submodule fsnode id in submodule repo
    let submodule_fsnode_id = root_fsnode_id_from_submodule_git_commit(
        ctx,
        submodule_repo,
        git_hash,
        &sm_exp_data.dangling_submodule_pointers,
    )
    .await?;

    // STEP 8: Validate the expansion contents
    // TODO: distinguish validation errors from other errors
    if let Err(err) = validate_working_copy_of_expansion_with_recursive_submodules(
        ctx,
        sm_exp_data.clone(),
        adjusted_submodule_deps,
        submodule_repo,
        expansion_fsnode_id,
        submodule_fsnode_id,
    )
    .await
    {
        return Ok(Some(SubmoduleExpansionMismatch(err.to_string())));
    }
    Ok(None)
}

/// Verify that submodule expansion in the repo is correct in large->small direction
/// i.e. for given large repo submodule expansion and metadata file whether they
/// have a corresponding submodule node in the small repo and its content match
/// the submodule contents.
async fn verify_git_submodule_expansion_large_to_small<'a>(
    ctx: &'a CoreContext,
    small_repo: Target<&'a impl Repo>,
    mover: &dyn Mover,
    sm_exp_data: &Option<SubmoduleExpansionData<'a, impl Repo>>,
    small_root_fsnode_id: FsnodeId,
    expansion_path: NonRootMPath,
    expansion_fsnode_dir_entry: FsnodeDirectory,
    expansion_metadata_file: FsnodeFile,
) -> Result<Option<ValidationOutputElement>, Error> {
    // STEP 1: Assert that the submodule expansion data is available
    let sm_exp_data = sm_exp_data
        .as_ref()
        .ok_or(anyhow!("submodule expansion data needed for validation"))?;

    // STEP 2: Compute the submodule path and find is fsnode in the small repo
    let submodule_path = if let Some(submodule_path) = mover.move_path(&expansion_path)? {
        submodule_path
    } else {
        return Err(anyhow!("expansion path rewrites to nothing in small repo!"));
    };
    let submodule_fsnode_entry = small_root_fsnode_id
        .find_entry(
            ctx.clone(),
            small_repo.repo_blobstore_arc(),
            submodule_path.clone().into(),
        )
        .await?;
    let submodule_fsnode_file = submodule_fsnode_entry
        .ok_or(anyhow!(
            "No manifest entry in small repo for submodule path {}",
            &submodule_path
        ))?
        .into_leaf()
        .ok_or(anyhow!(
            "Small repo manifest entry for submodule path {} is not a leaf",
            &submodule_path
        ))?;

    if *submodule_fsnode_file.file_type() != FileType::GitSubmodule {
        return Err(anyhow!(
            "submodule path is not a git submodule: {}!",
            &submodule_path,
        ));
    }

    // STEP 3: Adjust the submodule deps
    let adjusted_submodule_deps =
        build_recursive_submodule_deps(sm_exp_data.submodule_deps, &submodule_path);

    // STEP 4: Get submodule repo
    let submodule_repo = get_submodule_repo(
        &SubmodulePath(submodule_path.clone()),
        sm_exp_data.submodule_deps,
    )?;

    // STEP 5: Load and compare the commit hashes from metadata file and submodule
    let exp_metadata_git_hash = match git_hash_from_submodule_metadata_file(
        ctx,
        &sm_exp_data.large_repo,
        *expansion_metadata_file.content_id(),
    )
    .await
    {
        Ok(exp_metadata_git_hash) => exp_metadata_git_hash,
        Err(err) => {
            // TODO: distinguish validation errors from other errors
            return Ok(Some(SubmoduleExpansionMismatch(err.to_string())));
        }
    };
    let git_hash =
        get_git_hash_from_submodule_file(ctx, small_repo.0, *submodule_fsnode_file.content_id())
            .await?;

    if git_hash != exp_metadata_git_hash {
        return Err(anyhow!(
            "submodule metadata file git hash {:?} doesn't match the hash in metadata file {:?}",
            git_hash,
            exp_metadata_git_hash,
        ));
    }

    // STEP 6: Load submodule fsnode id in submodule repo
    let submodule_fsnode_id = root_fsnode_id_from_submodule_git_commit(
        ctx,
        submodule_repo,
        git_hash,
        &sm_exp_data.dangling_submodule_pointers,
    )
    .await?;

    // STEP 7: Validate the expansion contents
    // TODO: distinguish validation errors from other errors
    if let Err(err) = validate_working_copy_of_expansion_with_recursive_submodules(
        ctx,
        sm_exp_data.clone(),
        adjusted_submodule_deps,
        submodule_repo,
        *expansion_fsnode_dir_entry.id(),
        submodule_fsnode_id,
    )
    .await
    {
        return Ok(Some(SubmoduleExpansionMismatch(err.to_string())));
    }
    Ok(None)
}

/// Datastructure that allows quick identification of submodule expansion paths
/// in the large repo and finding their corresponding metadata.
#[derive(Default, Debug)]
struct ExpansionAndMetadataPaths {
    expansion_path_to_metadata: SortedVectorMap<NonRootMPath, NonRootMPath>,
    metadata_path_to_expansion: SortedVectorMap<NonRootMPath, NonRootMPath>,
}

fn list_possible_expansion_and_metadata_paths<'a>(
    small_to_large_mover: &dyn Mover,
    submodules_action: GitSubmodulesChangesAction,
    sm_exp_data: &Option<SubmoduleExpansionData<'a, impl Repo>>,
) -> Result<ExpansionAndMetadataPaths, Error> {
    match submodules_action {
        GitSubmodulesChangesAction::Keep | GitSubmodulesChangesAction::Strip => {
            Ok(Default::default())
        }
        GitSubmodulesChangesAction::Expand => {
            let sm_exp_data = sm_exp_data
                .as_ref()
                .ok_or(anyhow!("submodule expansion data needed for validation"))?;
            let mut expansion_to_metadata = Vec::new();
            let mut metadata_to_expansion = Vec::new();

            for submodule_path in sm_exp_data.submodule_deps.keys() {
                let expansion_path =
                    if let Some(expansion_path) = small_to_large_mover.move_path(submodule_path)? {
                        expansion_path
                    } else {
                        return Err(anyhow!(
                            "submodule path rewrites to nothing in the large repo!"
                        ));
                    };
                let metadata_path = get_x_repo_submodule_metadata_file_path(
                    &SubmodulePath(expansion_path.clone()),
                    sm_exp_data.x_repo_submodule_metadata_file_prefix,
                )?;
                expansion_to_metadata.push((expansion_path.clone(), metadata_path.clone()));
                metadata_to_expansion.push((metadata_path, expansion_path));
            }
            Ok(ExpansionAndMetadataPaths {
                expansion_path_to_metadata: expansion_to_metadata.into_iter().collect(),
                metadata_path_to_expansion: metadata_to_expansion.into_iter().collect(),
            })
        }
    }
}

// Struct used for output of function below. Represents a
// submodule expansion directory and its metadata file.
struct SubmoduleExpansionDirectoryAndMetadata {
    expansion_path: NonRootMPath,
    expansion_fsnode_dir_entry: FsnodeDirectory,
    expansion_metadata_file: FsnodeFile,
}

enum ElemAction {
    Keep,
    Skip(Option<ValidationOutputElement>),
    VerifyExpansion(SubmoduleExpansionDirectoryAndMetadata),
}

// Determines if given entry is submodule expansion directory and returns
// pointers to directory and metadata.
// Determines whether further validation should skip the entry.
// Assumes that submodule expansion is enabled.
fn find_submodule_expansion(
    exp_and_metadata_paths: &ExpansionAndMetadataPaths,
    source_dir_path: &MPath,
    source_dir_map: &SortedVectorMap<MPathElement, FsnodeEntry>,
    elem: &MPathElement,
    entry: &FsnodeEntry,
) -> Result<ElemAction, Error> {
    // validation errors
    if let FsnodeEntry::File(fsnode_fileentry) = entry {
        // if submodule expansion is ON then the submodules have no business to exist in
        // the large repo
        if *fsnode_fileentry.file_type() == FileType::GitSubmodule {
            return Ok(ElemAction::Skip(Some(SubmoduleExpansionMismatch(
                "git submodules not allowed in large to small sync".to_string(),
            ))));
        }
    }
    let source_elem_path = source_dir_path.join_element(Some(elem)).try_into()?;

    if let Some(expansion_path) = exp_and_metadata_paths
        .metadata_path_to_expansion
        .get(&source_elem_path)
    {
        let expansion_metadata_file = if let FsnodeEntry::File(fsnode_fileentry) = entry {
            if *fsnode_fileentry.file_type() != FileType::Regular {
                return Ok(ElemAction::Skip(Some(SubmoduleExpansionMismatch(format!(
                    "git submodule expansion metadata file {} has to be a regular file",
                    &source_elem_path,
                )))));
            }
            fsnode_fileentry
        } else {
            return Ok(ElemAction::Skip(Some(SubmoduleExpansionMismatch(format!(
                "git submodule expansion metadata path {} has to be a file",
                &source_elem_path,
            )))));
        };
        if let Some(FsnodeEntry::Directory(expansion_fsnode_dir_entry)) =
            source_dir_map.get(expansion_path.basename())
        {
            return Ok(ElemAction::VerifyExpansion(
                SubmoduleExpansionDirectoryAndMetadata {
                    expansion_path: expansion_path.clone(),
                    expansion_fsnode_dir_entry: expansion_fsnode_dir_entry.clone(),
                    expansion_metadata_file: expansion_metadata_file.clone(),
                },
            ));
        } else {
            return Ok(ElemAction::Skip(Some(SubmoduleExpansionMismatch(format!(
                "submodule expansion directory not found in large repo: {:?} while submodule metadata file is present",
                &expansion_path,
            )))));
        }
    }
    if let Some(metadata_path) = exp_and_metadata_paths
        .expansion_path_to_metadata
        .get(&source_elem_path)
    {
        if let Some(_entry) = source_dir_map.get(metadata_path.basename()) {
            // here we just skip the expansion file and continue (as it was handled
            // together with metadata file earlier)
            return Ok(ElemAction::Skip(None));
        }
    }
    Ok(ElemAction::Keep)
}

/// Given a source and target directories fsnodes and a mover, verify that for all submodule
/// expansions (or submodules) in the source repo the expansion was done correctly. Also filter
/// those out so the rest of validation process will ignore them.
async fn verify_and_filter_out_submodule_changes<'a>(
    ctx: &'a CoreContext,
    direction: CommitSyncDirection,
    source_repo: Source<&'a impl Repo>,
    source_path: &MPath,
    source_dir: Fsnode,
    target_repo: Target<&'a impl Repo>,
    target_root_fsnode_id: FsnodeId,
    mover: &dyn Mover,
    submodules_action: GitSubmodulesChangesAction,
    sm_exp_data: &Option<SubmoduleExpansionData<'a, impl Repo>>,
    exp_and_metadata_paths: &ExpansionAndMetadataPaths,
) -> Result<
    (
        Vec<ValidationOutputElement>,
        Vec<(NonRootMPath, FsnodeEntry)>,
    ),
    Error,
> {
    // the filtered directory entries that will be returned
    let mut filtered_directory_entries = Vec::new();
    // validation errors
    let mut output_elements = vec![];
    // futures for submodule verification, we buffer them in this vector so we can run them in parallel
    let mut verification_futures = vec![];
    match direction {
        // large to small: find all expansions and their metadata files and call the
        // appropriate validation function
        CommitSyncDirection::Backwards => {
            match submodules_action {
                // in case of keep and strip there should be nothing in the large repo
                // that doesn't rewrite cleanly to small one with just mover - no filtering needed
                GitSubmodulesChangesAction::Keep | GitSubmodulesChangesAction::Strip => {
                    return Ok((
                        vec![],
                        source_dir
                            .into_subentries()
                            .into_iter()
                            .map(|(elem, entry)| {
                                (source_path.join_into_non_root_mpath(&elem), entry)
                            })
                            .collect::<Vec<_>>(),
                    ));
                }
                // rest of this block cares only about expand scenario
                // we're using match here rather than "if let" so the person adding
                // new variants of submodule changes action will get a compile time error
                GitSubmodulesChangesAction::Expand => (),
            };
            // this map will contain only the entries that are not submodule expansions or metadata files
            let source_dir_map = source_dir.clone().into_subentries();
            for (elem, entry) in source_dir.into_subentries() {
                let elem_action = find_submodule_expansion(
                    exp_and_metadata_paths,
                    source_path,
                    &source_dir_map,
                    &elem,
                    &entry,
                )?;
                match elem_action {
                    ElemAction::Keep => filtered_directory_entries.push((elem, entry)),
                    ElemAction::Skip(Some(output_elem)) => output_elements.push(output_elem),
                    ElemAction::Skip(None) => (),
                    ElemAction::VerifyExpansion(exp_and_metadata) => {
                        let verification_fut = verify_git_submodule_expansion_large_to_small(
                            ctx,
                            target_repo,
                            mover,
                            sm_exp_data,
                            target_root_fsnode_id,
                            exp_and_metadata.expansion_path,
                            exp_and_metadata.expansion_fsnode_dir_entry,
                            exp_and_metadata.expansion_metadata_file,
                        );
                        verification_futures.push(verification_fut.boxed());
                    }
                }
            }
        }
        // small to large is simpler: ws need to call validation for each submodule
        CommitSyncDirection::Forward => {
            for (elem, entry) in source_dir.into_subentries() {
                if let FsnodeEntry::File(fsnode_fileentry) = entry {
                    if *fsnode_fileentry.file_type() == FileType::GitSubmodule {
                        match submodules_action {
                            // when keeping submodules don't filter them out - we need a matching
                            // submodule on both sides of sync
                            GitSubmodulesChangesAction::Keep => (),
                            // for strip we just drop the submodule
                            GitSubmodulesChangesAction::Strip => {
                                continue;
                            }
                            // for expand call the validation function
                            GitSubmodulesChangesAction::Expand => {
                                let submodule_path =
                                    source_path.join_element(Some(&elem)).try_into()?;
                                verification_futures.push(
                                    verify_git_submodule_expansion_small_to_large(
                                        ctx,
                                        source_repo,
                                        target_repo,
                                        sm_exp_data,
                                        mover,
                                        submodule_path,
                                        fsnode_fileentry,
                                        target_root_fsnode_id,
                                    )
                                    .boxed(),
                                );
                                continue;
                            }
                        };
                    }
                }
                filtered_directory_entries.push((elem, entry));
            }
        }
    }
    let downstream_verification_output: Vec<ValidationOutputElement> =
        stream::iter(verification_futures)
            .buffered(10)
            .try_filter_map(|x| async { Ok(x) })
            .try_collect()
            .await?;
    output_elements.extend(downstream_verification_output);

    let filtered_directory_entries = filtered_directory_entries
        .into_iter()
        .map(|(elem, entry)| (source_path.join_into_non_root_mpath(&elem), entry))
        .collect::<Vec<_>>();
    Ok((output_elements, filtered_directory_entries))
}

async fn verify_dir<'a>(
    ctx: &'a CoreContext,
    direction: CommitSyncDirection,
    source_repo: Source<&'a impl Repo>,
    source_path: Option<NonRootMPath>,
    source_root_fsnode_id: FsnodeId,
    target_repo: Target<&'a impl Repo>,
    target_root_fsnode_id: FsnodeId,
    mover: &dyn Mover,
    prefixes_to_visit: &HashSet<NonRootMPath>,
    submodules_action: GitSubmodulesChangesAction,
    sm_exp_data: &Option<SubmoduleExpansionData<'a, impl Repo>>,
    exp_and_metadata_paths: &ExpansionAndMetadataPaths,
) -> Result<ValidationOutput, Error> {
    let source_blobstore = source_repo.repo_blobstore_arc();
    let target_blobstore = target_repo.repo_blobstore_arc();
    let maybe_source_manifest_entry = source_root_fsnode_id
        .find_entry(
            ctx.clone(),
            source_blobstore.clone(),
            source_path.clone().into(),
        )
        .await?;

    let mut outs = vec![];
    let inits = match maybe_source_manifest_entry {
        Some(source_entry) => match source_entry {
            Entry::Leaf(source_leaf) => {
                vec![(
                    source_path.clone().expect("leaf path can't be empty!"),
                    FsnodeEntry::File(source_leaf),
                )]
            }
            Entry::Tree(source_dir_fsnode_id) => {
                let source_dir = source_dir_fsnode_id.load(ctx, &source_blobstore).await?;
                let (validation_errors, filtered_source_dir) =
                    verify_and_filter_out_submodule_changes(
                        ctx,
                        direction,
                        source_repo,
                        &source_path.clone().into(),
                        source_dir,
                        target_repo,
                        target_root_fsnode_id,
                        mover,
                        submodules_action,
                        sm_exp_data,
                        exp_and_metadata_paths,
                    )
                    .await?;
                outs.extend(validation_errors);
                filtered_source_dir
            }
        },
        None => vec![],
    };
    let start_source_path = source_path;

    for init in inits {
        cloned!(start_source_path, source_blobstore, target_blobstore);
        let out = bounded_traversal::bounded_traversal(
            256,
            init,
            move |(source_path, source_entry)| {
                cloned!(start_source_path, source_blobstore, target_blobstore);
                Box::pin(async move {
                    let target_path = wrap_mover_result(mover, &Some(source_path.clone()))?;

                    if start_source_path.is_some_and(|p| p != source_path)
                        && (prefixes_to_visit.contains(&source_path))
                    {
                        return Ok((vec![], vec![]));
                    }

                    let target_path = if let Some(target_path) = target_path {
                        target_path
                    } else {
                        return Ok((vec![], vec![]));
                    };

                    let target_fsnode = target_root_fsnode_id
                        .find_entry(
                            ctx.clone(),
                            target_blobstore.clone(),
                            target_path.clone().into(),
                        )
                        .await?;

                    if let (
                        FsnodeEntry::Directory(source_dir),
                        Some(Entry::Tree(target_dir_fsnode_id)),
                    ) = (&source_entry, target_fsnode)
                    {
                        let target_dir = target_dir_fsnode_id.load(ctx, &target_blobstore).await?;
                        if source_dir.summary().simple_format_sha256
                            != target_dir.summary().simple_format_sha256
                        {
                            let source_dir = source_dir.id().load(ctx, &source_blobstore).await?;
                            let (validation_errors, recurse) =
                                verify_and_filter_out_submodule_changes(
                                    ctx,
                                    direction,
                                    source_repo,
                                    &source_path.clone().into(),
                                    source_dir,
                                    target_repo,
                                    target_root_fsnode_id,
                                    mover,
                                    submodules_action,
                                    sm_exp_data,
                                    exp_and_metadata_paths,
                                )
                                .await?;
                            return Ok((validation_errors, recurse));
                        } else {
                            return Ok((vec![], vec![]));
                        };
                    }
                    // The dir might not to map to the other side but if all subdirs map then we're good.
                    if let (FsnodeEntry::Directory(source_dir), None) =
                        (&source_entry, target_fsnode)
                    {
                        let source_dir = source_dir.id().load(ctx, &source_blobstore).await?;
                        let (validation_errors, recurse) = verify_and_filter_out_submodule_changes(
                            ctx,
                            direction,
                            source_repo,
                            &source_path.clone().into(),
                            source_dir,
                            target_repo,
                            target_root_fsnode_id,
                            mover,
                            submodules_action,
                            sm_exp_data,
                            exp_and_metadata_paths,
                        )
                        .await?;
                        return Ok((validation_errors, recurse));
                    }

                    let source_elem = match source_entry {
                        FsnodeEntry::File(source_file) => RewriteMismatchElement::File((
                            source_file.content_id().clone(),
                            source_file.file_type().clone(),
                        )),
                        FsnodeEntry::Directory(_dir) => RewriteMismatchElement::Directory,
                    };

                    let target_elem = match target_fsnode {
                        Some(Entry::Leaf(target_file)) => RewriteMismatchElement::File((
                            target_file.content_id().clone(),
                            target_file.file_type().clone(),
                        )),
                        Some(Entry::Tree(_id)) => RewriteMismatchElement::Directory,
                        None => RewriteMismatchElement::Nothing,
                    };

                    let output = if source_elem != target_elem {
                        vec![RewriteMismatch {
                            source: (Some(source_path), source_elem),
                            target: (target_path, target_elem),
                        }]
                    } else {
                        vec![]
                    };

                    Ok((output, vec![]))
                })
            },
            |mut output, child_outputs| {
                Box::pin(future::ready({
                    for child_output in child_outputs {
                        output.extend(child_output)
                    }
                    Ok::<_, Error>(output)
                }))
            },
        )
        .await?;
        outs.extend(out.into_iter());
    }

    Ok(outs)
}

// Returns list of prefixes that need to be visited in both large and small
// repositories to establish working copy equivalence.
async fn get_large_repo_prefixes_to_visit<'a, R: Repo>(
    commit_sync_data: &'a CommitSyncData<R>,
    version: &'a CommitSyncConfigVersion,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
) -> Result<Vec<Option<NonRootMPath>>, Error> {
    let small_repo_id = commit_sync_data.get_small_repo().repo_identity().id();
    let config = live_commit_sync_config
        .get_commit_sync_config_by_version(small_repo_id, version)
        .await?;

    let small_repo_config = config.small_repos.get(&small_repo_id).ok_or_else(|| {
        format_err!(
            "cannot find small repo id {} in commit sync config for {}",
            small_repo_id,
            version
        )
    })?;

    // Gets a list of large repo paths that small repo paths can map to.
    // All other large repo paths don't need visiting. Except for `Preserve` action.
    let mut prefixes_to_visit = small_repo_config
        .map
        .values()
        .cloned()
        .map(Some)
        .collect::<Vec<_>>();
    match &small_repo_config.default_action {
        DefaultSmallToLargeCommitSyncPathAction::Preserve => {
            prefixes_to_visit.push(None);
        }
        DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(prefix) => {
            prefixes_to_visit.push(Some(prefix.clone()));
        }
    }

    Ok(prefixes_to_visit)
}

/// This function returns what bookmarks are different between a source repo and a target repo.
/// Note that this is not just a trivial comparison, because this function also remaps all the
/// commits and renames bookmarks appropriately e.g. bookmark 'book' in source repo
/// might be renamed to bookmark 'prefix/book' in target repo, and commit A to which bookmark 'book'
/// points can be remapped to commit B in the target repo.
///
/// ```text
///  Source repo                Target repo
///
///   A <- "book"      <----->    B <- "prefix/book"
///   |                           |
///  ...                         ...
/// ```
pub async fn find_bookmark_diff<R: Repo>(
    ctx: CoreContext,
    commit_sync_data: &CommitSyncData<R>,
) -> Result<Vec<BookmarkDiff>, Error> {
    let source_repo = commit_sync_data.get_source_repo();
    let target_repo = commit_sync_data.get_target_repo();

    let target_bookmarks = target_repo
        .bookmarks()
        .get_publishing_bookmarks_maybe_stale(ctx.clone())
        .map_ok(|(bookmark, cs_id)| (bookmark.key().clone(), cs_id))
        .try_collect::<HashMap<_, _>>()
        .await?;

    // 'renamed_source_bookmarks' - take all the source bookmarks, rename the bookmarks, remap
    // the commits.
    let (renamed_source_bookmarks, no_sync_outcome) = {
        let source_bookmarks: Vec<_> = source_repo
            .bookmarks()
            .get_publishing_bookmarks_maybe_stale(ctx.clone())
            .map_ok(|(bookmark, cs_id)| (bookmark.key().clone(), cs_id))
            .try_collect()
            .await?;

        // Renames bookmarks and also maps large cs ids to small cs ids
        rename_and_remap_bookmarks(ctx.clone(), commit_sync_data, source_bookmarks).await?
    };

    let reverse_bookmark_renamer = commit_sync_data.get_reverse_bookmark_renamer().await?;
    let mut diff = vec![];
    for (target_book, target_cs_id) in &target_bookmarks {
        if no_sync_outcome.contains(target_book) {
            diff.push(BookmarkDiff::NoSyncOutcome {
                target_bookmark: target_book.clone(),
            });
            continue;
        }
        let corresponding_changesets = renamed_source_bookmarks.get(target_book);
        let remapped_source_cs_id = corresponding_changesets.map(|cs| cs.target_cs_id);
        if remapped_source_cs_id.is_none() && reverse_bookmark_renamer(target_book).is_none() {
            // Note that the reverse_bookmark_renamer check below is necessary because there
            // might be bookmark in the source repo that shouldn't be present in the target repo
            // at all. Without reverse_bookmark_renamer it's not possible to distinguish "bookmark
            // that shouldn't be in the target repo" and "bookmark that should be in the target
            // repo but is missing".
            continue;
        }

        if remapped_source_cs_id != Some(*target_cs_id) {
            diff.push(BookmarkDiff::InconsistentValue {
                target_bookmark: target_book.clone(),
                target_cs_id: target_cs_id.clone(),
                source_cs_id: corresponding_changesets.map(|cs| cs.source_cs_id),
            });
        }
    }

    // find all bookmarks that exist in source repo, but don't exist in target repo
    for (renamed_source_bookmark, corresponding_changesets) in renamed_source_bookmarks {
        if !target_bookmarks.contains_key(&renamed_source_bookmark) {
            diff.push(BookmarkDiff::MissingInTarget {
                target_bookmark: renamed_source_bookmark.clone(),
                source_cs_id: corresponding_changesets.source_cs_id,
            });
        }
    }

    Ok(diff)
}

/// Given a list of differences of a given type (`T`)
/// report them in the logs and return an appropriate result
pub fn report_different<
    T: Debug,
    E: ExactSizeIterator<Item = (NonRootMPath, Source<T>, Target<T>)>,
    I: IntoIterator<IntoIter = E, Item = <E as Iterator>::Item>,
>(
    ctx: &CoreContext,
    different_things: I,
    source_hash: &Source<ChangesetId>,
    name: &str,
    source_repo_name: Source<&str>,
    target_repo_name: Target<&str>,
) -> Result<(), Error> {
    let mut different_things = different_things.into_iter();
    let len = different_things.len();
    if len > 0 {
        // The very first value is preserved for error formatting
        let (mpath, source_thing, target_thing) = match different_things.next() {
            None => unreachable!("length of iterator is guaranteed to be >0"),
            Some((mpath, source_thing, target_thing)) => (mpath, source_thing, target_thing),
        };

        // And we also want a debug print of it
        debug!(
            ctx.logger(),
            "Different {} for path {:?}: {}: {:?} {}: {:?}",
            name,
            mpath,
            source_repo_name,
            source_thing,
            target_repo_name,
            target_thing
        );

        for (mpath, source_thing, target_thing) in different_things {
            debug!(
                ctx.logger(),
                "Different {} for path {:?}: {}: {:?} {}: {:?}",
                name,
                mpath,
                source_repo_name,
                source_thing,
                target_repo_name,
                target_thing
            );
        }

        Err(format_err!(
            "Found {} files with different {} in {} cs {} (example: {:?})",
            len,
            name,
            source_repo_name,
            source_hash,
            (mpath, source_thing, target_thing),
        ))
    } else {
        Ok(())
    }
}

async fn get_synced_commit<R: Repo>(
    ctx: CoreContext,
    commit_sync_data: &CommitSyncData<R>,
    hash: ChangesetId,
) -> Result<(ChangesetId, CommitSyncConfigVersion), Error> {
    let maybe_sync_outcome = commit_sync_data.get_commit_sync_outcome(&ctx, hash).await?;
    let sync_outcome = maybe_sync_outcome
        .ok_or_else(|| format_err!("No sync outcome for {} in {:?}", hash, commit_sync_data))?;

    use crate::commit_sync_outcome::CommitSyncOutcome::*;
    match sync_outcome {
        NotSyncCandidate(_) => Err(format_err!("{} does not remap in small repo", hash)),
        RewrittenAs(cs_id, mapping_version)
        | EquivalentWorkingCopyAncestor(cs_id, mapping_version) => Ok((cs_id, mapping_version)),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BookmarkDiff {
    InconsistentValue {
        target_bookmark: BookmarkKey,
        target_cs_id: ChangesetId,
        source_cs_id: Option<ChangesetId>,
    },
    MissingInTarget {
        target_bookmark: BookmarkKey,
        source_cs_id: ChangesetId,
    },
    NoSyncOutcome {
        target_bookmark: BookmarkKey,
    },
}

impl BookmarkDiff {
    pub fn target_bookmark(&self) -> &BookmarkKey {
        use BookmarkDiff::*;
        match self {
            InconsistentValue {
                target_bookmark, ..
            } => target_bookmark,
            MissingInTarget {
                target_bookmark, ..
            } => target_bookmark,
            NoSyncOutcome { target_bookmark } => target_bookmark,
        }
    }
}

struct CorrespondingChangesets {
    source_cs_id: ChangesetId,
    target_cs_id: ChangesetId,
}

async fn rename_and_remap_bookmarks<R: Repo>(
    ctx: CoreContext,
    commit_sync_data: &CommitSyncData<R>,
    bookmarks: impl IntoIterator<Item = (BookmarkKey, ChangesetId)>,
) -> Result<
    (
        HashMap<BookmarkKey, CorrespondingChangesets>,
        HashSet<BookmarkKey>,
    ),
    Error,
> {
    let mut renamed_and_remapped_bookmarks = vec![];
    for (bookmark, cs_id) in bookmarks {
        let mb_renamed_bookmark = commit_sync_data.rename_bookmark(&bookmark).await?;
        if let Some(renamed_bookmark) = mb_renamed_bookmark {
            let maybe_sync_outcome = commit_sync_data
                .get_commit_sync_outcome(&ctx, cs_id)
                .map(move |maybe_sync_outcome| {
                    let maybe_sync_outcome = maybe_sync_outcome?;
                    use crate::commit_sync_outcome::CommitSyncOutcome::*;

                    let maybe_remapped_cs_id = match maybe_sync_outcome {
                        Some(RewrittenAs(cs_id, _))
                        | Some(EquivalentWorkingCopyAncestor(cs_id, _)) => Some(cs_id),
                        Some(NotSyncCandidate(_)) => {
                            return Err(format_err!("{} is not a sync candidate", cs_id));
                        }
                        None => None,
                    };
                    let maybe_corresponding_changesets =
                        maybe_remapped_cs_id.map(|target_cs_id| CorrespondingChangesets {
                            source_cs_id: cs_id,
                            target_cs_id,
                        });
                    Ok((renamed_bookmark, maybe_corresponding_changesets))
                })
                .boxed();
            renamed_and_remapped_bookmarks.push(maybe_sync_outcome);
        }
    }

    let mut s = stream::iter(renamed_and_remapped_bookmarks).buffer_unordered(100);
    let mut remapped_bookmarks = HashMap::new();
    let mut no_sync_outcome = HashSet::new();

    while let Some(item) = s.next().await {
        let (renamed_bookmark, maybe_corresponding_changesets) = item?;
        match maybe_corresponding_changesets {
            Some(corresponding_changesets) => {
                remapped_bookmarks.insert(renamed_bookmark, corresponding_changesets);
            }
            None => {
                no_sync_outcome.insert(renamed_bookmark);
            }
        }
    }

    Ok((remapped_bookmarks, no_sync_outcome))
}

#[derive(Debug, Clone, PartialEq)]
pub enum VerifyBookmarksRunMode {
    JustVerify,
    UpdateLargeRepoBookmarks {
        limit: Option<usize>,
        mode: UpdateLargeRepoBookmarksMode,
    },
}

pub async fn verify_bookmarks<R: Repo>(
    ctx: &CoreContext,
    syncers: Syncers<R>,
    run_mode: VerifyBookmarksRunMode,
    mb_bookmark_regex: Option<Regex>,
) -> Result<(), Error> {
    let diff = find_bookmark_diff(ctx.clone(), &syncers.large_to_small).await?;

    let diff: Vec<_> = diff
        .into_iter()
        .filter(|d| {
            mb_bookmark_regex
                .as_ref()
                .is_none_or(|bookmark_regex| bookmark_regex.is_match(d.target_bookmark().as_str()))
        })
        .collect();

    let source_repo_name = syncers
        .large_to_small
        .get_source_repo()
        .repo_identity()
        .name();
    let target_repo_name = syncers
        .large_to_small
        .get_target_repo()
        .repo_identity()
        .name();

    if diff.is_empty() {
        info!(ctx.logger(), "all is well!");
        return Ok(());
    }

    match run_mode {
        VerifyBookmarksRunMode::UpdateLargeRepoBookmarks { mode, limit } => {
            update_large_repo_bookmarks(ctx, &diff, &syncers, mode, limit).await?;
            Ok(())
        }
        VerifyBookmarksRunMode::JustVerify => {
            for d in &diff {
                use BookmarkDiff::*;
                match d {
                    InconsistentValue {
                        target_bookmark,
                        target_cs_id,
                        source_cs_id,
                    } => {
                        warn!(
                            ctx.logger(),
                            "inconsistent value of {}: '{}' has {}, but '{}' bookmark points to {:?}",
                            target_bookmark,
                            target_repo_name,
                            target_cs_id,
                            source_repo_name,
                            source_cs_id,
                        );
                    }
                    MissingInTarget {
                        target_bookmark,
                        source_cs_id,
                    } => {
                        warn!(
                            ctx.logger(),
                            "'{}' doesn't have bookmark {} but '{}' has it and it points to {}",
                            target_repo_name,
                            target_bookmark,
                            source_repo_name,
                            source_cs_id,
                        );
                    }
                    NoSyncOutcome { target_bookmark } => {
                        warn!(
                            ctx.logger(),
                            "'{}' has a bookmark {} but it points to a commit that has no \
                            equivalent in '{}'. If it's a shared bookmark (e.g. master) \
                            that might mean that it points to a commit from another repository",
                            target_repo_name,
                            target_bookmark,
                            source_repo_name,
                        );
                    }
                }
            }
            Err(format_err!("found {} inconsistencies", diff.len()))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UpdateLargeRepoBookmarksMode {
    Real,
    DryRun,
}

pub async fn update_large_repo_bookmarks<'a, R: Repo>(
    ctx: &CoreContext,
    diff: &'a [BookmarkDiff],
    syncers: &'a Syncers<R>,
    update_mode: UpdateLargeRepoBookmarksMode,
    limit: Option<usize>,
) -> Result<(), Error> {
    let large_repo = syncers.small_to_large.get_large_repo();
    let common_commit_sync_config = large_repo
        .repo_cross_repo()
        .live_commit_sync_config()
        .get_common_config(large_repo.repo_identity().id())?;

    let mut book_txn = large_repo.bookmarks().create_transaction(ctx.clone());

    let bookmark_renamer =
        move |bookmark: &'a BookmarkKey| syncers.small_to_large.rename_bookmark(bookmark);

    let diff: Box<dyn Iterator<Item = &BookmarkDiff> + Send> = match limit {
        Some(limit) => {
            warn!(
                ctx.logger(),
                "found {} inconsistencies, will update at most {} of them...",
                diff.len(),
                limit
            );
            Box::new(diff.iter().take(limit))
        }
        None => {
            warn!(
                ctx.logger(),
                "found {} inconsistencies, trying to update them...",
                diff.len()
            );
            Box::new(diff.iter())
        }
    };
    for d in diff {
        if common_commit_sync_config
            .common_pushrebase_bookmarks
            .contains(d.target_bookmark())
        {
            info!(
                ctx.logger(),
                "skipping {} because it's a common bookmark",
                d.target_bookmark()
            );
            continue;
        }

        use BookmarkDiff::*;
        match d {
            InconsistentValue {
                target_bookmark,
                target_cs_id,
                ..
            } => {
                let outcomes = syncers
                    .small_to_large
                    .get_plural_commit_sync_outcome(ctx, *target_cs_id)
                    .await?
                    .with_context(|| {
                        format!("Missing outcome for {} from small repo", target_cs_id)
                    })?;

                use crate::commit_sync_outcome::PluralCommitSyncOutcome::*;
                let new_value = match outcomes {
                    NotSyncCandidate(..) => {
                        warn!(
                            ctx.logger(),
                            "{} from small repo doesn't remap to large repo", target_cs_id,
                        );
                        None
                    }
                    EquivalentWorkingCopyAncestor(large_cs_id, _) => Some(large_cs_id),
                    RewrittenAs(rewritten_commits) if rewritten_commits.len() == 1 => {
                        Some(rewritten_commits.into_iter().next().unwrap().0)
                    }
                    RewrittenAs(rewritten_commits) => {
                        return Err(format_err!(
                            "multiple remappings of {} in {}: {:?}",
                            *target_cs_id,
                            large_repo.repo_identity().name(),
                            rewritten_commits,
                        ));
                    }
                };

                if let Some(large_cs_id) = new_value {
                    let derived_data_types = large_repo
                        .repo_derived_data()
                        .active_config()
                        .types
                        .iter()
                        .copied()
                        .collect::<Vec<_>>();
                    large_repo
                        .repo_derived_data()
                        .manager()
                        .derive_bulk_locally(ctx, &[large_cs_id], None, &derived_data_types, None)
                        .await?;
                    let reason = BookmarkUpdateReason::XRepoSync;
                    let large_bookmark =
                        bookmark_renamer(target_bookmark).await?.ok_or_else(|| {
                            format_err!("small bookmark {} remaps to nothing", target_bookmark)
                        })?;

                    info!(ctx.logger(), "setting {} {}", large_bookmark, large_cs_id);
                    if update_mode == UpdateLargeRepoBookmarksMode::Real {
                        book_txn.force_set(&large_bookmark, large_cs_id, reason)?;
                    }
                }
            }
            MissingInTarget {
                target_bookmark, ..
            } => {
                warn!(
                    ctx.logger(),
                    "large repo bookmark (renames to {}) not found in small repo", target_bookmark,
                );
                let large_bookmark = bookmark_renamer(target_bookmark).await?.ok_or_else(|| {
                    format_err!("small bookmark {} remaps to nothing", target_bookmark)
                })?;
                let reason = BookmarkUpdateReason::XRepoSync;
                info!(ctx.logger(), "deleting {}", large_bookmark);
                if update_mode == UpdateLargeRepoBookmarksMode::Real {
                    book_txn.force_delete(&large_bookmark, reason)?;
                }
            }
            NoSyncOutcome { target_bookmark } => {
                warn!(
                    ctx.logger(),
                    "Not updating {} because it points to a commit that has no \
                     equivalent in source repo.",
                    target_bookmark,
                );
            }
        }
    }

    book_txn.commit().await?;
    Ok(())
}

#[cfg(test)]
mod test {
    use std::str::FromStr;
    use std::sync::Arc;

    use ascii::AsciiString;
    use bookmarks::BookmarkKey;
    use commit_transformation::SubmoduleDeps;
    use fbinit::FacebookInit;
    use fixtures::Linear;
    use fixtures::TestRepoFixture;
    use live_commit_sync_config::TestLiveCommitSyncConfig;
    use maplit::hashmap;
    use maplit::hashset;
    use mercurial_types::HgChangesetId;
    use metaconfig_types::CommitSyncConfig;
    use metaconfig_types::CommitSyncConfigVersion;
    use metaconfig_types::CommitSyncDirection;
    use metaconfig_types::CommonCommitSyncConfig;
    use metaconfig_types::SmallRepoCommitSyncConfig;
    use metaconfig_types::SmallRepoPermanentConfig;
    use mononoke_macros::mononoke;
    use mononoke_types::NonRootMPath;
    use mononoke_types::RepositoryId;
    use rendezvous::RendezVousOptions;
    use sql_construct::SqlConstruct;
    use synced_commit_mapping::SqlSyncedCommitMappingBuilder;
    use synced_commit_mapping::SyncedCommitMapping;
    use synced_commit_mapping::SyncedCommitMappingEntry;
    use test_repo_factory::TestRepoFactory;
    use tests_utils::CreateCommitContext;
    use tests_utils::bookmark;

    use super::*;
    use crate::commit_syncers_lib::CommitSyncRepos;
    use crate::commit_syncers_lib::create_commit_syncers;
    use crate::test_utils::TestRepo;
    use crate::test_utils::get_live_commit_sync_config;

    #[mononoke::fbinit_test]
    async fn test_bookmark_diff_with_renamer(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let (syncers, _config) = init(fb, CommitSyncDirection::Backwards).await?;
        let commit_sync_data = syncers.large_to_small;

        let small_repo = commit_sync_data.get_small_repo();
        let large_repo = commit_sync_data.get_large_repo();

        let another_hash = "607314ef579bd2407752361ba1b0c1729d08b281";
        bookmark(&ctx, &small_repo, "newbook")
            .set_to(another_hash)
            .await?;
        bookmark(&ctx, &large_repo, "prefix/newbook")
            .set_to(another_hash)
            .await?;
        let actual_diff = find_bookmark_diff(ctx.clone(), &commit_sync_data).await?;
        assert!(actual_diff.is_empty());

        bookmark(&ctx, &small_repo, "somebook")
            .set_to(another_hash)
            .await?;
        bookmark(&ctx, &large_repo, "somebook")
            .set_to(another_hash)
            .await?;

        let actual_diff = find_bookmark_diff(ctx.clone(), &commit_sync_data).await?;
        assert!(!actual_diff.is_empty());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_bookmark_diff_with_updates(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let (syncers, live_commit_sync_config) = init(fb, CommitSyncDirection::Backwards).await?;
        let commit_sync_data = &syncers.large_to_small;

        let small_repo = commit_sync_data.get_small_repo();
        let large_repo = commit_sync_data.get_large_repo();

        let master = BookmarkKey::new("master")?;

        let maybe_master_val = small_repo
            .bookmarks()
            .get(ctx.clone(), &master, bookmarks::Freshness::MostRecent)
            .await?;
        let master_val = maybe_master_val.ok_or_else(|| Error::msg("master not found"))?;

        // Everything is identical - no diff at all
        {
            let diff = find_bookmark_diff(ctx.clone(), commit_sync_data).await?;

            assert!(diff.is_empty());
        }

        // Move bookmark to another changeset
        let another_hash = "607314ef579bd2407752361ba1b0c1729d08b281";
        bookmark(&ctx, &small_repo, master.clone())
            .set_to(another_hash)
            .await?;
        let another_bcs_id = small_repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(&ctx, HgChangesetId::from_str(another_hash)?)
            .await?
            .expect("bonsai hg mapping not found for another_hash");

        let actual_diff = find_bookmark_diff(ctx.clone(), commit_sync_data).await?;

        let mut expected_diff = hashset! {
            BookmarkDiff::InconsistentValue {
                target_bookmark: master.clone(),
                target_cs_id: another_bcs_id,
                source_cs_id: Some(master_val),
            }
        };
        assert!(!actual_diff.is_empty());
        assert_eq!(
            actual_diff.into_iter().collect::<HashSet<_>>(),
            expected_diff,
        );

        // Create another bookmark
        let another_book = BookmarkKey::new("newbook")?;
        bookmark(&ctx, &small_repo, another_book.clone())
            .set_to(another_hash)
            .await?;

        let actual_diff = find_bookmark_diff(ctx.clone(), commit_sync_data).await?;

        expected_diff.insert(BookmarkDiff::InconsistentValue {
            target_bookmark: another_book,
            target_cs_id: another_bcs_id,
            source_cs_id: None,
        });
        assert_eq!(
            actual_diff.clone().into_iter().collect::<HashSet<_>>(),
            expected_diff
        );

        // Update the bookmarks
        {
            let mut common_config = CommonCommitSyncConfig {
                common_pushrebase_bookmarks: vec![master.clone()],
                small_repos: hashmap! {
                    small_repo.repo_identity().id() => SmallRepoPermanentConfig {
                        bookmark_prefix: Default::default(),
                        common_pushrebase_bookmarks_map: Default::default(),
                    },
                },
                large_repo_id: large_repo.repo_identity().id(),
            };
            live_commit_sync_config
                .source
                .replace_common_configs(vec![common_config.clone()]);

            update_large_repo_bookmarks(
                &ctx,
                &actual_diff,
                &syncers,
                UpdateLargeRepoBookmarksMode::Real,
                None,
            )
            .await?;

            let actual_diff = find_bookmark_diff(ctx.clone(), commit_sync_data).await?;

            // Master bookmark hasn't been updated because it's a common pushrebase bookmark
            let expected_diff = hashset! {
                BookmarkDiff::InconsistentValue {
                    target_bookmark: master.clone(),
                    target_cs_id: another_bcs_id,
                    source_cs_id: Some(master_val),
                }
            };
            assert_eq!(
                actual_diff.clone().into_iter().collect::<HashSet<_>>(),
                expected_diff,
            );

            // Now remove master bookmark from common_pushrebase_bookmarks and update large repo
            // bookmarks again
            common_config.common_pushrebase_bookmarks = vec![];
            live_commit_sync_config
                .source
                .replace_common_configs(vec![common_config]);

            update_large_repo_bookmarks(
                &ctx,
                &actual_diff,
                &syncers,
                UpdateLargeRepoBookmarksMode::Real,
                None,
            )
            .await?;
            let actual_diff = find_bookmark_diff(ctx.clone(), commit_sync_data).await?;
            assert!(actual_diff.is_empty());
        }
        Ok(())
    }

    #[mononoke::fbinit_test]
    fn test_bookmark_small_to_large(fb: FacebookInit) -> Result<(), Error> {
        let runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(test_bookmark_small_to_large_impl(fb))
    }

    async fn test_bookmark_small_to_large_impl(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let (syncers, _config) = init(fb, CommitSyncDirection::Forward).await?;
        let commit_sync_data = syncers.small_to_large;

        let large_repo = commit_sync_data.get_large_repo();

        // This bookmark is not present in the small repo, and it shouldn't be.
        // In that case
        bookmark(&ctx, &large_repo, "bookmarkfromanothersmallrepo")
            .set_to("master")
            .await?;

        let actual_diff = find_bookmark_diff(ctx.clone(), &commit_sync_data).await?;
        assert_eq!(actual_diff, vec![]);
        Ok(())
    }

    #[mononoke::fbinit_test]
    fn test_bookmark_no_sync_outcome(fb: FacebookInit) -> Result<(), Error> {
        let runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(test_bookmark_no_sync_outcome_impl(fb))
    }

    async fn test_bookmark_no_sync_outcome_impl(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let (syncers, _config) = init(fb, CommitSyncDirection::Backwards).await?;
        let commit_sync_data = syncers.large_to_small;

        let large_repo = commit_sync_data.get_large_repo();

        let commit = CreateCommitContext::new(&ctx, &large_repo, vec!["master"])
            .add_file("somefile", "ololo")
            .commit()
            .await?;
        // This bookmark is not present in the small repo, and it shouldn't be.
        // In that case
        bookmark(&ctx, &large_repo, "master").set_to(commit).await?;

        let actual_diff = find_bookmark_diff(ctx.clone(), &commit_sync_data).await?;
        assert_eq!(
            actual_diff,
            vec![BookmarkDiff::NoSyncOutcome {
                target_bookmark: BookmarkKey::new("master")?,
            }]
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_verify_working_copy(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let (syncers, live_commit_sync_config) = init(fb, CommitSyncDirection::Backwards).await?;
        let commit_sync_data = syncers.large_to_small;

        let source_cs_id = CreateCommitContext::new_root(&ctx, &commit_sync_data.get_large_repo())
            .add_file("prefix/file1", "1")
            .add_file("prefix/file2", "2")
            .commit()
            .await?;

        let target_cs_id = CreateCommitContext::new_root(&ctx, &commit_sync_data.get_small_repo())
            .add_file("file1", "1")
            .commit()
            .await?;

        let res = verify_working_copy_with_version(
            &ctx,
            &commit_sync_data,
            Source(source_cs_id),
            Target(target_cs_id),
            &CommitSyncConfigVersion("prefix".to_string()),
            live_commit_sync_config.clone(),
        )
        .await;

        assert!(res.is_err());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_verify_working_copy_with_prefixes(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let (syncers, live_commit_sync_config) = init(fb, CommitSyncDirection::Backwards).await?;
        let commit_sync_data = syncers.large_to_small;

        let source_cs_id = CreateCommitContext::new_root(&ctx, &commit_sync_data.get_large_repo())
            .add_file("prefix/sub/file1", "1")
            .add_file("prefix/sub/file2", "2")
            .add_file("prefix/file1", "1")
            .commit()
            .await?;

        let target_cs_id = CreateCommitContext::new_root(&ctx, &commit_sync_data.get_small_repo())
            .add_file("sub/file1", "1")
            .add_file("sub/file2", "2")
            .add_file("file1", "someothercontent")
            .commit()
            .await?;

        let res = verify_working_copy_with_version(
            &ctx,
            &commit_sync_data,
            Source(source_cs_id),
            Target(target_cs_id),
            &CommitSyncConfigVersion("prefix".to_string()),
            live_commit_sync_config.clone(),
        )
        .await;

        assert!(res.is_err());

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_verify_working_copy_fp(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut factory = TestRepoFactory::new(fb)?;
        let large_repo = factory.with_id(RepositoryId::new(0)).build().await?;
        let root_large_cs_id = CreateCommitContext::new_root(&ctx, &large_repo)
            .add_file("prefix/sub/file1", "1")
            .add_file("somefile", "content")
            .commit()
            .await?;
        let first_large_cs_id = CreateCommitContext::new(&ctx, &large_repo, vec![root_large_cs_id])
            .add_file("prefix/sub/file2", "1")
            .commit()
            .await?;
        let second_large_cs_id =
            CreateCommitContext::new(&ctx, &large_repo, vec![first_large_cs_id])
                .add_file("special/1", "special")
                .commit()
                .await?;

        let small_repo: TestRepo = factory.with_id(RepositoryId::new(1)).build().await?;
        let root_small_cs_id = CreateCommitContext::new_root(&ctx, &small_repo)
            .add_file("sub/file1", "1")
            .commit()
            .await?;
        let first_small_cs_id = CreateCommitContext::new(&ctx, &small_repo, vec![root_small_cs_id])
            .add_file("sub/file2", "1")
            .commit()
            .await?;
        let second_small_cs_id =
            CreateCommitContext::new(&ctx, &small_repo, vec![first_small_cs_id])
                .add_file("special/1", "special")
                .commit()
                .await?;

        let repos = CommitSyncRepos::new(
            small_repo,
            large_repo,
            CommitSyncDirection::Backwards,
            SubmoduleDeps::ForSync(HashMap::new()),
        );

        let live_commit_sync_config = get_live_commit_sync_config();

        let commit_sync_data = CommitSyncData::new(&ctx, repos, live_commit_sync_config.clone());

        println!("checking root commit");
        for version in &["first_version", "second_version"] {
            println!("version: {}", version);
            verify_working_copy_with_version(
                &ctx,
                &commit_sync_data,
                Source(root_large_cs_id),
                Target(root_small_cs_id),
                &CommitSyncConfigVersion(version.to_string()),
                live_commit_sync_config.clone(),
            )
            .await?;
        }

        println!("checking first commit");
        for version in &["first_version", "second_version"] {
            println!("version: {}", version);
            verify_working_copy_with_version(
                &ctx,
                &commit_sync_data,
                Source(first_large_cs_id),
                Target(first_small_cs_id),
                &CommitSyncConfigVersion(version.to_string()),
                live_commit_sync_config.clone(),
            )
            .await?;
        }

        let version = "second_version";
        println!("checking second commit, version: {}", version);
        verify_working_copy_with_version(
            &ctx,
            &commit_sync_data,
            Source(second_large_cs_id),
            Target(second_small_cs_id),
            &CommitSyncConfigVersion(version.to_string()),
            live_commit_sync_config.clone(),
        )
        .await?;

        let version = "first_version";
        println!("checking second commit, version: {}", version);
        let res = verify_working_copy_with_version(
            &ctx,
            &commit_sync_data,
            Source(second_large_cs_id),
            Target(second_small_cs_id),
            &CommitSyncConfigVersion(version.to_string()),
            live_commit_sync_config.clone(),
        )
        .await;
        assert!(res.is_err());

        let version = "second_version";
        println!("checking first and second commit, version: {}", version);
        let res = verify_working_copy_with_version(
            &ctx,
            &commit_sync_data,
            Source(first_large_cs_id),
            Target(second_small_cs_id),
            &CommitSyncConfigVersion(version.to_string()),
            live_commit_sync_config.clone(),
        )
        .await;
        assert!(res.is_err());

        Ok(())
    }

    async fn init(
        fb: FacebookInit,
        direction: CommitSyncDirection,
    ) -> Result<(Syncers<TestRepo>, Arc<TestLiveCommitSyncConfig>), Error> {
        let ctx = CoreContext::test_mock(fb);

        let mut factory = TestRepoFactory::new(fb)?;

        let (lv_cfg, lv_cfg_src) = TestLiveCommitSyncConfig::new_with_source();
        let live_commit_sync_config = Arc::new(lv_cfg);

        let mapping =
            SqlSyncedCommitMappingBuilder::from_sql_connections(factory.metadata_db().clone())
                .build(RendezVousOptions::for_test());

        let small_repo: TestRepo = factory
            .with_id(RepositoryId::new(0))
            .with_live_commit_sync_config(live_commit_sync_config.clone())
            .build()
            .await?;
        Linear::init_repo(fb, &small_repo).await?;
        let large_repo: TestRepo = factory
            .with_id(RepositoryId::new(1))
            .with_live_commit_sync_config(live_commit_sync_config.clone())
            .build()
            .await?;
        Linear::init_repo(fb, &large_repo).await?;

        let master = BookmarkKey::new("master")?;

        let current_version = CommitSyncConfigVersion("noop".to_string());

        let repos = CommitSyncRepos::new(
            small_repo.clone(),
            large_repo.clone(),
            direction,
            SubmoduleDeps::ForSync(HashMap::new()),
        );

        let maybe_master_val = small_repo
            .bookmarks()
            .get(ctx.clone(), &master, bookmarks::Freshness::MostRecent)
            .await?;

        let master_val = maybe_master_val.ok_or_else(|| Error::msg("master not found"))?;
        let changesets = small_repo
            .commit_graph()
            .ancestors_difference(&ctx, vec![master_val], vec![])
            .await?;

        for cs_id in changesets {
            mapping
                .add(
                    &ctx,
                    SyncedCommitMappingEntry {
                        large_repo_id: large_repo.repo_identity().id(),
                        small_repo_id: small_repo.repo_identity().id(),
                        small_bcs_id: cs_id,
                        large_bcs_id: cs_id,
                        version_name: Some(current_version.clone()),
                        source_repo: Some(repos.get_source_repo_type()),
                    },
                )
                .await?;
        }

        let common_config = CommonCommitSyncConfig {
            common_pushrebase_bookmarks: vec![BookmarkKey::new("master")?],
            small_repos: hashmap! {
                small_repo.repo_identity().id() => SmallRepoPermanentConfig {
                    bookmark_prefix: AsciiString::from_str("prefix/").unwrap(),
                    common_pushrebase_bookmarks_map: HashMap::new(),
                }
            },
            large_repo_id: large_repo.repo_identity().id(),
        };

        let current_version_config = CommitSyncConfig {
            large_repo_id: large_repo.repo_identity().id(),
            common_pushrebase_bookmarks: vec![BookmarkKey::new("master")?],
            small_repos: hashmap! {
                small_repo.repo_identity().id() => SmallRepoCommitSyncConfig {
                    default_action: DefaultSmallToLargeCommitSyncPathAction::Preserve,
                    map: hashmap! { },
                    submodule_config: Default::default(),
                },
            },
            version_name: current_version.clone(),
        };
        let config_with_prefix = CommitSyncConfig {
            large_repo_id: large_repo.repo_identity().id(),
            common_pushrebase_bookmarks: vec![BookmarkKey::new("master")?],
            small_repos: hashmap! {
                small_repo.repo_identity().id() => SmallRepoCommitSyncConfig {
                    default_action: DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(NonRootMPath::new("prefix/")?),
                    map: hashmap! { },
                    submodule_config: Default::default(),
                },
            },
            version_name: CommitSyncConfigVersion("prefix".to_string()),
        };

        lv_cfg_src.add_common_config(common_config);
        lv_cfg_src.add_config(current_version_config);
        lv_cfg_src.add_config(config_with_prefix);

        let syncers = create_commit_syncers(
            &ctx,
            small_repo,
            large_repo,
            SubmoduleDeps::ForSync(HashMap::new()),
            live_commit_sync_config.clone(),
        )?;

        Ok((syncers, live_commit_sync_config))
    }
}
