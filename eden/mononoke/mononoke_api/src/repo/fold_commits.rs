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
use mononoke_types::BasicFileChange;
use mononoke_types::FileChange;
use mononoke_types::NonRootMPath;
use mononoke_types::TrackedFileChange;
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
use crate::repo::create_changeset::verify_deleted_files_existed_in_a_parent;
use crate::repo::create_changeset::verify_no_noop_file_changes;
use crate::repo::create_changeset::verify_prefix_files_deleted;

/// A wrapper around BasicFileChange that customizes equality and hashing for folding operations.
///
/// This wrapper contains all fields from BasicFileChange (content_id, file_type, size, git_lfs)
/// so we have all the information needed to reconstruct it, but it implements Hash and Eq
/// to only consider content_id and file_type for comparisons.
///
/// Consider a use case like this
///     base     | a.txt (content_id: 1)
///     commit_1 | dir_1/a.txt (content_id: 1)
///     commit_2 | a.txt (content_id: 1)
///
/// If we try to fold commits 1 and 2 its essentially a no-op
/// But After merging commit_1 and commit_2 we get (path: a.txt, copy_from: (dir_1/a.txt, commit_1)
/// Which is not correct. In order to fix this we need to compare this with initial state of file
///     1. If dir_1/a.txt exists in base then we can use that as copy_from
///     2. If do not exist then discard it
///
/// If i use basic file change for comparison then it includes git-lfs pointer which we cannot query unless
///     1. We traverse complete commit graph
///     2. Get changeset id from unode then get exact changeset, reconstruct the BasicFileChange and then compare
///
/// Based on conversation with @youssefsalama there is no clear way to change the GitLfs pointer without changing
/// the content_id or file_type and hence these fields should suffice for comparison

#[derive(Debug, Clone)]
struct FoldableFile {
    inner: BasicFileChange,
}

impl FoldableFile {
    fn new(inner: BasicFileChange) -> Self {
        Self { inner }
    }

    fn content_id(&self) -> mononoke_types::ContentId {
        self.inner.content_id()
    }

    fn file_type(&self) -> mononoke_types::FileType {
        self.inner.file_type()
    }

    fn size(&self) -> u64 {
        self.inner.size()
    }

    fn git_lfs(&self) -> mononoke_types::file_change::GitLfs {
        self.inner.git_lfs()
    }

    fn to_tracked_file_change(
        self,
        copy_from: Option<(NonRootMPath, ChangesetId)>,
    ) -> TrackedFileChange {
        TrackedFileChange::new(
            self.content_id(),
            self.file_type(),
            self.size(),
            copy_from,
            self.git_lfs(),
        )
    }
}

impl From<BasicFileChange> for FoldableFile {
    fn from(bfc: BasicFileChange) -> Self {
        Self::new(bfc)
    }
}

impl From<&BasicFileChange> for FoldableFile {
    fn from(bfc: &BasicFileChange) -> Self {
        Self::new(bfc.clone())
    }
}

// Custom PartialEq that only compares content_id and file_type
impl PartialEq for FoldableFile {
    fn eq(&self, other: &Self) -> bool {
        self.content_id() == other.content_id() && self.file_type() == other.file_type()
    }
}

impl Eq for FoldableFile {}

