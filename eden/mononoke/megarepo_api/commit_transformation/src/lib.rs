/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Error;
use anyhow::Result;
use blobrepo::save_bonsai_changesets;
use blobrepo_utils::convert_diff_result_into_file_change_for_diamond_merge;
use blobsync::copy_content;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bookmarks::BookmarksRef;
use borrowed::borrowed;
use changeset_fetcher::ChangesetFetcherArc;
use changesets::ChangesetsRef;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use derivative::Derivative;
use filestore::FilestoreConfigRef;
use futures::future::try_join_all;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use manifest::get_implicit_deletes;
use megarepo_configs::types::SourceMappingRules;
use mononoke_types::non_root_mpath_element_iter;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::FileChange;
use mononoke_types::NonRootMPath;
use mononoke_types::SkeletonManifestId;
use mononoke_types::TrackedFileChange;
use pushrebase::find_bonsai_diff;
use repo_blobstore::RepoBlobstoreArc;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use skeleton_manifest::RootSkeletonManifestId;
use slog::debug;
use slog::Logger;
use sorted_vector_map::SortedVectorMap;
use thiserror::Error;

pub type MultiMover<'a> =
    Arc<dyn Fn(&NonRootMPath) -> Result<Vec<NonRootMPath>, Error> + Send + Sync + 'a>;
pub type DirectoryMultiMover = Arc<
    dyn Fn(&Option<NonRootMPath>) -> Result<Vec<Option<NonRootMPath>>, Error>
        + Send
        + Sync
        + 'static,
>;

/// Determines when a file change filter should be applied.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FileChangeFilterApplication {
    /// Filter only before getting the implicit deletes from the bonsai
    ImplicitDeletes,
    /// Filter only before calling the multi mover
    MultiMover,
    /// Filter both before getting the implicit deletes from the bonsai and
    /// before calling the multi mover
    Both,
}

// Function that can be used to filter out irrelevant file changes from the bonsai
// before getting its implicit deletes and/or calling the multi mover.
// Getting implicit deletes requires doing manifest lookups that are O(file changes),
// so removing unnecessary changes before can significantly speed up rewrites.
// This can also be used to filter out specific kinds of file changes, e.g.
// git submodules or untracked changes.
pub type FileChangeFilterFunc<'a> =
    Arc<dyn Fn((&NonRootMPath, &FileChange)) -> bool + Send + Sync + 'a>;

/// Specifies a filter to be applied to file changes from a bonsai to remove
/// unwanted changes before certain stages of the rewrite process, e.g. before
/// getting the implicit deletes from the bonsai or before calling the multi
/// mover.
#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct FileChangeFilter<'a> {
    /// Function containing the filter logic
    #[derivative(Debug = "ignore")]
    pub func: FileChangeFilterFunc<'a>,
    /// When to apply the filter
    pub application: FileChangeFilterApplication,
}

pub trait Repo = RepoIdentityRef
    + RepoBlobstoreArc
    + ChangesetsRef
    + ChangesetFetcherArc
    + BookmarksRef
    + BonsaiHgMappingRef
    + RepoDerivedDataRef
    + RepoBlobstoreRef
    + CommitGraphRef
    + Send
    + Sync;

const SQUASH_DELIMITER_MESSAGE: &str = r#"

============================

This commit created by squashing the following git commits:
"#;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Remapped commit {0} expected in target repo, but not present")]
    MissingRemappedCommit(ChangesetId),
    #[error(
        "Can't reoder changesets parents to put {0} first because it's not a changeset's parent."
    )]
    MissingForcedParent(ChangesetId),
}

pub fn create_source_to_target_multi_mover(
    mapping_rules: SourceMappingRules,
) -> Result<MultiMover<'static>, Error> {
    // We apply the longest prefix first
    let mut overrides = mapping_rules.overrides.into_iter().collect::<Vec<_>>();
    overrides.sort_unstable_by_key(|(ref prefix, _)| prefix.len());
    overrides.reverse();
    let prefix = NonRootMPath::new_opt(mapping_rules.default_prefix)?;

    Ok(Arc::new(
        move |path: &NonRootMPath| -> Result<Vec<NonRootMPath>, Error> {
            for (override_prefix_src, dsts) in &overrides {
                let override_prefix_src = NonRootMPath::new(override_prefix_src.clone())?;
                if override_prefix_src.is_prefix_of(path) {
                    let suffix: Vec<_> = path
                        .into_iter()
                        .skip(override_prefix_src.num_components())
                        .collect();

                    return dsts
                        .iter()
                        .map(|dst| {
                            let override_prefix = NonRootMPath::new_opt(dst)?;
                            NonRootMPath::join_opt(override_prefix.as_ref(), suffix.clone())
                                .ok_or_else(|| anyhow!("unexpected empty path"))
                        })
                        .collect::<Result<_, _>>();
                }
            }

            Ok(vec![
                NonRootMPath::join_opt(prefix.as_ref(), path)
                    .ok_or_else(|| anyhow!("unexpected empty path"))?,
            ])
        },
    ))
}

pub fn create_directory_source_to_target_multi_mover(
    mapping_rules: SourceMappingRules,
) -> Result<DirectoryMultiMover, Error> {
    // We apply the longest prefix first
    let mut overrides = mapping_rules.overrides.into_iter().collect::<Vec<_>>();
    overrides.sort_unstable_by_key(|(ref prefix, _)| prefix.len());
    overrides.reverse();
    let prefix = NonRootMPath::new_opt(mapping_rules.default_prefix)?;

    Ok(Arc::new(
        move |path: &Option<NonRootMPath>| -> Result<Vec<Option<NonRootMPath>>, Error> {
            for (override_prefix_src, dsts) in &overrides {
                let override_prefix_src = NonRootMPath::new(override_prefix_src.clone())?;
                if override_prefix_src.is_prefix_of(non_root_mpath_element_iter(path)) {
                    let suffix: Vec<_> = non_root_mpath_element_iter(path)
                        .skip(override_prefix_src.num_components())
                        .collect();

                    return dsts
                        .iter()
                        .map(|dst| {
                            let override_prefix = NonRootMPath::new_opt(dst)?;
                            Ok(NonRootMPath::join_opt(
                                override_prefix.as_ref(),
                                suffix.clone(),
                            ))
                        })
                        .collect::<Result<_, _>>();
                }
            }

            Ok(vec![NonRootMPath::join_opt(
                prefix.as_ref(),
                non_root_mpath_element_iter(path),
            )])
        },
    ))
}

/// Get `SkeletonManifestId`s for a set of `ChangesetId`s
/// This is needed for the purposes of implicit delete detection
async fn get_skeleton_manifest_ids<'a, I: IntoIterator<Item = ChangesetId>>(
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
    bcs_ids: I,
) -> Result<Vec<SkeletonManifestId>, Error> {
    try_join_all(bcs_ids.into_iter().map({
        |bcs_id| async move {
            let repo_derived_data = repo.repo_derived_data();

            let root_skeleton_manifest_id = repo_derived_data
                .derive::<RootSkeletonManifestId>(ctx, bcs_id)
                .await?;

            Ok(root_skeleton_manifest_id.into_skeleton_manifest_id())
        }
    }))
    .await
}

