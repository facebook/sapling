/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;

use anyhow::Result;
use chrono::Utc;
use commit_graph::CommitGraphRef;
use filestore::FilestoreConfigRef;
use futures::StreamExt;
use futures::TryStreamExt;
use futures_stats::TimedFutureExt;
use manifest::PathTree;
use maplit::btreemap;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use mononoke_types::file_change::GitLfs;
use mononoke_types::path::MPath;
use phases::PhasesRef;
use repo_blobstore::RepoBlobstoreRef;
use scuba_ext::FutureStatsScubaExt;
use sorted_vector_map::SortedVectorMap;

use crate::ChangesetContext;
use crate::ChangesetId;
use crate::ChangesetSpecifier;
use crate::CreateChange;
use crate::CreateChangeFile;
use crate::CreateChangeFileContents;
use crate::CreateChangeGitLfs;
use crate::CreateChangesetCheckMode;
use crate::CreateChangesetChecks;
use crate::CreateCopyInfo;
use crate::CreateInfo;
use crate::MononokeError;
use crate::MononokeRepo;
use crate::RepoContext;
use crate::file::FileId;
use crate::path::MononokePathPrefixes;
use crate::repo::create_changeset::CreateChangeType;
use crate::repo::create_changeset::CreatedChangeset;
use crate::repo::create_changeset::is_prefix_changed;
use crate::repo::create_changeset::lookup_file_types_from_parents;
use crate::repo::create_changeset::verify_deleted_files_existed_in_a_parent;
use crate::repo::create_changeset::verify_no_noop_file_changes;
use crate::repo::create_changeset::verify_prefix_files_deleted;

/// Check if a CreateChange represents an absent or deleted file.
fn is_absent_or_deleted(cc: Option<&CreateChange>) -> bool {
    matches!(
        cc,
        None | Some(CreateChange::Deletion | CreateChange::UntrackedDeletion)
    )
}

/// Compare CreateChange for folding purposes.
///
/// Two changes are equal if they have the same content_id and file_type.
/// A file_type of None in `current` means "inherit from base", so it's treated as equal
/// to whatever the base file_type is.
///
/// For folding, "file doesn't exist" (None) is equivalent to "file is deleted" (Deletion)
/// since the net effect is the same - the file is absent.
///
/// git_lfs is ignored because it cannot change independently of content_id.
fn create_change_content_equals(
    base: Option<&CreateChange>,
    current: Option<&CreateChange>,
) -> bool {
    // Both absent or deleted = equal
    if is_absent_or_deleted(base) && is_absent_or_deleted(current) {
        return true;
    }

    // Compare file contents
    match (base, current) {
        (
            Some(CreateChange::Tracked(fb, _) | CreateChange::Untracked(fb)),
            Some(CreateChange::Tracked(fc, _) | CreateChange::Untracked(fc)),
        ) => {
            if fb.content_id() != fc.content_id() {
                return false;
            }
            // file_type=None means "inherit from base"
            match (&fc.file_type, &fb.file_type) {
                (None, _) => true,
                (Some(ct), Some(bt)) => ct == bt,
                (Some(_), None) => false,
            }
        }
        _ => false,
    }
}

/// Convert GitLfs from mononoke_types to CreateChangeGitLfs.
fn convert_git_lfs(lfs: &GitLfs) -> CreateChangeGitLfs {
    match lfs {
        GitLfs::FullContent => CreateChangeGitLfs::FullContent,
        GitLfs::GitLfsPointer {
            non_canonical_pointer,
        } => CreateChangeGitLfs::GitLfsPointer {
            non_canonical_pointer: non_canonical_pointer.map(|id| {
                CreateChangeFileContents::Existing {
                    file_id: id,
                    maybe_size: None,
                }
            }),
        },
    }
}

