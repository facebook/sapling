/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use blobrepo_utils::convert_diff_result_into_file_change_for_diamond_merge;
use blobsync::copy_content;
use changesets_creation::save_changesets;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriterRef;
use context::CoreContext;
use filestore::FilestoreConfigRef;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use megarepo_configs::SourceMappingRules;
use metaconfig_types::GitSubmodulesChangesAction;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use mononoke_types::TrackedFileChange;
use mononoke_types::path::MPath;
use movers::Movers;
use pushrebase::find_bonsai_diff;
use repo_blobstore::RepoBlobstoreRef;
use repo_identity::RepoIdentityRef;
use slog::Logger;
use slog::debug;
use slog::error;
use sorted_vector_map::SortedVectorMap;

use crate::git_submodules::SubmoduleExpansionData;
use crate::git_submodules::sync_commit_with_submodule_expansion;
use crate::implicit_deletes::get_renamed_implicit_deletes;
use crate::implicit_deletes::minimize_file_change_set;
// TODO(T182311609): refine imports
use crate::types::*;

const SQUASH_DELIMITER_MESSAGE: &str = r#"

============================

This commit created by squashing the following git commits:
"#;

/// Create a version of `cs` with `Mover` applied to all changes
/// The return value can be:
/// - `Err` if the rewrite failed
/// - `Ok(None)` if the rewrite decided that this commit should
///   not be present in the rewrite target
/// - `Ok(Some(rewritten))` for a successful rewrite, which should be
///   present in the rewrite target
///
/// The notion that the commit "should not be present in the rewrite
/// target" means that the commit is not a merge and all of its changes
/// were rewritten into nothingness by the `Mover`.
///
/// Precondition: this function expects all `cs` parents to be present
/// in `remapped_parents` as keys, and their remapped versions as values.
pub async fn rewrite_commit<'a, R: Repo>(
    ctx: &'a CoreContext,
    cs: BonsaiChangesetMut,
    remapped_parents: &'a HashMap<ChangesetId, ChangesetId>,
    movers: Movers,
    source_repo: &'a R,
    rewrite_opts: RewriteOpts,
    git_submodules_action: GitSubmodulesChangesAction,
    mb_submodule_expansion_data: Option<SubmoduleExpansionData<'a, R>>,
) -> Result<CommitRewriteResult> {
    // TODO(T169695293): add filter to only keep submodules for implicit deletes?
    let (file_changes_filters, cs): (Vec<FileChangeFilter<'a>>, BonsaiChangesetMut) =
        match git_submodules_action {
            GitSubmodulesChangesAction::Strip => {
                let filter_func: FileChangeFilterFunc<'a> = Arc::new(move |(_path, fc)| match fc {
                    FileChange::Change(tfc) => tfc.file_type() != FileType::GitSubmodule,
                    _ => true,
                });
                let filter: FileChangeFilter<'a> = FileChangeFilter {
                    func: filter_func,
                    application: FileChangeFilterApplication::MultiMover,
                };

                (vec![filter], cs)
            }
            // Keep submodules -> no filters and keep original bonsai
            GitSubmodulesChangesAction::Keep => (vec![], cs),
            // Expand submodules -> no filters, but modify the file change
            // file types in the bonsai
            GitSubmodulesChangesAction::Expand => {
                let submodule_expansion_data = mb_submodule_expansion_data.ok_or(
                  anyhow!("Submodule expansion data not provided when submodules is enabled for small repo")
              )?;

                return sync_commit_with_submodule_expansion(
                    ctx,
                    cs,
                    source_repo,
                    submodule_expansion_data,
                    movers.clone(),
                    remapped_parents,
                    rewrite_opts,
                )
                .await;
            }
        };

    let mb_rewritten = rewrite_commit_with_file_changes_filter(
        ctx,
        cs,
        remapped_parents,
        Arc::new(movers.mover),
        source_repo,
        None,
        rewrite_opts,
        file_changes_filters,
    )
    .await?;

    Ok(CommitRewriteResult::new(mb_rewritten, HashMap::new()))
}

// TODO(T182311609): make this pub(crate) and ensure all external callers go through
// `rewrite_commit` instead.
/// Implementation of `rewrite_commit` that can take a vector of filters to
/// apply to the commit's file changes before getting its implicit deletes.
pub async fn rewrite_commit_with_file_changes_filter<'a>(
    ctx: &'a CoreContext,
    cs: BonsaiChangesetMut,
    remapped_parents: &'a HashMap<ChangesetId, ChangesetId>,
    mover: Arc<dyn MultiMover + 'a>,
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
    mover: Arc<dyn MultiMover + 'a>,
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
            let new_paths = mover.multi_move_path(&path)?;
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