/// Take an iterator of file changes, which may contain implicit deletes
/// and produce a `SortedVectorMap` suitable to be used in the `BonsaiChangeset`,
/// without any implicit deletes.
fn minimize_file_change_set<I: IntoIterator<Item = (NonRootMPath, FileChange)>>(
    file_changes: I,
) -> SortedVectorMap<NonRootMPath, FileChange> {
    let (adds, removes): (Vec<_>, Vec<_>) = file_changes
        .into_iter()
        .partition(|(_, fc)| fc.is_changed());
    let adds: HashMap<NonRootMPath, FileChange> = adds.into_iter().collect();

    let prefix_path_was_added = |removed_path: NonRootMPath| {
        removed_path
            .into_parent_dir_iter()
            .any(|parent_dir| adds.contains_key(&parent_dir))
    };

    let filtered_removes = removes
        .into_iter()
        .filter(|(ref mpath, _)| !prefix_path_was_added(mpath.clone()));
    let mut result: SortedVectorMap<_, _> = filtered_removes.collect();
    result.extend(adds);
    result
}

/// Given a changeset and it's parents, get the list of file
/// changes, which arise from "implicit deletes" as opposed
/// to naive `NonRootMPath` rewriting in `cs.file_changes`. For
/// more information about implicit deletes, please see
/// `manifest/src/implici_deletes.rs`
pub async fn get_renamed_implicit_deletes<'a, I: IntoIterator<Item = ChangesetId>>(
    ctx: &'a CoreContext,
    file_changes: Vec<(&NonRootMPath, &FileChange)>,
    parent_changeset_ids: I,
    mover: MultiMover<'a>,
    source_repo: &'a impl Repo,
) -> Result<Vec<Vec<NonRootMPath>>, Error> {
    let parent_manifest_ids =
        get_skeleton_manifest_ids(ctx, source_repo, parent_changeset_ids).await?;

    // Get all the paths that were added or modified and thus are capable of
    // implicitly deleting existing directories.
    let paths_added: Vec<_> = file_changes
        .into_iter()
        .filter(|&(_mpath, file_change)| file_change.is_changed())
        .map(|(mpath, _file_change)| mpath.clone())
        .collect();

    let store = source_repo.repo_blobstore().clone();
    let implicit_deletes: Vec<NonRootMPath> =
        get_implicit_deletes(ctx, store, paths_added, parent_manifest_ids)
            .try_collect()
            .await?;
    implicit_deletes.iter().map(|mpath| mover(mpath)).collect()
}

/// Determines what to do in commits rewriting to empty commit in small repo.
///
/// NOTE: The empty commits from large repo are kept regardless of this flag.
#[derive(PartialEq, Debug, Copy, Clone, Default)]
pub enum CommitRewrittenToEmpty {
    Keep,
    #[default]
    Discard,
}

/// Determines what to do with commits that are empty in large repo.  They may
/// be useful to keep them in small repo if they have some special meaning.
///
/// NOTE: This flag doesn't affect non-empty commits from large repo rewriting
/// to empty commits in small repo. Use CommitsRewrittenToEmpty to control that.
#[derive(PartialEq, Debug, Copy, Clone, Default)]
pub enum EmptyCommitFromLargeRepo {
    #[default]
    Keep,
    Discard,
}

#[derive(PartialEq, Debug, Copy, Clone, Default)]
pub struct RewriteOpts {
    pub commit_rewritten_to_empty: CommitRewrittenToEmpty,
    pub empty_commit_from_large_repo: EmptyCommitFromLargeRepo,
}

/// Create a version of `cs` with `Mover` applied to all changes
/// The return value can be:
/// - `Err` if the rewrite failed
/// - `Ok(None)` if the rewrite decided that this commit should
///              not be present in the rewrite target
/// - `Ok(Some(rewritten))` for a successful rewrite, which should be
///                         present in the rewrite target
/// The notion that the commit "should not be present in the rewrite
/// target" means that the commit is not a merge and all of its changes
/// were rewritten into nothingness by the `Mover`.
///
/// Precondition: this function expects all `cs` parents to be present
/// in `remapped_parents` as keys, and their remapped versions as values.
///
/// If `force_first_parent` is set commit parents are reordered to ensure that
/// the specified changeset comes first.
pub async fn rewrite_commit<'a>(
    ctx: &'a CoreContext,
    cs: BonsaiChangesetMut,
    remapped_parents: &'a HashMap<ChangesetId, ChangesetId>,
    mover: MultiMover<'a>,
    source_repo: &'a impl Repo,
    force_first_parent: Option<ChangesetId>,
    rewrite_opts: RewriteOpts,
) -> Result<Option<BonsaiChangesetMut>, Error> {
    rewrite_commit_with_file_changes_filter(
        ctx,
        cs,
        remapped_parents,
        mover,
        source_repo,
        force_first_parent,
        rewrite_opts,
        vec![], // No file change filters by default
    )
    .await
}

/// Implementation of `rewrite_commit` that can take a vector of filters to
/// apply to the commit's file changes before getting its implicit deletes.
pub async fn rewrite_commit_with_file_changes_filter<'a>(
    ctx: &'a CoreContext,
    cs: BonsaiChangesetMut,
    remapped_parents: &'a HashMap<ChangesetId, ChangesetId>,
    mover: MultiMover<'a>,
    source_repo: &'a impl Repo,
    force_first_parent: Option<ChangesetId>,
    rewrite_opts: RewriteOpts,
    file_change_filters: Vec<FileChangeFilter<'a>>,
) -> Result<Option<BonsaiChangesetMut>, Error> {
    // All file change filters that should be applied before getting implicit
    // deletes.
    let implicit_deletes_filters = file_change_filters
        .iter()
        .filter_map(|filter| match filter.application {
            FileChangeFilterApplication::ImplicitDeletes | FileChangeFilterApplication::Both => {
                Some(filter.func.clone())
            }
            FileChangeFilterApplication::MultiMover => None,
        })
        .collect::<Vec<_>>();

    let filtered_file_changes: Vec<(&NonRootMPath, &FileChange)> = cs
        .file_changes
        .iter()
        // Keep file changes that pass all the filters
        .filter(|fc| {
            implicit_deletes_filters
                .iter()
                .all(|filter_func| filter_func(*fc))
        })
        .collect();

    let renamed_implicit_deletes = if !filtered_file_changes.is_empty() {
        get_renamed_implicit_deletes(
            ctx,
            filtered_file_changes,
            remapped_parents.keys().cloned(),
            mover.clone(),
            source_repo,
        )
        .await?
    } else {
        vec![]
    };

    rewrite_commit_with_implicit_deletes(
        ctx.logger(),
        cs,
        remapped_parents,
        mover,
        file_change_filters,
        force_first_parent,
        renamed_implicit_deletes,
        rewrite_opts,
    )
}

pub async fn rewrite_as_squashed_commit<'a>(
    ctx: &'a CoreContext,
    source_repo: &'a impl Repo,
    source_cs_id: ChangesetId,
    (source_parent_cs_id, target_parent_cs_id): (ChangesetId, ChangesetId),
    mut cs: BonsaiChangesetMut,
    mover: MultiMover<'a>,
    side_commits_info: Vec<String>,
) -> Result<Option<BonsaiChangesetMut>, Error> {
    let diff_stream = find_bonsai_diff(ctx, source_repo, source_parent_cs_id, source_cs_id).await?;
    let diff_changes: Vec<_> = diff_stream
        .map_ok(|diff_result| async move {
            convert_diff_result_into_file_change_for_diamond_merge(ctx, source_repo, diff_result)
                .await
        })
        .try_buffered(100)
        .try_collect()
        .await?;

    let rewritten_changes = diff_changes
        .into_iter()
        .map(|(path, change)| {
            let new_paths = mover(&path)?;
            Ok(new_paths
                .into_iter()
                .map(|new_path| (new_path, change.clone()))
                .collect())
        })
        .collect::<Result<Vec<Vec<_>>, Error>>()?;

    let rewritten_changes: SortedVectorMap<_, _> = rewritten_changes
        .into_iter()
        .flat_map(|changes| changes.into_iter())
        .collect();

    cs.file_changes = rewritten_changes;
    // `validate_can_sync_changeset` already ensures
    // that target_parent_cs_id is one of the existing parents
    cs.parents = vec![target_parent_cs_id];
    let old_message = cs.message;
    cs.message = format!(
        "{}{}{}",
        old_message,
        SQUASH_DELIMITER_MESSAGE,
        side_commits_info.join("\n")
    );
    Ok(Some(cs))
}