impl From<&FileChange> for CreateChange {
    fn from(fc: &FileChange) -> Self {
        match fc {
            FileChange::Change(tracked) => CreateChange::Tracked(
                CreateChangeFile {
                    contents: CreateChangeFileContents::Existing {
                        file_id: tracked.content_id(),
                        maybe_size: Some(tracked.size()),
                    },
                    file_type: Some(tracked.file_type()),
                    git_lfs: Some(convert_git_lfs(&tracked.git_lfs())),
                },
                tracked
                    .copy_from()
                    .map(|(path, _)| CreateCopyInfo::new(MPath::from(path.clone()), 0)),
            ),
            FileChange::Deletion => CreateChange::Deletion,
            FileChange::UntrackedDeletion => CreateChange::UntrackedDeletion,
            FileChange::UntrackedChange(basic) => CreateChange::Untracked(CreateChangeFile {
                contents: CreateChangeFileContents::Existing {
                    file_id: basic.content_id(),
                    maybe_size: Some(basic.size()),
                },
                file_type: Some(basic.file_type()),
                git_lfs: Some(convert_git_lfs(&basic.git_lfs())),
            }),
        }
    }
}

/// Build initial tree entries by looking up paths in base context.
///
/// Uses batched manifest lookup for efficiency. Returns entries for paths
/// that exist as files in the base context.
async fn build_initial_tree_entries<R: MononokeRepo>(
    paths: impl IntoIterator<Item = NonRootMPath>,
    base_ctx: &ChangesetContext<R>,
) -> Result<Vec<(MPath, CreateChange)>, MononokeError> {
    let paths_to_check: Vec<MPath> = paths.into_iter().map(MPath::from).collect();

    if paths_to_check.is_empty() {
        return Ok(Vec::new());
    }

    let path_contexts = base_ctx
        .paths_with_content(paths_to_check.into_iter())
        .await?;

    futures::pin_mut!(path_contexts);
    let mut entries = Vec::new();

    while let Some(path_ctx) = path_contexts.try_next().await? {
        if let Some(file_type) = path_ctx.file_type().await?
            && let Some(file_ctx) = path_ctx.file().await?
        {
            let metadata = file_ctx.metadata().await?;
            let create_change = CreateChange::Tracked(
                CreateChangeFile {
                    contents: CreateChangeFileContents::Existing {
                        file_id: metadata.content_id,
                        maybe_size: Some(metadata.total_size),
                    },
                    file_type: Some(file_type),
                    git_lfs: Some(CreateChangeGitLfs::FullContent),
                },
                None, // No copy info for base files
            );
            entries.push((path_ctx.path().clone(), create_change));
        }
    }

    Ok(entries)
}

impl<R: MononokeRepo> RepoContext<R> {
    /// Fold (squash) a range of commits into a single new commit.
    pub async fn fold_commits(
        &self,
        bottom_id: ChangesetId,
        top_id: Option<ChangesetId>,
        additional_changes: Option<BTreeMap<MPath, CreateChange>>,
        info: Option<CreateInfo>,
        checks: CreateChangesetChecks,
    ) -> Result<CreatedChangeset<R>, MononokeError> {
        if top_id.is_none() && additional_changes.is_none() {
            return Err(MononokeError::InvalidRequest(
                "one of top_id or additional_changes is required".to_string(),
            ));
        }

        let public = self
            .repo()
            .phases()
            .get_public(self.ctx(), vec![bottom_id], false)
            .await?;

        if !public.is_empty() {
            return Err(MononokeError::InvalidRequest(format!(
                "Cannot fold public commits: {}",
                bottom_id
            )));
        }
        let top_id = top_id.unwrap_or(bottom_id);
        if !self
            .repo()
            .commit_graph()
            .is_linear_stack(self.ctx(), bottom_id, top_id)
            .await?
        {
            return Err(MononokeError::InvalidRequest(format!(
                "stack is not linear bottom: {bottom_id}, top: {top_id}"
            )));
        }

        let bottom_ctx = self
            .changeset(ChangesetSpecifier::Bonsai(bottom_id))
            .await?
            .ok_or_else(|| {
                MononokeError::InvalidRequest(format!("bottom commit {bottom_id} not found"))
            })?;

        let top_ctx = if top_id == bottom_id {
            bottom_ctx.clone()
        } else {
            self.changeset(ChangesetSpecifier::Bonsai(top_id))
                .await?
                .ok_or_else(|| {
                    MononokeError::InvalidRequest(format!("top commit {top_id} not found"))
                })?
        };

        let bottom_parents = bottom_ctx.parents().await?;
        // Not handling folding on top of merge commit
        if bottom_parents.len() != 1 {
            return Err(MononokeError::InvalidRequest(format!(
                "bottom commit {bottom_id} has a merged parent"
            )));
        }

        let cs_ids = self
            .repo()
            .commit_graph()
            .range_stream(self.ctx(), bottom_id, top_id)
            .await?
            .collect::<Vec<_>>()
            .await;

        let changes = self
            .merge_stack(cs_ids, bottom_parents[0], additional_changes, &checks)
            .await?;

        // 5. Create commit info - use provided info or generate default from target commit
        let commit_info = Self::build_commit_info(info, bottom_id, &top_ctx).await?;

        // 6. Create the new folded commit with the provided checks
        let created_changeset = self
            .create_changeset(
                vec![bottom_parents[0]], // Parent is bottom's parent
                commit_info,             // Commit metadata
                changes,                 // All the file changes (already CreateChange)
                None,                    // No bubble
                checks,
            )
            .await?;

        Ok(created_changeset)
    }