// Custom Hash that only hashes content_id and file_type
impl std::hash::Hash for FoldableFile {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.content_id().hash(state);
        self.file_type().hash(state);
    }
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

        let file_changes = self
            .merge_stack(cs_ids, bottom_parents[0], additional_changes, &checks)
            .await?;

        // 5. Create commit info - use provided info or generate default from target commit
        let commit_info = Self::build_commit_info(info, bottom_id, &top_ctx).await?;

        // 6. Create the new folded commit with the provided checks
        let created_changeset = self
            .create_changeset(
                vec![bottom_parents[0]], // Parent is bottom's parent
                commit_info,             // Commit metadata
                Self::convert_file_changes_to_create_changes(file_changes)?, // All the file changes
                None,                    // No bubble
                checks,
            )
            .await?;

        Ok(created_changeset)
    }

    async fn build_commit_info(
        info: Option<CreateInfo>,
        bottom_id: ChangesetId,
        top_ctx: &ChangesetContext<R>,
    ) -> Result<CreateInfo> {
        match info {
            Some(info) => Ok(info),
            None => {
                let target_commit_info = top_ctx.changeset_info().await?;
                let message = if bottom_id == top_ctx.id() {
                    format!(
                        "Amended commit {bottom_id}\n\n{}",
                        target_commit_info.message()
                    )
                } else {
                    format!(
                        "Folded commits {bottom_id} to {}\n\n{}",
                        top_ctx.id(),
                        target_commit_info.message()
                    )
                };

                Ok(CreateInfo {
                    author: target_commit_info.author().to_string(),
                    author_date: Utc::now().into(),
                    committer: None,
                    committer_date: None,
                    message,
                    extra: btreemap! {},
                    git_extra_headers: None,
                })
            }
        }
    }

    fn convert_file_changes_to_create_changes(
        changes: BTreeMap<NonRootMPath, FileChange>,
    ) -> Result<BTreeMap<MPath, CreateChange>> {
        changes
            .into_iter()
            .map(|(path, file_change)| {
                let mpath = MPath::from(path);
                let create_change = match file_change {
                    FileChange::Change(tracked) => {
                        let copy_info = tracked.copy_from().map(|(copy_path, _cs_id)| {
                            CreateCopyInfo::new(MPath::from(copy_path.clone()), 0)
                        });

                        CreateChange::Tracked(
                            CreateChangeFile {
                                contents: CreateChangeFileContents::Existing {
                                    file_id: tracked.content_id(),
                                    maybe_size: Some(tracked.size()),
                                },
                                file_type: tracked.file_type(),
                                git_lfs: match tracked.git_lfs() {
                                    mononoke_types::file_change::GitLfs::FullContent => {
                                        Some(CreateChangeGitLfs::FullContent)
                                    }
                                    mononoke_types::file_change::GitLfs::GitLfsPointer {
                                        non_canonical_pointer,
                                    } => Some(CreateChangeGitLfs::GitLfsPointer {
                                        non_canonical_pointer: non_canonical_pointer.map(|id| {
                                            CreateChangeFileContents::Existing {
                                                file_id: id,
                                                maybe_size: None,
                                            }
                                        }),
                                    }),
                                },
                            },
                            copy_info,
                        )
                    }
                    FileChange::Deletion => CreateChange::Deletion,
                    FileChange::UntrackedDeletion | FileChange::UntrackedChange(_) => {
                        // This should never happen during stack folding, as we only process
                        // committed changesets which don't have untracked changes
                        anyhow::bail!(
                            "logic error: unexpected FileChange variant {:?} in convert_file_changes_to_create_changes",
                            file_change
                        );
                    }
                };
                Ok((mpath, create_change))
            })
            .collect()
    }

    /// Convert RepoFoldCommitsParams changes to SortedVectorMap<NonRootMPath, FileChange>
    async fn convert_create_changes_to_file_changes(
        create_changes: BTreeMap<MPath, CreateChange>,
        parent_ids: &[ChangesetId],
    ) -> Result<SortedVectorMap<NonRootMPath, FileChange>, MononokeError> {
        create_changes
            .into_iter()
            .map(|(path, change)| {
                // Convert MPath to NonRootMPath - skip root paths as they can't be in file_changes
                let change_path = NonRootMPath::try_from(path)?;
                let file_change = change.into_file_change(parent_ids)?;
                Ok((change_path, file_change))
            })
            .collect()
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
    ) -> Result<BTreeMap<NonRootMPath, FileChange>> {
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

        // Use PathTree to track file states (None = deleted, Some(FoldableFile) = exists)
        // PathTree's hierarchical structure automatically handles implicit deletions
        let mut working_tree: PathTree<Option<FoldableFile>> = PathTree::default();

        // Track copy chains: maps current path -> ultimate source path
        // When a file is copied from another file that was itself copied, we follow
        // the chain to find the original source that should exist in the base commit.
        let mut copy_chain: HashMap<NonRootMPath, NonRootMPath> = HashMap::new();

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

            // Update stack_changes for each change in this commit
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
                &mut replaced_directory_paths,
            );
        }

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
                        &[base_ctx.clone()],
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
                &[base_ctx.clone()],
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
                &[base_ctx.clone()],
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
                        Some(file) => {
                            tree.insert_and_prune(mpath, Some(file.content_id()));
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
                    &[base_ctx.clone()],
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

            let file_changes =
                Self::convert_create_changes_to_file_changes(create_changes, &[base]).await?;
            Self::apply_file_changes(
                file_changes,
                &mut all_paths,
                &mut working_tree,
                &mut copy_chain,
                &mut replaced_directory_paths,
            );
        }

        let mut initial_tree: PathTree<Option<FoldableFile>> = PathTree::default();

        // Remember which paths we check in this first pass
        let paths_to_check_first_pass: BTreeSet<NonRootMPath> = all_paths.clone();

        // Check each touched path in base commit
        for path in &paths_to_check_first_pass {
            let path_with_content = base_ctx.path_with_content(path.clone()).await?;

            if path_with_content.exists().await? && path_with_content.is_file().await? {
                if let Some(file_type) = path_with_content.file_type().await? {
                    if let Some(file_ctx) = path_with_content.file().await? {
                        let metadata = file_ctx.metadata().await?;
                        let basic_file_change = BasicFileChange::new(
                            metadata.content_id,
                            file_type,
                            metadata.total_size,
                            mononoke_types::file_change::GitLfs::FullContent,
                        );
                        initial_tree.insert(
                            MPath::from(path.clone()),
                            Some(FoldableFile::new(basic_file_change)),
                        );
                    }
                }
            }
        }

        // For paths where we did insert_and_prune (directory->file replacement),
        // check if those paths are now deleted and had files underneath in base.
        // Those files need explicit deletions.
        for replaced_path in &replaced_directory_paths {
            // Check if this path is currently deleted or never existed as a file in working_tree
            let current_state = working_tree.get((&MPath::from(replaced_path.clone())).into());
            let is_deleted_or_absent = match current_state {
                Some(Some(_)) => false, // File exists in working tree
                _ => true,              // Deleted (None) or not in tree
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
        for path in &all_paths {
            if paths_to_check_first_pass.contains(path) {
                continue; // Already checked in the first pass
            }

            let path_with_content = base_ctx.path_with_content(path.clone()).await?;

            if path_with_content.exists().await? && path_with_content.is_file().await? {
                if let Some(file_type) = path_with_content.file_type().await? {
                    if let Some(file_ctx) = path_with_content.file().await? {
                        let metadata = file_ctx.metadata().await?;
                        let basic_file_change = BasicFileChange::new(
                            metadata.content_id,
                            file_type,
                            metadata.total_size,
                            mononoke_types::file_change::GitLfs::FullContent,
                        );
                        initial_tree.insert(
                            MPath::from(path.clone()),
                            Some(FoldableFile::new(basic_file_change)),
                        );
                    }
                }
            }
        }

        // Generate merged changes
        let mut merged: BTreeMap<NonRootMPath, FileChange> = BTreeMap::new();

        // Convert trees to maps for easier iteration
        let working_entries: BTreeMap<MPath, Option<FoldableFile>> =
            working_tree.into_iter().collect();
        let initial_entries: BTreeMap<MPath, Option<FoldableFile>> =
            initial_tree.into_iter().collect();

        // First, iterate over working_entries to find changes
        for (mpath, after) in &working_entries {
            let before = initial_entries.get(mpath).cloned().unwrap_or(None);

            if before == *after {
                // No change
                continue;
            }

            let path = NonRootMPath::try_from(mpath.clone())?;
            match after {
                None => {
                    // File was deleted (either explicitly or implicitly)
                    merged.insert(path.clone(), FileChange::Deletion);
                }
                Some(file) => {
                    // File was added or modified
                    // Use resolved copy chain source if it exists in base
                    let copy_from = copy_chain.get(&path).cloned().and_then(|src_path| {
                        // Validate: source must be different path and exist in initial state
                        let src_mpath = MPath::from(src_path.clone());
                        let src_exists_in_base = initial_entries
                            .get(&src_mpath)
                            .map_or(false, |v| v.is_some());
                        if src_path != path && src_exists_in_base {
                            // Use base as the changeset ID since that's the parent of the folded commit
                            Some((src_path, base))
                        } else {
                            None
                        }
                    });

                    merged.insert(
                        path.clone(),
                        FileChange::Change(file.clone().to_tracked_file_change(copy_from)),
                    );
                }
            }
        }

        // Second, check for paths in initial_entries that are missing from working_entries.
        // These are files that were implicitly deleted (e.g., by a directory becoming a file
        // and then being deleted).
        for (mpath, before) in &initial_entries {
            if before.is_some() && !working_entries.contains_key(mpath) {
                // This file existed in base but is not in working_tree (was pruned)
                // It should be deleted
                let path = NonRootMPath::try_from(mpath.clone())?;
                if !merged.contains_key(&path) {
                    merged.insert(path, FileChange::Deletion);
                }
            }
        }

        Ok(merged)
    }

    /// Apply file changes to the working tree, tracking copy chains and replaced directories.
    ///
    /// Uses a two-pass approach:
    /// 1. First pass: Process additions/modifications to build copy chains before any deletions
    /// 2. Second pass: Process deletions
    ///
    /// This is necessary because the iterator may return deletions before copies alphabetically.
    fn apply_file_changes(
        file_changes: SortedVectorMap<NonRootMPath, FileChange>,
        all_paths: &mut BTreeSet<NonRootMPath>,
        working_tree: &mut PathTree<Option<FoldableFile>>,
        copy_chain: &mut HashMap<NonRootMPath, NonRootMPath>,
        replaced_directory_paths: &mut BTreeSet<NonRootMPath>,
    ) {
        // First pass: additions/modifications
        for (path, change) in &file_changes {
            all_paths.insert(path.clone());

            if let FileChange::Change(tracked) = change {
                let file = FoldableFile::from(tracked.basic_file_change());

                // Handle copy chain resolution
                if let Some((src_path, _src_cs_id)) = tracked.copy_from() {
                    all_paths.insert(src_path.clone());

                    // Resolve the copy chain to find the ultimate source
                    let ultimate_src = copy_chain
                        .get(src_path)
                        .cloned()
                        .unwrap_or_else(|| src_path.clone());

                    copy_chain.insert(path.clone(), ultimate_src);
                } else {
                    // No explicit copy_from, remove any previous copy chain info
                    copy_chain.remove(path);
                }

                // Insert and prune children (handles implicit deletions when directory becomes file)
                working_tree.insert_and_prune(MPath::from(path.clone()), Some(file));
                // Track this path in case it had files underneath that need explicit deletion
                replaced_directory_paths.insert(path.clone());
            }
        }

        // Second pass: deletions
        for (path, change) in &file_changes {
            match change {
                FileChange::Deletion => {
                    // Mark as deleted and prune all children (handles implicit deletions)
                    working_tree.insert(MPath::from(path.clone()), None);
                    copy_chain.remove(path);
                }
                FileChange::Change(_) => {
                    // Already processed in first pass
                }
                FileChange::UntrackedDeletion | FileChange::UntrackedChange(_) => {
                    // These shouldn't occur when processing committed changesets
                }
            }
        }
    }
}