pub async fn rewrite_stack_no_merges<'a>(
    ctx: &'a CoreContext,
    css: Vec<BonsaiChangeset>,
    mut rewritten_parent: ChangesetId,
    mover: MultiMover<'a>,
    source_repo: &'a impl Repo,
    force_first_parent: Option<ChangesetId>,
    mut modify_bonsai_cs: impl FnMut((ChangesetId, BonsaiChangesetMut)) -> BonsaiChangesetMut,
) -> Result<Vec<Option<BonsaiChangeset>>, Error> {
    borrowed!(mover: &Arc<_>, source_repo);

    for cs in &css {
        if cs.is_merge() {
            return Err(anyhow!(
                "cannot remap merges in a stack - {}",
                cs.get_changeset_id()
            ));
        }
    }

    let css = stream::iter(css)
        .map({
            |cs| async move {
                let deleted_file_changes = if cs.file_changes().next().is_some() {
                    let parents = cs.parents();
                    get_renamed_implicit_deletes(
                        ctx,
                        cs.file_changes().collect(),
                        parents,
                        mover.clone(),
                        source_repo,
                    )
                    .await?
                } else {
                    vec![]
                };

                anyhow::Ok((cs, deleted_file_changes))
            }
        })
        .buffered(100)
        .try_collect::<Vec<_>>()
        .await?;

    let mut res = vec![];
    for (from_cs, renamed_implicit_deletes) in css {
        let from_cs_id = from_cs.get_changeset_id();
        let from_cs = from_cs.into_mut();

        let mut remapped_parents = HashMap::new();
        if let Some(parent) = from_cs.parents.get(0) {
            remapped_parents.insert(*parent, rewritten_parent);
        }

        let maybe_cs = rewrite_commit_with_implicit_deletes(
            ctx.logger(),
            from_cs,
            &remapped_parents,
            mover.clone(),
            vec![],
            force_first_parent,
            renamed_implicit_deletes,
            Default::default(),
        )?;

        let maybe_cs = maybe_cs
            .map(|cs| modify_bonsai_cs((from_cs_id, cs)))
            .map(|bcs| bcs.freeze())
            .transpose()?;
        if let Some(ref cs) = maybe_cs {
            let to_cs_id = cs.get_changeset_id();
            rewritten_parent = to_cs_id;
        }

        res.push(maybe_cs);
    }

    Ok(res)
}

pub fn rewrite_commit_with_implicit_deletes<'a>(
    logger: &Logger,
    mut cs: BonsaiChangesetMut,
    remapped_parents: &'a HashMap<ChangesetId, ChangesetId>,
    mover: MultiMover,
    file_change_filters: Vec<FileChangeFilter<'a>>,
    force_first_parent: Option<ChangesetId>,
    renamed_implicit_deletes: Vec<Vec<NonRootMPath>>,
    rewrite_opts: RewriteOpts,
) -> Result<Option<BonsaiChangesetMut>, Error> {
    let empty_commit = cs.file_changes.is_empty();
    if !empty_commit
        || rewrite_opts.empty_commit_from_large_repo == EmptyCommitFromLargeRepo::Discard
    {
        // All file change filters that should be applied before calling the
        // multi mover.
        let multi_mover_filters = file_change_filters
            .iter()
            .filter_map(|filter| match filter.application {
                FileChangeFilterApplication::MultiMover | FileChangeFilterApplication::Both => {
                    Some(filter.func.clone())
                }
                FileChangeFilterApplication::ImplicitDeletes => None,
            })
            .collect::<Vec<_>>();
        let path_rewritten_changes = cs
            .file_changes
            .iter()
            .filter(|fc| {
                multi_mover_filters
                    .iter()
                    .all(|filter_func| filter_func(*fc))
            })
            .map(|(path, change)| {
                // Just rewrite copy_from information, when we have it
                fn rewrite_copy_from(
                    copy_from: &(NonRootMPath, ChangesetId),
                    remapped_parents: &HashMap<ChangesetId, ChangesetId>,
                    mover: MultiMover,
                ) -> Result<Option<(NonRootMPath, ChangesetId)>, Error> {
                    let (path, copy_from_commit) = copy_from;
                    let new_paths = mover(path)?;
                    let copy_from_commit =
                        remapped_parents.get(copy_from_commit).ok_or_else(|| {
                            Error::from(ErrorKind::MissingRemappedCommit(*copy_from_commit))
                        })?;

                    // If the source path doesn't remap, drop this copy info.

                    // TODO(stash): a path can be remapped to multiple other paths,
                    // but for copy_from path we pick only the first one. Instead of
                    // picking only the first one, it's a better to have a dedicated
                    // field in a thrift struct which says which path should be picked
                    // as copy from
                    Ok(new_paths
                        .get(0)
                        .cloned()
                        .map(|new_path| (new_path, *copy_from_commit)))
                }

                // Extract any copy_from information, and use rewrite_copy_from on it
                fn rewrite_file_change(
                    change: &TrackedFileChange,
                    remapped_parents: &HashMap<ChangesetId, ChangesetId>,
                    mover: MultiMover,
                ) -> Result<FileChange, Error> {
                    let new_copy_from = change
                        .copy_from()
                        .and_then(|copy_from| {
                            rewrite_copy_from(copy_from, remapped_parents, mover).transpose()
                        })
                        .transpose()?;

                    Ok(FileChange::Change(change.with_new_copy_from(new_copy_from)))
                }

                // Rewrite both path and changes
                fn do_rewrite(
                    path: &NonRootMPath,
                    change: &FileChange,
                    remapped_parents: &HashMap<ChangesetId, ChangesetId>,
                    mover: MultiMover,
                ) -> Result<Vec<(NonRootMPath, FileChange)>, Error> {
                    let new_paths = mover(path)?;
                    let change = match change {
                        FileChange::Change(tc) => {
                            rewrite_file_change(tc, remapped_parents, mover.clone())?
                        }
                        FileChange::Deletion => FileChange::Deletion,
                        FileChange::UntrackedDeletion | FileChange::UntrackedChange(_) => {
                            bail!("Can't rewrite untracked changes")
                        }
                    };
                    Ok(new_paths
                        .into_iter()
                        .map(|new_path| (new_path, change.clone()))
                        .collect())
                }
                do_rewrite(path, change, remapped_parents, mover.clone())
            })
            .collect::<Result<Vec<Vec<_>>, _>>()?;

        // If any file change in the source bonsai changeset has no equivalent file change in the destination
        // changeset, the conversion was lossy
        if path_rewritten_changes
            .iter()
            .any(|rewritten| rewritten.is_empty())
        {
            mark_as_created_by_lossy_conversion(
                logger,
                &mut cs,
                LossyConversionReason::FileChanges,
            );
        // If any implicit delete in the source bonsai changeset has no equivalent file change in the destination
        // changeset, the conversion was lossy
        } else if renamed_implicit_deletes
            .iter()
            .any(|rewritten| rewritten.is_empty())
        {
            mark_as_created_by_lossy_conversion(
                logger,
                &mut cs,
                LossyConversionReason::ImplicitFileChanges,
            );
        }

        let mut path_rewritten_changes: SortedVectorMap<_, _> = path_rewritten_changes
            .into_iter()
            .flat_map(|changes| changes.into_iter())
            .collect();

        // Add the implicit deletes as explicit delete changes.
        let implicit_delete_file_changes: Vec<(NonRootMPath, FileChange)> =
            renamed_implicit_deletes
                .into_iter()
                .flatten()
                .map(|implicit_delete_mpath| (implicit_delete_mpath, FileChange::Deletion))
                .collect();
        path_rewritten_changes.extend(implicit_delete_file_changes);

        // Then minimize the file changes by removing the deletes that don't
        // need to be explicit because they'll still be expressed implicitly
        // after the rewrite.
        let path_rewritten_changes = minimize_file_change_set(path_rewritten_changes);

        let is_merge = cs.parents.len() >= 2;

        // If all parent has < 2 commits then it's not a merge, and it was completely rewritten
        // out. In that case we can just discard it because there are not changes to the working copy.
        // However if it's a merge then we can't discard it, because even
        // though bonsai merge commit might not have file changes inside it can still change
        // a working copy. E.g. if p1 has fileA, p2 has fileB, then empty merge(p1, p2)
        // contains both fileA and fileB.
        if !is_merge
            && ((path_rewritten_changes.is_empty()
                && rewrite_opts.commit_rewritten_to_empty == CommitRewrittenToEmpty::Discard)
                || (empty_commit
                    && rewrite_opts.empty_commit_from_large_repo
                        == EmptyCommitFromLargeRepo::Discard))
        {
            return Ok(None);
        } else {
            cs.file_changes = path_rewritten_changes;
        }
    }

    // Update hashes
    for commit in cs.parents.iter_mut() {
        let remapped = remapped_parents
            .get(commit)
            .ok_or_else(|| Error::from(ErrorKind::MissingRemappedCommit(*commit)))?;

        *commit = *remapped;
    }
    if let Some(first_parent) = force_first_parent {
        if !cs.parents.contains(&first_parent) {
            return Err(Error::from(ErrorKind::MissingForcedParent(first_parent)));
        }
        let mut new_parents = vec![first_parent];
        new_parents.extend(cs.parents.into_iter().filter(|cs| *cs != first_parent));
        cs.parents = new_parents
    }

    Ok(Some(cs))
}