    async fn build_commit_info(
        info: Option<CreateInfo>,
        _bottom_id: ChangesetId,
        top_ctx: &ChangesetContext<R>,
    ) -> Result<CreateInfo> {
        match info {
            Some(info) => Ok(info),
            None => {
                let target_commit_info = top_ctx.changeset_info().await?;

                Ok(CreateInfo {
                    author: target_commit_info.author().to_string(),
                    author_date: Utc::now().into(),
                    committer: None,
                    committer_date: None,
                    message: target_commit_info.message().to_string(),
                    extra: btreemap! {},
                    git_extra_headers: None,
                })
            }
        }
    }

    /// Merge a stack of changesets into a single changeset.
    ///
    /// This method uses PathTree to properly handle implicit deletions and avoid
    /// false copy detection:
    /// 1. Implicit deletions: When a directory becomes a file (e.g., dir1 -> dir1),
    ///    all files under that directory (e.g., dir1/a.txt) are implicitly deleted.
    ///    PathTree's insert_and_prune handles this automatically.
    /// 2. False copy detection: Only uses copy_from information that is explicitly
    ///    provided in file changes, not inferred from content matching.
    /// 3. Copy chain resolution: When folding A→B→C where file moves multiple times,
    ///    resolves the copy source to the original file that exists in the base commit.
    pub async fn merge_stack(
        &self,
        stack: Vec<ChangesetId>,
        base: ChangesetId,
        additional_changes: Option<BTreeMap<MPath, CreateChange>>,
        checks: &CreateChangesetChecks,
    ) -> Result<BTreeMap<MPath, CreateChange>> {
        if stack.is_empty() {
            return Ok(BTreeMap::new());
        }

        // Get base_ctx, which is the immediate parent of the commits to be
        // folded, and will be the parent of the new commit.
        let base_ctx = self
            .changeset(ChangesetSpecifier::Bonsai(base))
            .await?
            .ok_or_else(|| {
                MononokeError::InvalidRequest(format!("base commit {base} not found"))
            })?;

        // Use PathTree to track file states (None = doesn't exist, Some(CreateChange) = exists)
        // PathTree's hierarchical structure automatically handles implicit deletions
        let mut working_tree: PathTree<Option<CreateChange>> = PathTree::default();

        // Track copy chains: maps current path -> ultimate source path
        // When a file is copied from another file that was itself copied, we follow
        // the chain to find the original source that should exist in the base commit.
        let mut copy_chain: HashMap<NonRootMPath, NonRootMPath> = HashMap::new();

        // Track copy sources that should NOT be deleted (they are copied FROM, not renamed)
        // When a file is copied (not renamed), the source still exists and should not
        // be marked as deleted in the folded result.
        let mut copy_sources: BTreeSet<NonRootMPath> = BTreeSet::new();

        // Track all paths we've seen (including deletions)
        let mut all_paths: BTreeSet<NonRootMPath> = BTreeSet::new();

        // Track paths where files replaced directories (insert_and_prune was called).
        // These paths may have had files underneath them in the base that need explicit
        // deletions if the replacing file is later deleted.
        let mut replaced_directory_paths: BTreeSet<NonRootMPath> = BTreeSet::new();

        // Build stack_changes incrementally as we process each commit
        // This mirrors the pattern in create_changeset.rs - uses insert_and_prune for
        // changes (handles implicit deletes) and regular insert for deletions
        let mut stack_changes: PathTree<CreateChangeType> = PathTree::default();

        // Apply each changeset in topological order
        for cs_id in stack {
            let curr_ctx = self
                .changeset(ChangesetSpecifier::Bonsai(cs_id))
                .await?
                .ok_or_else(|| {
                    MononokeError::InvalidRequest(format!("changeset {cs_id} not found"))
                })?;

            let file_changes = curr_ctx.file_changes().await?;

            for (path, change) in &file_changes {
                let mpath = MPath::from(path.clone());
                match change {
                    FileChange::Change(_) => {
                        stack_changes.insert_and_prune(mpath, CreateChangeType::Change);
                    }
                    FileChange::Deletion => {
                        stack_changes.insert(mpath, CreateChangeType::Deletion);
                    }
                    _ => {}
                }
            }

            Self::apply_file_changes(
                file_changes,
                &mut all_paths,
                &mut working_tree,
                &mut copy_chain,
                &mut copy_sources,
                &mut replaced_directory_paths,
            );
        }

        // Build stack_file_types from working_tree BEFORE applying additional_changes.
        // This captures the file types from the folded stack, so we can resolve
        // file_type=None in additional_changes and merged output later.
        let stack_file_types: BTreeMap<NonRootMPath, FileType> = working_tree
            .clone()
            .into_iter()
            .filter_map(|(mpath, state)| {
                mpath
                    .into_optional_non_root_path()
                    .and_then(|non_root| match state {
                        Some(CreateChange::Tracked(f, _) | CreateChange::Untracked(f)) => {
                            f.file_type.map(|ft| (non_root, ft))
                        }
                        _ => None,
                    })
            })
            .collect();

        // Now resolve additional_changes
        if let Some(mut create_changes) = additional_changes {
            let filestore_config = *self.repo().filestore_config();
            let repo_blobstore = self.repo().repo_blobstore().clone();

            // Resolve each change (uploads file content, validates copy_from)
            for change in create_changes.values_mut() {
                change
                    .resolve::<R>(
                        self.ctx(),
                        filestore_config,
                        repo_blobstore.clone(),
                        Some(&stack_changes),
                        std::slice::from_ref(&base_ctx),
                    )
                    .await?;
            }

            // Build path_changes for the additional changes
            let path_changes: PathTree<CreateChangeType> = PathTree::from_iter(
                create_changes
                    .iter()
                    .map(|(path, change)| (path.clone(), change.change_type())),
            );

            // Extract deleted files (tracked deletions only)
            let deleted_files: BTreeSet<MPath> = create_changes
                .iter()
                .filter(|(_, change)| matches!(change, CreateChange::Deletion))
                .map(|(path, _)| path.clone())
                .collect();

            // Determine prefix paths (directories that might need file deletion checks)
            let prefix_paths: BTreeSet<MPath> = create_changes
                .iter()
                .filter(|(_, change)| change.change_type() == CreateChangeType::Change)
                .flat_map(|(path, _)| MononokePathPrefixes::new(path))
                .collect();

            // Verify deleted files existed (in stack_changes or base commit)
            let _deletions_to_remove = verify_deleted_files_existed_in_a_parent(
                std::slice::from_ref(&base_ctx),
                Some(&stack_changes),
                deleted_files,
                checks.deleted_files_existed_in_a_parent,
            )
            .timed()
            .await
            .log_future_stats(
                self.ctx().scuba().clone(),
                "fold_commits: verify deleted files existed",
                None,
            )?;

            // Verify prefix files are deleted when creating files inside a path that is a file
            verify_prefix_files_deleted(
                std::slice::from_ref(&base_ctx),
                Some(&stack_changes),
                prefix_paths,
                &path_changes,
            )
            .timed()
            .await
            .log_future_stats(
                self.ctx().scuba().clone(),
                "fold_commits: verify prefix files deleted",
                None,
            )?;

            // Filter deletions that have a change at a path prefix (implicit from the change)
            let create_changes: BTreeMap<MPath, CreateChange> = create_changes
                .into_iter()
                .filter(|(path, change)| {
                    change.change_type() != CreateChangeType::Deletion
                        || !is_prefix_changed(path, &path_changes)
                })
                .collect();

            // Convert paths to NonRootMPath for noop check
            let file_changes_for_noop_check: SortedVectorMap<NonRootMPath, CreateChange> =
                create_changes
                    .iter()
                    .filter_map(|(path, change)| {
                        path.clone()
                            .into_optional_non_root_path()
                            .map(|mpath| (mpath, change.clone()))
                    })
                    .collect();

            // Build stack content changes for noop check (content_ids from the folded stack)
            // We need to track what content exists at each path after the stack is processed
            let stack_content_changes: PathTree<Option<FileId>> = {
                let mut tree: PathTree<Option<FileId>> = PathTree::default();
                // Add content from working_tree (the accumulated state of the folded stack)
                for (mpath, state) in working_tree.clone().into_iter() {
                    match state {
                        Some(CreateChange::Tracked(file, _))
                        | Some(CreateChange::Untracked(file)) => {
                            tree.insert_and_prune(mpath, file.content_id());
                        }
                        Some(CreateChange::Deletion) | Some(CreateChange::UntrackedDeletion) => {
                            tree.insert(mpath, None);
                        }
                        None => {
                            tree.insert(mpath, None);
                        }
                    }
                }
                tree
            };

            // Verify no noop file changes (if check mode is not Skip)
            if checks.noop_file_changes != CreateChangesetCheckMode::Skip {
                verify_no_noop_file_changes(
                    std::slice::from_ref(&base_ctx),
                    Some(stack_content_changes),
                    &file_changes_for_noop_check,
                )
                .timed()
                .await
                .log_future_stats(
                    self.ctx().scuba().clone(),
                    "fold_commits: verify no noop file changes",
                    None,
                )?;
            }

            // Apply additional changes to working tree
            // Note: file_type can remain None - create_changeset will resolve it later
            Self::apply_create_changes(
                &create_changes,
                &mut all_paths,
                &mut working_tree,
                &mut copy_chain,
                &mut copy_sources,
                &mut replaced_directory_paths,
            );
        }

        let mut initial_tree: PathTree<Option<CreateChange>> = PathTree::default();

        // Remember which paths we check in this first pass
        let paths_to_check_first_pass: BTreeSet<NonRootMPath> = all_paths.clone();

        // Check each touched path in base commit using batched lookup
        for (mpath, create_change) in
            build_initial_tree_entries(paths_to_check_first_pass.iter().cloned(), &base_ctx).await?
        {
            initial_tree.insert(mpath, Some(create_change));
        }

        // For paths where we did insert_and_prune (directory->file replacement),
        // check if those paths are now deleted and had files underneath in base.
        // Those files need explicit deletions.
        for replaced_path in &replaced_directory_paths {
            // Check if this path is currently deleted or never existed as a file in working_tree
            let current_state = working_tree.get(&MPath::from(replaced_path.clone()));
            let is_deleted_or_absent = match current_state {
                Some(Some(CreateChange::Tracked(_, _) | CreateChange::Untracked(_))) => false, // File exists
                _ => true, // Deleted, UntrackedDeletion, None, or not in tree
            };

            if is_deleted_or_absent {
                // This path was used for insert_and_prune but is now gone.
                // Check if it exists as a directory in base and find files under it.
                let replaced_path_with_content =
                    base_ctx.path_with_content(replaced_path.clone()).await?;

                // If the path exists in base but is not a file, it might be a directory
                if replaced_path_with_content.exists().await?
                    && !replaced_path_with_content.is_file().await?
                {
                    // It's a directory in base. Find all files under it that were implicitly deleted.
                    let prefix = MPath::from(replaced_path.clone());
                    let files_under_prefix = base_ctx
                        .find_files_unordered(Some(vec![prefix]), None)
                        .await?
                        .try_collect::<Vec<_>>()
                        .await?;

                    for file_path in files_under_prefix {
                        // Convert MPath to NonRootMPath and add to all_paths
                        if let Some(non_root_path) = file_path.into_optional_non_root_path() {
                            all_paths.insert(non_root_path);
                        }
                    }
                }
            }
        }

        // Re-check base for newly discovered paths (ones added from replaced directories)
        let new_paths: Vec<NonRootMPath> = all_paths
            .iter()
            .filter(|p| !paths_to_check_first_pass.contains(*p))
            .cloned()
            .collect();
        for (mpath, create_change) in build_initial_tree_entries(new_paths, &base_ctx).await? {
            initial_tree.insert(mpath, Some(create_change));
        }

        // Generate merged changes
        let mut merged: BTreeMap<MPath, CreateChange> = BTreeMap::new();

        // Convert trees to maps for easier iteration
        let working_entries: BTreeMap<MPath, Option<CreateChange>> =
            working_tree.into_iter().collect();
        let initial_entries: BTreeMap<MPath, Option<CreateChange>> =
            initial_tree.into_iter().collect();

        // First, iterate over working_entries to find changes
        for (mpath, after) in &working_entries {
            let before = initial_entries.get(mpath).and_then(|v| v.as_ref());

            if create_change_content_equals(before, after.as_ref()) {
                // No change
                continue;
            }

            match after {
                None => {
                    // File was deleted (either explicitly or implicitly via prune)
                    merged.insert(mpath.clone(), CreateChange::Deletion);
                }
                Some(CreateChange::Deletion) | Some(CreateChange::UntrackedDeletion) => {
                    // Explicit deletion
                    merged.insert(mpath.clone(), after.clone().unwrap());
                }
                Some(CreateChange::Tracked(file, _)) => {
                    // File was added or modified
                    // Use resolved copy chain source if it exists in base
                    let copy_info = mpath
                        .clone()
                        .into_optional_non_root_path()
                        .and_then(|path| copy_chain.get(&path).cloned())
                        .and_then(|src_path| {
                            // Validate: source must be different path and exist in initial state
                            let src_mpath = MPath::from(src_path.clone());
                            let src_exists_in_base =
                                initial_entries.get(&src_mpath).is_some_and(|v| v.is_some());
                            if Some(src_path.clone()) != mpath.clone().into_optional_non_root_path()
                                && src_exists_in_base
                            {
                                // Parent index 0 since we have single parent (base)
                                Some(CreateCopyInfo::new(MPath::from(src_path), 0))
                            } else {
                                None
                            }
                        });

                    merged.insert(
                        mpath.clone(),
                        CreateChange::Tracked(file.clone(), copy_info),
                    );
                }
                Some(CreateChange::Untracked(file)) => {
                    merged.insert(mpath.clone(), CreateChange::Untracked(file.clone()));
                }
            }
        }

        // Second, check for paths in initial_entries that are missing from working_entries.
        // These are files that were implicitly deleted (e.g., by a directory becoming a file
        // and then being deleted).
        // IMPORTANT: Skip copy sources - files that were copied FROM (not renamed).
        // These files still exist and should not be marked as deleted.
        for (mpath, before) in &initial_entries {
            if before.is_some() && !working_entries.contains_key(mpath) {
                // Check if this path is a copy source (was copied FROM, not renamed)
                let is_copy_source = mpath
                    .clone()
                    .into_optional_non_root_path()
                    .is_some_and(|path| copy_sources.contains(&path));

                if is_copy_source {
                    // This file was copied FROM, not renamed - it should remain unchanged
                    continue;
                }

                // This file existed in base but is not in working_tree (was pruned)
                // It should be deleted
                if !merged.contains_key(mpath) {
                    merged.insert(mpath.clone(), CreateChange::Deletion);
                }
            }
        }

        // Resolve file types for any entries with file_type=None
        // This uses the stack_file_types (captured before additional_changes) and base_ctx

        // Phase 1: Sync resolution from stack_file_types
        let mut needs_parent_lookup: Vec<(NonRootMPath, NonRootMPath)> = Vec::new();
        for (mpath, change) in &mut merged {
            if let Some(path) = mpath.clone().into_optional_non_root_path() {
                if !change.resolve_file_type_from_stack(&path, &stack_file_types) {
                    let lookup_path = change.copy_source_or_path(&path);
                    needs_parent_lookup.push((path, lookup_path));
                }
            }
        }

        // Phase 2: Batch parent lookup using shared helper
        let unique_lookup_paths = needs_parent_lookup
            .iter()
            .map(|(_, lookup_path)| lookup_path.clone());
        let parent_file_types =
            lookup_file_types_from_parents(unique_lookup_paths, std::slice::from_ref(&base_ctx))
                .await?;

        // Phase 3: Apply resolved types from parents
        for (mpath, change) in &mut merged {
            if change.needs_file_type_resolution() {
                if let Some(path) = mpath.clone().into_optional_non_root_path() {
                    let lookup_path = change.copy_source_or_path(&path);
                    if let Some(ft) = parent_file_types.get(&lookup_path) {
                        change.set_file_type(*ft);
                    }
                }
            }
        }

        Ok(merged)
    }