// TODO(T182311609): reduce visibility to crate
pub fn rewrite_commit_with_implicit_deletes<'a>(
    logger: &Logger,
    mut cs: BonsaiChangesetMut,
    remapped_parents: &'a HashMap<ChangesetId, ChangesetId>,
    mover: Arc<dyn MultiMover + 'a>,
    file_change_filters: Vec<FileChangeFilter<'a>>,
    force_first_parent: Option<ChangesetId>,
    renamed_implicit_deletes: Vec<Vec<NonRootMPath>>,
    rewrite_opts: RewriteOpts,
) -> Result<Option<BonsaiChangesetMut>, Error> {
    for (path, change) in cs.subtree_changes.iter() {
        if change.alters_manifest() {
            match path.clone().into_optional_non_root_path() {
                None => {
                    bail!(
                        "Subtree changes for the root are not supported in commit transformation"
                    );
                }
                Some(dst_path) => {
                    if mover.conflicts_with(&dst_path)? {
                        bail!("Subtree change for {path:?} overlaps with commit transformation");
                    }
                }
            }
        }
    }
    if !cs.subtree_changes.is_empty() || cs.hg_extra.contains_key("subtree") {
        cs.subtree_changes.clear();
        cs.hg_extra.remove("subtree");
        mark_as_created_by_lossy_conversion(logger, &mut cs, LossyConversionReason::SubtreeChanges);
    }

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
                    mover: &dyn MultiMover,
                ) -> Result<Option<(NonRootMPath, ChangesetId)>, Error> {
                    let (path, copy_from_commit) = copy_from;
                    let new_paths = mover.multi_move_path(path)?;
                    let copy_from_commit =
                        remapped_parents.get(copy_from_commit).ok_or_else(|| {
                            Error::from(ErrorKind::MissingRemappedCommit(*copy_from_commit))
                        })?;

                    // If the source path doesn't remap, drop this copy info.

                    // FIXME: a path can be remapped to multiple other paths,
                    // but for copy_from path we pick only the first one. Instead of
                    // picking only the first one, it's a better to have a dedicated
                    // field in a thrift struct which says which path should be picked
                    // as copy from
                    Ok(new_paths
                        .first()
                        .cloned()
                        .map(|new_path| (new_path, *copy_from_commit)))
                }

                // Extract any copy_from information, and use rewrite_copy_from on it
                fn rewrite_file_change(
                    change: &TrackedFileChange,
                    remapped_parents: &HashMap<ChangesetId, ChangesetId>,
                    mover: &dyn MultiMover,
                ) -> Result<FileChange, Error> {
                    let new_copy_from = change
                        .copy_from()
                        .and_then(|copy_from| {
                            rewrite_copy_from(copy_from, remapped_parents, mover).transpose()
                        })
                        .transpose()?;

                    Ok(FileChange::Change(
                        change.with_new_copy_from(new_copy_from).without_git_lfs(),
                    ))
                }

                // Rewrite both path and changes
                fn do_rewrite(
                    path: &NonRootMPath,
                    change: &FileChange,
                    remapped_parents: &HashMap<ChangesetId, ChangesetId>,
                    mover: &dyn MultiMover,
                ) -> Result<Vec<(NonRootMPath, FileChange)>, Error> {
                    let new_paths = mover.multi_move_path(path)?;
                    let change = match change {
                        FileChange::Change(tc) => rewrite_file_change(tc, remapped_parents, mover)?,
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
                do_rewrite(path, change, remapped_parents, mover.as_ref())
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

    let enable_commit_extra_stripping =
        justknobs::eval("scm/mononoke:strip_commit_extras_in_xrepo_sync", None, None)
            .unwrap_or_else(|err| {
                error!(
                    logger,
                    "Failed to read just knob scm/mononoke:strip_commit_extras_in_xrepo_sync: {err}"
                );
                false
            });

    if enable_commit_extra_stripping {
        match rewrite_opts.strip_commit_extras {
            StripCommitExtras::Hg => {
                // Set to an empty map to strip the hg extras
                cs.hg_extra = Default::default();
            }
            StripCommitExtras::Git => {
                // Set to an empty map to strip the git extras
                cs.git_extra_headers = None;
            }
            StripCommitExtras::None => {}
        };
    }

    cs.hg_extra.extend(rewrite_opts.add_hg_extras);

    let enable_should_set_committer_info_to_author_info_if_empty = justknobs::eval(
        "scm/mononoke:should_set_committer_info_to_author_info_if_empty",
        None,
        None,
    )
    .unwrap_or_else(|err| {
        error!(logger, "Failed to read just knob scm/mononoke:should_set_committer_info_to_author_info_if_empty: {err}");
        false
    });

    // Hg doesn't have a concept of committer and committer date, so commits
    // that are originally created in Hg have these fields empty when synced
    // to a git repo.
    //
    // This setting determines if, in Hg->Git sync, the committer and committer
    // date fields should be set to the author and date fields if empty.
    if enable_should_set_committer_info_to_author_info_if_empty
        && rewrite_opts.should_set_committer_info_to_author_info_if_empty
    {
        if cs.committer.is_none() {
            cs.committer = Some(cs.author.clone());
        }

        if cs.committer_date.is_none() {
            cs.committer_date = Some(cs.author_date.clone());
        }
    }

    Ok(Some(cs))
}

pub fn create_source_to_target_multi_mover(
    mapping_rules: SourceMappingRules,
) -> Result<Arc<dyn MultiMover + 'static>, Error> {
    Ok(Arc::new(MegarepoMultiMover::new(mapping_rules)?))
}