enum LossyConversionReason {
    FileChanges,
    ImplicitFileChanges,
}

fn mark_as_created_by_lossy_conversion(
    logger: &Logger,
    cs: &mut BonsaiChangesetMut,
    reason: LossyConversionReason,
) {
    let reason = match reason {
        LossyConversionReason::FileChanges => {
            "the file changes from the source changeset don't all have an equivalent file change in the target changeset"
        }
        LossyConversionReason::ImplicitFileChanges => {
            "implicit file changes from the source changeset don't all have an equivalent implicit file change in the target changeset"
        }
    };
    debug!(
        logger,
        "Marking changeset as created by lossy conversion because {}", reason
    );
    cs.hg_extra
        .insert("created_by_lossy_conversion".to_string(), Vec::new());
}

pub async fn upload_commits<'a>(
    ctx: &'a CoreContext,
    rewritten_list: Vec<BonsaiChangeset>,
    source_repo: &'a (impl RepoBlobstoreRef + ChangesetsRef),
    target_repo: &'a (impl RepoBlobstoreRef + ChangesetsRef + FilestoreConfigRef),
) -> Result<(), Error> {
    let mut files_to_sync = vec![];
    for rewritten in &rewritten_list {
        let rewritten_mut = rewritten.clone().into_mut();
        let new_files_to_sync =
            rewritten_mut
                .file_changes
                .values()
                .filter_map(|change| match change {
                    FileChange::Change(tc) => Some(tc.content_id()),
                    FileChange::UntrackedChange(uc) => Some(uc.content_id()),
                    FileChange::Deletion | FileChange::UntrackedDeletion => None,
                });
        files_to_sync.extend(new_files_to_sync);
    }
    copy_file_contents(ctx, source_repo, target_repo, files_to_sync, |_| {}).await?;
    save_bonsai_changesets(rewritten_list.clone(), ctx.clone(), target_repo).await?;
    Ok(())
}

pub async fn copy_file_contents<'a>(
    ctx: &'a CoreContext,
    source_repo: &'a impl RepoBlobstoreRef,
    target_repo: &'a (impl RepoBlobstoreRef + FilestoreConfigRef),
    content_ids: impl IntoIterator<Item = ContentId>,
    progress_reporter: impl Fn(usize),
) -> Result<(), Error> {
    let source_blobstore = source_repo.repo_blobstore();
    let target_blobstore = target_repo.repo_blobstore();
    let target_filestore_config = target_repo.filestore_config();

    let mut i = 0;
    stream::iter(content_ids.into_iter().map({
        |content_id| {
            copy_content(
                ctx,
                source_blobstore,
                target_blobstore,
                target_filestore_config.clone(),
                content_id,
            )
        }
    }))
    .buffer_unordered(100)
    .try_for_each(|_| {
        i += 1;
        progress_reporter(i);
        async { Ok(()) }
    })
    .await
}

#[cfg(test)]
mod test {
    use std::collections::BTreeMap;
    use std::collections::HashSet;

    use anyhow::bail;
    use blobrepo::save_bonsai_changesets;
    use blobstore::Loadable;
    use fbinit::FacebookInit;
    use maplit::btreemap;
    use maplit::hashmap;
    use maplit::hashset;
    use mononoke_types::ContentId;
    use mononoke_types::FileType;
    use test_repo_factory::TestRepoFactory;
    use tests_utils::list_working_copy_utf8;
    use tests_utils::CreateCommitContext;

    use super::*;

    #[test]
    fn test_multi_mover_simple() -> Result<(), Error> {
        let mapping_rules = SourceMappingRules {
            default_prefix: "".to_string(),
            ..Default::default()
        };
        let multi_mover = create_source_to_target_multi_mover(mapping_rules)?;
        assert_eq!(
            multi_mover(&NonRootMPath::new("path")?)?,
            vec![NonRootMPath::new("path")?]
        );
        Ok(())
    }

    #[test]
    fn test_multi_mover_prefixed() -> Result<(), Error> {
        let mapping_rules = SourceMappingRules {
            default_prefix: "prefix".to_string(),
            ..Default::default()
        };
        let multi_mover = create_source_to_target_multi_mover(mapping_rules)?;
        assert_eq!(
            multi_mover(&NonRootMPath::new("path")?)?,
            vec![NonRootMPath::new("prefix/path")?]
        );
        Ok(())
    }