    /// Apply file changes to the working tree, tracking copy chains and replaced directories.
    ///
    /// Converts FileChange to CreateChange and delegates to apply_create_changes.
    fn apply_file_changes(
        file_changes: SortedVectorMap<NonRootMPath, FileChange>,
        all_paths: &mut BTreeSet<NonRootMPath>,
        working_tree: &mut PathTree<Option<CreateChange>>,
        copy_chain: &mut HashMap<NonRootMPath, NonRootMPath>,
        copy_sources: &mut BTreeSet<NonRootMPath>,
        replaced_directory_paths: &mut BTreeSet<NonRootMPath>,
    ) {
        // Convert FileChange to CreateChange using From impl
        let create_changes: BTreeMap<MPath, CreateChange> = file_changes
            .into_iter()
            .map(|(path, change)| (MPath::from(path), CreateChange::from(&change)))
            .collect();

        Self::apply_create_changes(
            &create_changes,
            all_paths,
            working_tree,
            copy_chain,
            copy_sources,
            replaced_directory_paths,
        );
    }

    /// Apply CreateChange directly to the working tree.
    fn apply_create_changes(
        create_changes: &BTreeMap<MPath, CreateChange>,
        all_paths: &mut BTreeSet<NonRootMPath>,
        working_tree: &mut PathTree<Option<CreateChange>>,
        copy_chain: &mut HashMap<NonRootMPath, NonRootMPath>,
        copy_sources: &mut BTreeSet<NonRootMPath>,
        replaced_directory_paths: &mut BTreeSet<NonRootMPath>,
    ) {
        // First pass: additions/modifications
        for (mpath, change) in create_changes {
            if let Some(path) = mpath.clone().into_optional_non_root_path() {
                all_paths.insert(path.clone());

                match change {
                    CreateChange::Tracked(_, copy_info) => {
                        // Handle copy chain resolution
                        if let Some(copy_info) = copy_info {
                            if let Some(src_path) =
                                copy_info.path().clone().into_optional_non_root_path()
                            {
                                all_paths.insert(src_path.clone());

                                // Track that this source path is being copied FROM (not renamed).
                                // This is important because we need to distinguish between:
                                // - A file that was copied FROM (should remain unchanged)
                                // - A file that was explicitly deleted
                                // Without this, the copy source would be incorrectly marked as deleted
                                // in the final merged output, turning a copy into a rename.
                                // Only add to copy_sources if the source is NOT explicitly deleted
                                // in this changeset (if it's deleted, it's a rename, not a copy).
                                if !create_changes.iter().any(|(p, c)| {
                                    p.clone().into_optional_non_root_path()
                                        == Some(src_path.clone())
                                        && matches!(
                                            c,
                                            CreateChange::Deletion
                                                | CreateChange::UntrackedDeletion
                                        )
                                }) {
                                    copy_sources.insert(src_path.clone());
                                }

                                // Resolve the copy chain to find the ultimate source
                                let ultimate_src =
                                    copy_chain.get(&src_path).cloned().unwrap_or(src_path);

                                copy_chain.insert(path.clone(), ultimate_src);
                            }
                        } else {
                            // No explicit copy_from, remove any previous copy chain info
                            copy_chain.remove(&path);
                        }

                        // Insert and prune; file_type resolved later from stack or base
                        working_tree.insert_and_prune(mpath.clone(), Some(change.clone()));
                        replaced_directory_paths.insert(path);
                    }
                    CreateChange::Untracked(_) => {
                        working_tree.insert_and_prune(mpath.clone(), Some(change.clone()));
                        replaced_directory_paths.insert(path);
                    }
                    _ => {}
                }
            }
        }

        // Second pass: deletions
        for (mpath, change) in create_changes {
            if let Some(path) = mpath.clone().into_optional_non_root_path() {
                match change {
                    CreateChange::Deletion | CreateChange::UntrackedDeletion => {
                        working_tree.insert(mpath.clone(), Some(change.clone()));
                        copy_chain.remove(&path);
                        // If a copy source is explicitly deleted, remove it from copy_sources
                        copy_sources.remove(&path);
                    }
                    _ => {}
                }
            }
        }
    }
}