pub fn create_directory_source_to_target_multi_mover(
    mapping_rules: SourceMappingRules,
) -> Result<DirectoryMultiMover, Error> {
    // We apply the longest prefix first
    let mut overrides = mapping_rules.overrides.into_iter().collect::<Vec<_>>();
    overrides.sort_unstable_by_key(|(prefix, _)| prefix.len());
    overrides.reverse();
    let prefix = MPath::new(mapping_rules.default_prefix)?;

    Ok(Arc::new(move |path: &MPath| -> Result<Vec<MPath>, Error> {
        for (override_prefix_src, dsts) in &overrides {
            let override_prefix_src = MPath::new(override_prefix_src.clone())?;
            if override_prefix_src.is_prefix_of(path.into_iter()) {
                let suffix: Vec<_> = path
                    .into_iter()
                    .skip(override_prefix_src.num_components())
                    .collect();

                return dsts
                    .iter()
                    .map(|dst| {
                        let override_prefix = MPath::new(dst)?;
                        Ok(override_prefix.join(suffix.clone()))
                    })
                    .collect::<Result<_, _>>();
            }
        }
        Ok(vec![prefix.join(path)])
    }))
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
        LossyConversionReason::SubtreeChanges => {
            "the source changeset has subtree changes that have been removed in the target changeset"
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
    source_repo: &'a impl RepoBlobstoreRef,
    target_repo: &'a (
            impl RepoBlobstoreRef
            + CommitGraphRef
            + CommitGraphWriterRef
            + FilestoreConfigRef
            + RepoIdentityRef
        ),
    submodule_content_ids: Vec<(Arc<impl RepoBlobstoreRef>, HashSet<ContentId>)>,
) -> Result<(), Error> {
    let mut files_to_sync = HashSet::new();
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

    // Remove the content ids from submodules from the ones that will be
    // copied from source repo
    //
    // Used to dedupe duplicate content ids between submodules.
    let mut already_processed: HashSet<ContentId> = HashSet::new();

    let submodule_content_ids = submodule_content_ids
        .into_iter()
        .map(|(repo, content_ids)| {
            // Keep only ids that haven't been seen in other submodules yet
            let deduped_sm_content_ids = content_ids
                .difference(&already_processed)
                .cloned()
                .collect::<HashSet<_>>();

            deduped_sm_content_ids.iter().for_each(|content_id| {
                // Remove it from the set of ids that are actually from
                // the source repo
                files_to_sync.remove(content_id);
                // Add it to the list of content ids that were processed
                already_processed.insert(*content_id);
            });

            (repo, deduped_sm_content_ids)
        });

    // Copy submodule changes
    stream::iter(submodule_content_ids)
        .map(|(sm_repo, content_ids)| async move {
            copy_file_contents(ctx, sm_repo.as_ref(), target_repo, content_ids, |_| {}).await
        })
        .buffer_unordered(10)
        .try_collect::<()>()
        .await?;

    // Then copy from source repo
    copy_file_contents(ctx, source_repo, target_repo, files_to_sync, |_| {}).await?;
    save_changesets(ctx, target_repo, rewritten_list.clone()).await?;
    Ok(())
}

pub async fn copy_file_contents<'a>(
    ctx: &'a CoreContext,
    source_repo: &'a impl RepoBlobstoreRef,
    target_repo: &'a (impl RepoBlobstoreRef + FilestoreConfigRef),
    // Contents are uploaded concurrently, so they have to deduped to prevent
    // race conditions that bypass the `filestore::exists` check and lead to
    // `File exists (os error 17)` errors.
    content_ids: HashSet<ContentId>,
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