    #[test]
    fn test_multi_mover_prefixed_with_exceptions() -> Result<(), Error> {
        let mapping_rules = SourceMappingRules {
            default_prefix: "prefix".to_string(),
            overrides: btreemap! {
                "override".to_string() => vec![
                    "overriden_1".to_string(),
                    "overriden_2".to_string(),
                ]
            },
            ..Default::default()
        };
        let multi_mover = create_source_to_target_multi_mover(mapping_rules)?;
        assert_eq!(
            multi_mover(&NonRootMPath::new("path")?)?,
            vec![NonRootMPath::new("prefix/path")?]
        );

        assert_eq!(
            multi_mover(&NonRootMPath::new("override/path")?)?,
            vec![
                NonRootMPath::new("overriden_1/path")?,
                NonRootMPath::new("overriden_2/path")?,
            ]
        );
        Ok(())
    }

    #[test]
    fn test_multi_mover_longest_prefix_first() -> Result<(), Error> {
        let mapping_rules = SourceMappingRules {
            default_prefix: "prefix".to_string(),
            overrides: btreemap! {
                "prefix".to_string() => vec![
                    "prefix_1".to_string(),
                ],
                "prefix/sub".to_string() => vec![
                    "prefix/sub_1".to_string(),
                ]
            },
            ..Default::default()
        };
        let multi_mover = create_source_to_target_multi_mover(mapping_rules)?;
        assert_eq!(
            multi_mover(&NonRootMPath::new("prefix/path")?)?,
            vec![NonRootMPath::new("prefix_1/path")?]
        );

        assert_eq!(
            multi_mover(&NonRootMPath::new("prefix/sub/path")?)?,
            vec![NonRootMPath::new("prefix/sub_1/path")?]
        );

        Ok(())
    }

    fn path(p: &str) -> NonRootMPath {
        NonRootMPath::new(p).unwrap()
    }

    fn verify_minimized(changes: Vec<(&str, Option<()>)>, expected: BTreeMap<&str, Option<()>>) {
        fn to_file_change(o: Option<()>) -> FileChange {
            match o {
                Some(_) => FileChange::tracked(
                    ContentId::from_bytes([1; 32]).unwrap(),
                    FileType::Regular,
                    0,
                    None,
                ),
                None => FileChange::Deletion,
            }
        }
        let changes: Vec<_> = changes
            .into_iter()
            .map(|(p, c)| (path(p), to_file_change(c)))
            .collect();
        let minimized = minimize_file_change_set(changes);
        let expected: SortedVectorMap<NonRootMPath, FileChange> = expected
            .into_iter()
            .map(|(p, c)| (path(p), to_file_change(c)))
            .collect();
        assert_eq!(expected, minimized);
    }

    #[fbinit::test]
    fn test_minimize_file_change_set(_fb: FacebookInit) {
        verify_minimized(
            vec![("a", Some(())), ("a", None)],
            btreemap! { "a" => Some(())},
        );
        verify_minimized(vec![("a", Some(()))], btreemap! { "a" => Some(())});
        verify_minimized(vec![("a", None)], btreemap! { "a" => None});
        // directories are deleted implicitly, so explicit deletes are
        // minimized away
        verify_minimized(
            vec![("a/b", None), ("a/c", None), ("a", Some(()))],
            btreemap! { "a" => Some(()) },
        );
        // files, replaced with a directy at a longer path are not
        // deleted implicitly, so they aren't minimized away
        verify_minimized(
            vec![("a", None), ("a/b", Some(()))],
            btreemap! { "a" => None, "a/b" => Some(()) },
        );
    }

    #[fbinit::test]
    async fn test_rewrite_commit_marks_lossy_conversions(fb: FacebookInit) -> Result<(), Error> {
        let repo: blobrepo::BlobRepo = TestRepoFactory::new(fb)?.build().await?;
        let ctx = CoreContext::test_mock(fb);
        let mapping_rules = SourceMappingRules {
            default_prefix: "".to_string(), // Rewrite to root
            overrides: btreemap! {
                "www".to_string() => vec!["".to_string()], // map changes to www to root
                "xplat".to_string() => vec![], // swallow changes outside of www
            },
            ..Default::default()
        };
        let multi_mover = create_source_to_target_multi_mover(mapping_rules)?;
        // Add files to www and xplat (lossy)
        let first = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("www/foo.php", "foo content")
            .add_file("www/bar/baz.php", "baz content")
            .add_file("www/bar/crux.php", "crux content")
            .add_file("xplat/a/a.js", "a content")
            .add_file("xplat/a/b.js", "b content")
            .add_file("xplat/b/c.js", "c content")
            .commit()
            .await?;
        // Only add one file in xplat (No changeset will be generated)
        let second = CreateCommitContext::new(&ctx, &repo, vec![first])
            .add_file("xplat/c/d.js", "d content")
            .commit()
            .await?;
        // Only add one file in www (non-lossy)
        let third = CreateCommitContext::new(&ctx, &repo, vec![second])
            .add_file("www/baz/foobar.php", "foobar content")
            .commit()
            .await?;
        // Only change files in www (non-lossy)
        let fourth = CreateCommitContext::new(&ctx, &repo, vec![third])
            .add_file("www/baz/foobar.php", "more foobar content")
            .add_file("www/foo.php", "more foo content")
            .commit()
            .await?;
        // Only delete files in www (non-lossy)
        let fifth = CreateCommitContext::new(&ctx, &repo, vec![fourth])
            .delete_file("www/baz/crux.php")
            .commit()
            .await?;
        // Delete files in www and xplat (lossy)
        let sixth = CreateCommitContext::new(&ctx, &repo, vec![fifth])
            .delete_file("xplat/a/a.js")
            .delete_file("www/bar/baz.php")
            .commit()
            .await?;

        let first_rewritten_bcs_id = test_rewrite_commit_cs_id(
            &ctx,
            &repo,
            first,
            HashMap::new(),
            multi_mover.clone(),
            None,
        )
        .await?;
        assert_changeset_is_marked_lossy(&ctx, &repo, first_rewritten_bcs_id).await?;

        assert!(
            test_rewrite_commit_cs_id(
                &ctx,
                &repo,
                second,
                hashmap! {
                    first => first_rewritten_bcs_id,
                },
                multi_mover.clone(),
                None,
            )
            .await
            .is_err()
        );

        let third_rewritten_bcs_id = test_rewrite_commit_cs_id(
            &ctx,
            &repo,
            third,
            hashmap! {
                second => first_rewritten_bcs_id, // there is no second equivalent
            },
            multi_mover.clone(),
            None,
        )
        .await?;
        assert_changeset_is_not_marked_lossy(&ctx, &repo, third_rewritten_bcs_id).await?;

        let fourth_rewritten_bcs_id = test_rewrite_commit_cs_id(
            &ctx,
            &repo,
            fourth,
            hashmap! {
                third => third_rewritten_bcs_id,
            },
            multi_mover.clone(),
            None,
        )
        .await?;
        assert_changeset_is_not_marked_lossy(&ctx, &repo, fourth_rewritten_bcs_id).await?;

        let fifth_rewritten_bcs_id = test_rewrite_commit_cs_id(
            &ctx,
            &repo,
            fifth,
            hashmap! {
                fourth => fourth_rewritten_bcs_id,
            },
            multi_mover.clone(),
            None,
        )
        .await?;
        assert_changeset_is_not_marked_lossy(&ctx, &repo, fifth_rewritten_bcs_id).await?;

        let sixth_rewritten_bcs_id = test_rewrite_commit_cs_id(
            &ctx,
            &repo,
            sixth,
            hashmap! {
                fifth => fifth_rewritten_bcs_id,
            },
            multi_mover.clone(),
            None,
        )
        .await?;
        assert_changeset_is_marked_lossy(&ctx, &repo, sixth_rewritten_bcs_id).await?;

        Ok(())
    }

    #[fbinit::test]
    async fn test_rewrite_commit_marks_lossy_conversions_with_implicit_deletes(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let repo: blobrepo::BlobRepo = TestRepoFactory::new(fb)?.build().await?;
        let ctx = CoreContext::test_mock(fb);
        // This commit is not lossy because all paths will be mapped somewhere.
        let first = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("a/b/c/d", "d")
            .add_file("a/b/c/e", "e")
            .add_file("a/b/c/f/g", "g")
            .add_file("a/b/c/f/h", "h")
            .add_file("a/b/c/i", "i")
            .commit()
            .await?;
        let second = CreateCommitContext::new(&ctx, &repo, vec![first])
            .add_file("a/b/c", "new c") // This creates a file at `a/b/c`, implicitely deleting the directory
            // at `a/b/c` and all the files it contains (`d`, `e`, `f/g` and `f/h`)
            .add_file("a/b/i", "new i")
            .commit()
            .await?;

        // With the full mapping rules, all directories from the repo have a mapping
        let full_mapping_rules = SourceMappingRules {
            default_prefix: "".to_string(),
            overrides: btreemap! {
                "a/b".to_string() => vec!["ab".to_string()],
                "a/b/c".to_string() => vec!["abc".to_string()],
                "a/b/c/f".to_string() => vec!["abcf".to_string()],
            },
            ..Default::default()
        };
        let full_multi_mover = create_source_to_target_multi_mover(full_mapping_rules)?;
        // With the partial mapping rules, files under `a/b/c/f` don't have a mapping
        let partial_mapping_rules = SourceMappingRules {
            default_prefix: "".to_string(),
            overrides: btreemap! {
                "a/b".to_string() => vec!["ab".to_string()],
                "a/b/c".to_string() => vec!["abc".to_string()],
                "a/b/c/f".to_string() => vec![],
            },
            ..Default::default()
        };
        let partial_multi_mover = create_source_to_target_multi_mover(partial_mapping_rules)?;

        // We rewrite the first commit with the full_multi_mover.
        // This is not lossy.
        let first_rewritten_bcs_id = test_rewrite_commit_cs_id(
            &ctx,
            &repo,
            first,
            HashMap::new(),
            full_multi_mover.clone(),
            None,
        )
        .await?;
        assert_changeset_is_not_marked_lossy(&ctx, &repo, first_rewritten_bcs_id).await?;
        // When we rewrite the second commit with the full_multi_mover.
        // This is not lossy:
        // All file changes have a mapping and all implicitely deleted files have a mapping.
        let full_second_rewritten_bcs_id = test_rewrite_commit_cs_id(
            &ctx,
            &repo,
            second,
            hashmap! {
                first => first_rewritten_bcs_id
            },
            full_multi_mover.clone(),
            None,
        )
        .await?;
        assert_changeset_is_not_marked_lossy(&ctx, &repo, full_second_rewritten_bcs_id).await?;
        // When we rewrite the second commit with the partial_multi_mover.
        // This **is** lossy:
        // All file changes have a mapping but some implicitely deleted files don't have a mapping
        // (namely, `a/b/c/f/g` and `a/b/c/f/h`).
        let partial_second_rewritten_bcs_id = test_rewrite_commit_cs_id(
            &ctx,
            &repo,
            second,
            hashmap! {
                first => first_rewritten_bcs_id
            },
            partial_multi_mover.clone(),
            None,
        )
        .await?;
        assert_changeset_is_marked_lossy(&ctx, &repo, partial_second_rewritten_bcs_id).await?;
        Ok(())
    }

    #[fbinit::test]
    async fn test_rewrite_commit(fb: FacebookInit) -> Result<(), Error> {
        let repo: blobrepo::BlobRepo = TestRepoFactory::new(fb)?.build().await?;
        let ctx = CoreContext::test_mock(fb);
        let first = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("path", "path")
            .commit()
            .await?;
        let second = CreateCommitContext::new(&ctx, &repo, vec![first])
            .add_file_with_copy_info("pathsecondcommit", "pathsecondcommit", (first, "path"))
            .commit()
            .await?;
        let third = CreateCommitContext::new(&ctx, &repo, vec![first, second])
            .add_file("path", "pathmodified")
            .commit()
            .await?;

        let mapping_rules = SourceMappingRules {
            default_prefix: "prefix".to_string(),
            overrides: btreemap! {
                "path".to_string() => vec![
                    "path_1".to_string(),
                    "path_2".to_string(),
                ]
            },
            ..Default::default()
        };
        let multi_mover = create_source_to_target_multi_mover(mapping_rules)?;

        let first_rewritten_bcs_id = test_rewrite_commit_cs_id(
            &ctx,
            &repo,
            first,
            HashMap::new(),
            multi_mover.clone(),
            None,
        )
        .await?;

        let first_rewritten_wc =
            list_working_copy_utf8(&ctx, &repo, first_rewritten_bcs_id).await?;
        assert_eq!(
            first_rewritten_wc,
            hashmap! {
                NonRootMPath::new("path_1")? => "path".to_string(),
                NonRootMPath::new("path_2")? => "path".to_string(),
            }
        );

        let second_rewritten_bcs_id = test_rewrite_commit_cs_id(
            &ctx,
            &repo,
            second,
            hashmap! {
                first => first_rewritten_bcs_id
            },
            multi_mover.clone(),
            None,
        )
        .await?;

        let second_bcs = second_rewritten_bcs_id
            .load(&ctx, &repo.repo_blobstore())
            .await?;
        let maybe_copy_from = match second_bcs
            .file_changes_map()
            .get(&NonRootMPath::new("prefix/pathsecondcommit")?)
            .ok_or_else(|| anyhow!("path not found"))?
        {
            FileChange::Change(tc) => tc.copy_from().cloned(),
            _ => bail!("path_is_deleted"),
        };

        assert_eq!(
            maybe_copy_from,
            Some((NonRootMPath::new("path_1")?, first_rewritten_bcs_id))
        );

        let second_rewritten_wc =
            list_working_copy_utf8(&ctx, &repo, second_rewritten_bcs_id).await?;
        assert_eq!(
            second_rewritten_wc,
            hashmap! {
                NonRootMPath::new("path_1")? => "path".to_string(),
                NonRootMPath::new("path_2")? => "path".to_string(),
                NonRootMPath::new("prefix/pathsecondcommit")? => "pathsecondcommit".to_string(),
            }
        );

        // Diamond merge test with error during parent reordering
        assert!(
            test_rewrite_commit_cs_id(
                &ctx,
                &repo,
                third,
                hashmap! {
                    first => first_rewritten_bcs_id,
                    second => second_rewritten_bcs_id
                },
                multi_mover.clone(),
                Some(second), // wrong, should be after-rewrite id
            )
            .await
            .is_err()
        );

        // Diamond merge test with success
        let third_rewritten_bcs_id = test_rewrite_commit_cs_id(
            &ctx,
            &repo,
            third,
            hashmap! {
                first => first_rewritten_bcs_id,
                second => second_rewritten_bcs_id
            },
            multi_mover,
            Some(second_rewritten_bcs_id),
        )
        .await?;

        let third_bcs = third_rewritten_bcs_id
            .load(&ctx, &repo.repo_blobstore().clone())
            .await?;

        assert_eq!(
            third_bcs.parents().collect::<Vec<_>>(),
            vec![second_rewritten_bcs_id, first_rewritten_bcs_id],
        );

        Ok(())
    }

    /**
     * Set up a small repo to test multiple scenarios with file change filters.
     *
     * The first commit sets the following structure:
     * foo
     *  └── bar
     *      ├── a
     *      ├── b
     *      │   ├── d
     *      │   └── e
     *      └── c
     *          ├── f
     *          └── g
     *
     * The second commit adds two files `foo/bar/b` (executable) and `foo/bar/c`
     * which implicitly deletes some files under `foo/bar`.
     */
    async fn test_rewrite_commit_with_file_changes_filter(
        fb: FacebookInit,
        file_change_filters: Vec<FileChangeFilter<'_>>,
        mut expected_affected_paths: HashMap<&str, HashSet<NonRootMPath>>,
    ) -> Result<(), Error> {
        let repo: blobrepo::BlobRepo = TestRepoFactory::new(fb)?.build().await?;

        let ctx = CoreContext::test_mock(fb);
        // This commit is not lossy because all paths will be mapped somewhere.
        let first = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("foo/bar/a", "a")
            .add_file("foo/bar/b/d", "d")
            .add_file("foo/bar/b/e", "e")
            .add_file("foo/bar/c/f", "f")
            .add_file("foo/bar/c/g", "g")
            .commit()
            .await?;

        // Create files at `foo/bar/b` and `foo/bar/c`, implicitely deleting all
        // files under those directories.
        let second = CreateCommitContext::new(&ctx, &repo, vec![first])
            // Implicitly deletes `foo/bar/b/d` and `foo/bar/b/e`.
            // Adding it as an executable so we can test filters that apply on
            // conditions other than paths.
            .add_file_with_type("foo/bar/b", "new b", FileType::Executable)
            // Implicitly deletes `foo/bar/c/f` and `foo/bar/c/g`.
            .add_file("foo/bar/c", "new c")
            .commit()
            .await?;

        let identity_multi_mover = Arc::new(
            move |path: &NonRootMPath| -> Result<Vec<NonRootMPath>, Error> {
                Ok(vec![path.clone()])
            },
        );

        async fn verify_affected_paths(
            ctx: &CoreContext,
            repo: &blobrepo::BlobRepo,
            rewritten_bcs_id: &ChangesetId,
            expected_affected_paths: HashSet<NonRootMPath>,
        ) -> Result<()> {
            let bcs = rewritten_bcs_id.load(ctx, repo.repo_blobstore()).await?;

            let affected_paths = bcs
                .file_changes()
                .map(|(p, _fc)| p.clone())
                .collect::<HashSet<_>>();

            assert_eq!(expected_affected_paths, affected_paths);
            Ok(())
        }

        let first_rewritten_bcs_id = test_rewrite_commit_cs_id_with_file_change_filters(
            &ctx,
            &repo,
            first,
            HashMap::new(),
            identity_multi_mover.clone(),
            None,
            file_change_filters.clone(),
        )
        .await?;

        verify_affected_paths(
            &ctx,
            &repo,
            &first_rewritten_bcs_id,
            expected_affected_paths.remove("first").unwrap(),
        )
        .await?;

        let second_rewritten_bcs_id = test_rewrite_commit_cs_id_with_file_change_filters(
            &ctx,
            &repo,
            second,
            hashmap! {
                first => first_rewritten_bcs_id
            },
            identity_multi_mover.clone(),
            None,
            file_change_filters,
        )
        .await?;

        verify_affected_paths(
            &ctx,
            &repo,
            &second_rewritten_bcs_id,
            expected_affected_paths.remove("second").unwrap(),
        )
        .await?;

        Ok(())
    }

    /// Tests applying a file change filter before getting the implicit deletes
    /// and calling the multi mover.
    #[fbinit::test]
    async fn test_rewrite_commit_with_file_changes_filter_on_both_based_on_path(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let file_change_filter_func: FileChangeFilterFunc<'_> =
            Arc::new(|(source_path, _): (&NonRootMPath, &FileChange)| -> bool {
                let ignored_path_prefix: NonRootMPath = NonRootMPath::new("foo/bar/b").unwrap();
                !ignored_path_prefix.is_prefix_of(source_path)
            });

        let file_change_filters: Vec<FileChangeFilter<'_>> = vec![FileChangeFilter {
            func: file_change_filter_func,
            application: FileChangeFilterApplication::Both,
        }];

        let expected_affected_paths: HashMap<&str, HashSet<NonRootMPath>> = hashmap! {
            // Changes to `foo/bar/b/d` and `foo/bar/b/e` are removed in the
            // final bonsai because the filter ran before the multi-mover.
            "first" => hashset! {
                NonRootMPath::new("foo/bar/a").unwrap(),
                NonRootMPath::new("foo/bar/c/f").unwrap(),
                NonRootMPath::new("foo/bar/c/g").unwrap()
            },
            // We expect only the added file to be affected. The delete of
            // `foo/bar/c/g` and `foo/bar/c/f` will remain implicit because
            // the change to `foo/bar/c` is present in the bonsai.
            "second" => hashset! {
                NonRootMPath::new("foo/bar/c").unwrap()
            },
        };

        test_rewrite_commit_with_file_changes_filter(
            fb,
            file_change_filters,
            expected_affected_paths,
        )
        .await?;

        Ok(())
    }

    /// Tests applying a file change filter before getting the implicit deletes
    /// and calling the multi mover.
    #[fbinit::test]
    async fn test_rewrite_commit_with_file_changes_filter_on_both_based_on_file_type(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let file_change_filter_func: FileChangeFilterFunc<'_> =
            Arc::new(|(_, fc): (&NonRootMPath, &FileChange)| -> bool {
                match fc {
                    FileChange::Change(tfc) => tfc.file_type() != FileType::Executable,
                    _ => true,
                }
            });

        let file_change_filters: Vec<FileChangeFilter<'_>> = vec![FileChangeFilter {
            func: file_change_filter_func,
            application: FileChangeFilterApplication::Both,
        }];

        let expected_affected_paths: HashMap<&str, HashSet<NonRootMPath>> = hashmap! {
             // All changes are synced because there are no executable files.
             "first" => hashset! {
                NonRootMPath::new("foo/bar/a").unwrap(),
                NonRootMPath::new("foo/bar/c/f").unwrap(),
                NonRootMPath::new("foo/bar/c/g").unwrap(),
                NonRootMPath::new("foo/bar/b/e").unwrap(),
                NonRootMPath::new("foo/bar/b/d").unwrap(),
            },
            // We expect only the added file to be affected. The delete of
            // `foo/bar/c/g` and `foo/bar/c/f` will remain implicit because
            // the change to `foo/bar/c` is present in the bonsai.
            // The files under `foo/bar/b` will not be implicitly or explicitly
            // deleted because the addition of the executable file was ignored
            // when getting the implicit deletes and rewriting the changes.
            "second" => hashset! {
                NonRootMPath::new("foo/bar/c").unwrap()
            },
        };

        test_rewrite_commit_with_file_changes_filter(
            fb,
            file_change_filters,
            expected_affected_paths,
        )
        .await?;

        Ok(())
    }

    /// Tests applying a file change filter only before getting the
    /// implicit deletes.
    #[fbinit::test]
    async fn test_rewrite_commit_with_file_changes_filter_implicit_deletes_only(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let file_change_filter_func: FileChangeFilterFunc<'_> =
            Arc::new(|(source_path, _): (&NonRootMPath, &FileChange)| -> bool {
                let ignored_path_prefix: NonRootMPath = NonRootMPath::new("foo/bar/b").unwrap();
                !ignored_path_prefix.is_prefix_of(source_path)
            });

        let file_change_filters: Vec<FileChangeFilter<'_>> = vec![FileChangeFilter {
            func: file_change_filter_func,
            application: FileChangeFilterApplication::ImplicitDeletes,
        }];
        // Applying the filter only before the implicit deletes should increase
        // performance because it won't do unnecessary work, but it should NOT
        // affect which file changes are synced.
        // That's because even if implicit deletes are found, because no filter
        // is applied before the multi-mover, they will still be expressed
        // implicitly in the final bonsai.
        let expected_affected_paths: HashMap<&str, HashSet<NonRootMPath>> = hashmap! {
            // Since the filter for `foo/bar/b` is applied only before getting
            // the implicit deletes, all changes will be synced.
            "first" => hashset! {
                NonRootMPath::new("foo/bar/a").unwrap(),
                NonRootMPath::new("foo/bar/b/d").unwrap(),
                NonRootMPath::new("foo/bar/b/e").unwrap(),
                NonRootMPath::new("foo/bar/c/f").unwrap(),
                NonRootMPath::new("foo/bar/c/g").unwrap()
            },
            // The same applies to the second commit. The same paths are synced.
            "second" => hashset! {
                NonRootMPath::new("foo/bar/c").unwrap(),
                // The path file added that implicitly deletes the two above
                NonRootMPath::new("foo/bar/b").unwrap(),
                // `foo/bar/b/d` and `foo/bar/b/e` will not be present in the
                // bonsai, because they're being deleted implicitly.
                //
                // WHY: the filter is applied only when getting the implicit deletes.
                // So `foo/bar/b` is synced via the multi mover, which means that
                // the delete is already expressed implicitly, so `minimize_file_change_set`
                // will remove the unnecessary explicit deletes.
            },
        };

        test_rewrite_commit_with_file_changes_filter(
            fb,
            file_change_filters,
            expected_affected_paths,
        )
        .await?;

        Ok(())
    }

    /// Tests applying a file change filter only before calling the
    /// multi mover.
    /// This test uses the file type as the filter condition, to showcase
    /// a more realistic scenario where we only want to apply the filter to
    /// the multi mover.
    #[fbinit::test]
    async fn test_rewrite_commit_with_file_changes_filter_multi_mover_only(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let file_change_filter_func: FileChangeFilterFunc<'_> =
            Arc::new(|(_, fc): (&NonRootMPath, &FileChange)| -> bool {
                match fc {
                    FileChange::Change(tfc) => tfc.file_type() != FileType::Executable,
                    _ => true,
                }
            });
        let file_change_filters: Vec<FileChangeFilter<'_>> = vec![FileChangeFilter {
            func: file_change_filter_func,
            application: FileChangeFilterApplication::MultiMover,
        }];

        let expected_affected_paths: HashMap<&str, HashSet<NonRootMPath>> = hashmap! {
            // All changes are synced because there are no executable files.
            "first" => hashset! {
                NonRootMPath::new("foo/bar/a").unwrap(),
                NonRootMPath::new("foo/bar/c/f").unwrap(),
                NonRootMPath::new("foo/bar/c/g").unwrap(),
                NonRootMPath::new("foo/bar/b/e").unwrap(),
                NonRootMPath::new("foo/bar/b/d").unwrap(),
            },
            "second" => hashset! {
                NonRootMPath::new("foo/bar/c").unwrap(),
                // `foo/bar/b` implicitly deletes these two files below in the
                // source bonsai. However, because the change to `foo/bar/b`
                // will not be synced (is't an executable file), these implicit
                // deletes will be added explicitly to the rewritten bonsai.
                NonRootMPath::new("foo/bar/b/e").unwrap(),
                NonRootMPath::new("foo/bar/b/d").unwrap(),
            },
        };

        test_rewrite_commit_with_file_changes_filter(
            fb,
            file_change_filters,
            expected_affected_paths,
        )
        .await?;

        Ok(())
    }

    async fn test_rewrite_commit_cs_id<'a>(
        ctx: &'a CoreContext,
        repo: &'a impl Repo,
        bcs_id: ChangesetId,
        parents: HashMap<ChangesetId, ChangesetId>,
        multi_mover: MultiMover<'a>,
        force_first_parent: Option<ChangesetId>,
    ) -> Result<ChangesetId, Error> {
        test_rewrite_commit_cs_id_with_file_change_filters(
            ctx,
            repo,
            bcs_id,
            parents,
            multi_mover,
            force_first_parent,
            vec![],
        )
        .await
    }

    async fn test_rewrite_commit_cs_id_with_file_change_filters<'a>(
        ctx: &'a CoreContext,
        repo: &'a impl Repo,
        bcs_id: ChangesetId,
        parents: HashMap<ChangesetId, ChangesetId>,
        multi_mover: MultiMover<'a>,
        force_first_parent: Option<ChangesetId>,
        file_change_filters: Vec<FileChangeFilter<'a>>,
    ) -> Result<ChangesetId, Error> {
        let bcs = bcs_id.load(ctx, &repo.repo_blobstore()).await?;
        let bcs = bcs.into_mut();

        let maybe_rewritten = rewrite_commit_with_file_changes_filter(
            ctx,
            bcs,
            &parents,
            multi_mover,
            repo,
            force_first_parent,
            Default::default(),
            file_change_filters,
        )
        .await?;
        let rewritten =
            maybe_rewritten.ok_or_else(|| anyhow!("can't rewrite commit {}", bcs_id))?;
        let rewritten = rewritten.freeze()?;

        save_bonsai_changesets(vec![rewritten.clone()], ctx.clone(), repo).await?;

        Ok(rewritten.get_changeset_id())
    }

    async fn assert_changeset_is_marked_lossy<'a>(
        ctx: &'a CoreContext,
        repo: &'a impl Repo,
        bcs_id: ChangesetId,
    ) -> Result<(), Error> {
        let bcs = bcs_id.load(ctx, &repo.repo_blobstore()).await?;
        assert!(
            bcs.hg_extra()
                .any(|(key, _)| key == "created_by_lossy_conversion"),
            "Failed with {:?}",
            bcs
        );
        Ok(())
    }

    async fn assert_changeset_is_not_marked_lossy<'a>(
        ctx: &'a CoreContext,
        repo: &'a impl Repo,
        bcs_id: ChangesetId,
    ) -> Result<(), Error> {
        let bcs = bcs_id.load(ctx, &repo.repo_blobstore()).await?;
        assert!(
            !bcs.hg_extra()
                .any(|(key, _)| key == "created_by_lossy_conversion"),
            "Failed with {:?}",
            bcs
        );
        Ok(())
    }

    #[test]
    fn test_directory_multi_mover() -> Result<(), Error> {
        let mapping_rules = SourceMappingRules {
            default_prefix: "prefix".to_string(),
            ..Default::default()
        };
        let multi_mover = create_directory_source_to_target_multi_mover(mapping_rules)?;
        assert_eq!(
            multi_mover(&Some(NonRootMPath::new("path")?))?,
            vec![Some(NonRootMPath::new("prefix/path")?)]
        );

        assert_eq!(
            multi_mover(&None)?,
            vec![Some(NonRootMPath::new("prefix")?)]
        );
        Ok(())
    }
}
