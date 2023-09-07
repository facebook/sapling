/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashSet;

use anyhow::anyhow;
use blobstore::Loadable;
use borrowed::borrowed;
use bytes::Bytes;
use changesets::ChangesetsRef;
use chrono::DateTime;
use chrono::FixedOffset;
use context::CoreContext;
use ephemeral_blobstore::Bubble;
use filestore::FetchKey;
use filestore::FilestoreConfig;
use filestore::FilestoreConfigRef;
use filestore::StoreRequest;
use futures::stream;
use futures::stream::Stream;
use futures::stream::TryStreamExt;
use futures::try_join;
use futures::StreamExt;
use futures_stats::TimedFutureExt;
use itertools::Itertools;
use manifest::PathTree;
use metaconfig_types::RepoConfigRef;
use mononoke_types::fsnode::FsnodeEntry;
use mononoke_types::path::MPath;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime as MononokeDateTime;
use mononoke_types::FileChange;
use mononoke_types::MPathElement;
use mononoke_types::NonRootMPath;
use repo_authorization::RepoWriteOperation;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_identity::RepoIdentityRef;
use repo_update_logger::log_new_commits;
use repo_update_logger::CommitInfo;
use smallvec::SmallVec;
use sorted_vector_map::SortedVectorMap;

use crate::changeset::ChangesetContext;
use crate::errors::MononokeError;
use crate::file::FileId;
use crate::file::FileType;
use crate::path::MononokePath;
use crate::repo::RepoContext;
use crate::specifiers::ChangesetSpecifier;

#[derive(Clone)]
pub struct CreateCopyInfo {
    path: MononokePath,
    parent_index: usize,
}

impl CreateCopyInfo {
    pub fn new(path: MononokePath, parent_index: usize) -> Self {
        CreateCopyInfo { path, parent_index }
    }

    async fn check_valid(
        &self,
        stack_changes: Option<&PathTree<CreateChangeType>>,
        stack_parents: &[ChangesetContext],
    ) -> Result<(), MononokeError> {
        if let Some(stack_changes) = stack_changes {
            // Since this is a stacked commit, there is only one parent.
            if self.parent_index > 0 {
                return Err(MononokeError::InvalidRequest(format!(
                    "Parent index '{}' out of range for stacked commit",
                    self.parent_index,
                )));
            }
            // Check if the copy-from path was added or removed in the stack.
            match stack_changes.get(self.path.as_mpath().into()) {
                None | Some(CreateChangeType::None) => {}
                Some(CreateChangeType::Change) => {
                    return Ok(());
                }
                Some(CreateChangeType::Deletion) => {
                    return Err(MononokeError::InvalidRequest(String::from(
                        "Copy-from path references a file deleted earler in the stack",
                    )));
                }
            }
            // Check if the copy-from path was deleted by a prefix change.
            for prefix in self.path.prefixes() {
                if stack_changes.get(prefix.as_mpath().into()) == Some(&CreateChangeType::Change) {
                    return Err(MononokeError::InvalidRequest(String::from(
                        "Copy-from path references a file in a directory deleted earler in the stack",
                    )));
                }
            }
            // The copy-from path wasn't touched in the stack, check it was in
            // at least one of the stack's parents.
            for parent_ctx in stack_parents {
                if parent_ctx
                    .path_with_content(self.path.clone())
                    .await?
                    .is_file()
                    .await?
                {
                    return Ok(());
                }
            }
        } else {
            // This is the root of the stack.  Check the specific parent.
            let parent_ctx = stack_parents.get(self.parent_index).ok_or_else(|| {
                MononokeError::InvalidRequest(format!(
                    "Parent index '{}' out of range for commit with {} parent(s)",
                    self.parent_index,
                    stack_parents.len()
                ))
            })?;
            // Check the file exists in that parent.
            if parent_ctx
                .path_with_content(self.path.clone())
                .await?
                .is_file()
                .await?
            {
                return Ok(());
            }
        };

        Err(MononokeError::InvalidRequest(String::from(
            "Copy-from path must reference a file",
        )))
    }

    fn into_file_change(
        self,
        parent_ids: &[ChangesetId],
    ) -> Result<(NonRootMPath, ChangesetId), MononokeError> {
        let mpath = self.path.into_mpath().ok_or_else(|| {
            MononokeError::InvalidRequest(String::from("Copy-from path cannot be the root"))
        })?;
        let parent_id = parent_ids.get(self.parent_index).ok_or_else(|| {
            MononokeError::InvalidRequest(format!(
                "Parent index '{}' out of range for commit with {} parent(s)",
                self.parent_index,
                parent_ids.len()
            ))
        })?;
        Ok((mpath, *parent_id))
    }
}

/// Description of a change to make to a file.
#[derive(Clone)]
pub enum CreateChange {
    /// The file is created or modified to contain new data.
    Tracked(CreateChangeFile, Option<CreateCopyInfo>),

    /// A new file is modified in the working copy
    Untracked(CreateChangeFile),

    /// The file is not present in the working copy
    UntrackedDeletion,

    /// The file is marked as deleted
    Deletion,
}

#[derive(Clone)]
pub enum CreateChangeFile {
    // Upload content from bytes
    New {
        bytes: Bytes,
        file_type: FileType,
    },
    // Use already uploaded content
    Existing {
        file_id: FileId,
        file_type: FileType,
        // If not present, will be fetched from the blobstore
        maybe_size: Option<u64>,
    },
}

// Enum for recording whether a path is not changed, changed or deleted.
#[derive(Copy, Clone, Default, Eq, PartialEq, Debug)]
enum CreateChangeType {
    #[default]
    None,
    Change,
    Deletion,
}

impl CreateChangeType {
    fn is_modification(&self) -> bool {
        match self {
            Self::None => false,
            Self::Change => true,
            Self::Deletion => true,
        }
    }
}

impl CreateChange {
    async fn resolve(
        &mut self,
        ctx: &CoreContext,
        filestore_config: FilestoreConfig,
        repo_blobstore: RepoBlobstore,
        stack_changes: Option<&PathTree<CreateChangeType>>,
        stack_parents: &[ChangesetContext],
    ) -> Result<(), MononokeError> {
        let file = match self {
            CreateChange::Tracked(file, copy_info) => {
                if let Some(copy_info) = copy_info {
                    copy_info.check_valid(stack_changes, stack_parents).await?;
                }
                file
            }
            CreateChange::Untracked(file) => file,
            CreateChange::UntrackedDeletion | CreateChange::Deletion => return Ok(()),
        };
        match file {
            CreateChangeFile::New { bytes, file_type } => {
                let meta = filestore::store(
                    &repo_blobstore,
                    filestore_config,
                    ctx,
                    &StoreRequest::new(bytes.len() as u64),
                    stream::once(async move { Ok(bytes.clone()) }),
                )
                .await?;
                let file_type = *file_type;
                *file = CreateChangeFile::Existing {
                    file_id: meta.content_id,
                    file_type,
                    maybe_size: Some(meta.total_size),
                };
            }
            CreateChangeFile::Existing {
                file_id,
                maybe_size,
                ..
            } => {
                if maybe_size.is_none() {
                    let size = filestore::get_metadata(
                        &repo_blobstore,
                        ctx,
                        &FetchKey::Canonical(*file_id),
                    )
                    .await?
                    .ok_or_else(|| {
                        MononokeError::InvalidRequest(format!(
                            "File id '{}' is not available in this repo",
                            file_id
                        ))
                    })?
                    .total_size;
                    *maybe_size = Some(size);
                }
            }
        }
        Ok(())
    }

    pub fn into_file_change(self, parent_ids: &[ChangesetId]) -> Result<FileChange, MononokeError> {
        match self {
            CreateChange::Tracked(
                CreateChangeFile::Existing {
                    file_id,
                    file_type,
                    maybe_size: Some(size),
                },
                copy_info,
            ) => Ok(FileChange::tracked(
                file_id,
                file_type,
                size,
                copy_info
                    .map(|copy_info| copy_info.into_file_change(parent_ids))
                    .transpose()?,
            )),
            CreateChange::Untracked(CreateChangeFile::Existing {
                file_id,
                file_type,
                maybe_size: Some(size),
            }) => Ok(FileChange::untracked(file_id, file_type, size)),
            CreateChange::UntrackedDeletion => Ok(FileChange::UntrackedDeletion),
            CreateChange::Deletion => Ok(FileChange::Deletion),
            _ => Err(anyhow!("Programming error: create change must be resolved first").into()),
        }
    }

    fn change_type(&self) -> CreateChangeType {
        match self {
            CreateChange::Deletion | CreateChange::UntrackedDeletion => CreateChangeType::Deletion,
            CreateChange::Tracked(..) | CreateChange::Untracked(..) => CreateChangeType::Change,
        }
    }
}

/// Commit info for a newly created commit.
pub struct CreateInfo {
    pub author: String,
    pub author_date: DateTime<FixedOffset>,
    pub committer: Option<String>,
    pub committer_date: Option<DateTime<FixedOffset>>,
    pub message: String,
    pub extra: BTreeMap<String, Vec<u8>>,
    pub git_extra_headers: Option<BTreeMap<SmallVec<[u8; 24]>, Bytes>>,
}

/// Verify that all deleted files existed in at least one of the parents.
async fn verify_deleted_files_existed_in_a_parent(
    parent_ctxs: &[ChangesetContext],
    stack_changes: Option<&PathTree<CreateChangeType>>,
    mut deleted_files: BTreeSet<MononokePath>,
) -> Result<(), MononokeError> {
    async fn get_matching_files<'a>(
        parent_ctx: &'a ChangesetContext,
        files: &'a BTreeSet<MononokePath>,
    ) -> Result<impl Stream<Item = Result<MononokePath, MononokeError>> + 'a, MononokeError> {
        Ok(parent_ctx
            .paths(files.iter().cloned())
            .await?
            .try_filter_map(|changeset_path| async move {
                if changeset_path.is_file().await? {
                    Ok(Some(changeset_path.path().clone()))
                } else {
                    Ok(None)
                }
            })
            .boxed())
    }

    if let Some(stack_changes) = stack_changes {
        // Ignore files that were created or modified earlier in the stack.
        deleted_files.retain(|deleted_file| {
            stack_changes.get(deleted_file.as_mpath().into()) != Some(&CreateChangeType::Change)
        });

        for deleted_file in deleted_files.iter() {
            // It's an error if this file was already deleted, or if any of
            // its path prefixes were created (this implicitly deletes the
            // directory).
            if stack_changes.get(deleted_file.as_mpath().into())
                == Some(&CreateChangeType::Deletion)
            {
                return Err(MononokeError::InvalidRequest(format!(
                    "Deleted file '{}' was deleted earlier in the stack",
                    deleted_file
                )));
            }
            for prefix in deleted_file.prefixes() {
                if let Some(CreateChangeType::Change) = stack_changes.get(prefix.as_mpath().into())
                {
                    return Err(MononokeError::InvalidRequest(format!(
                        "Deleted file '{}' was deleted earlier in the stack through replacement of '{}'",
                        deleted_file, prefix
                    )));
                }
            }
        }
    }

    // Filter the deleted files to those that existed in a parent.
    let deleted_files = &deleted_files;
    let parent_files: BTreeSet<_> = stream::iter(
        parent_ctxs
            .iter()
            .map(|parent_ctx| async move { get_matching_files(parent_ctx, deleted_files).await }),
    )
    .boxed()
    .buffered(10)
    .try_flatten()
    .try_collect()
    .await?;

    // Quickly check if all deleted files existed by comparing set lengths.
    if deleted_files.len() == parent_files.len() {
        Ok(())
    } else {
        // At least one deleted file didn't exist. Find out which ones to
        // give a good error message.
        let non_existent_path = deleted_files
            .difference(&parent_files)
            .next()
            .expect("at least one file did not exist");
        let path_count = deleted_files.len().saturating_sub(parent_files.len());
        if path_count == 1 {
            Err(MononokeError::InvalidRequest(format!(
                "Deleted file '{}' does not exist in any parent",
                non_existent_path
            )))
        } else {
            Err(MononokeError::InvalidRequest(format!(
                "{} deleted files ('{}', ...) do not exist in any parent",
                path_count, non_existent_path
            )))
        }
    }
}

/// Returns `true` if any prefix of the path has a change.  Use for
/// detecting when a directory is replaced by a file.
fn is_prefix_changed(path: &MononokePath, paths: &PathTree<CreateChangeType>) -> bool {
    path.prefixes()
        .any(|prefix| paths.get(prefix.as_mpath().into()) == Some(&CreateChangeType::Change))
}

/// Verify that any files in `prefix_paths` that exist in any of
/// `parent_ctxs`, as modified by the existing stack changes, have been marked
/// as deleted in `path_changes`.
async fn verify_prefix_files_deleted(
    parent_ctxs: &[ChangesetContext],
    stack_changes: Option<&PathTree<CreateChangeType>>,
    mut prefix_paths: BTreeSet<MononokePath>,
    path_changes: &PathTree<CreateChangeType>,
) -> Result<(), MononokeError> {
    if let Some(stack_changes) = stack_changes {
        // Remove any prefix paths that have already been deleted earlier in the stack.
        prefix_paths.retain(|prefix_path| {
            stack_changes.get(prefix_path.as_mpath().into()) != Some(&CreateChangeType::Deletion)
        });
        // Check that any prefix path added earlier in the stack is being deleted.
        for prefix_path in prefix_paths.iter() {
            if stack_changes.get(prefix_path.as_mpath().into()) == Some(&CreateChangeType::Change)
                && path_changes.get(prefix_path.as_mpath().into())
                    != Some(&CreateChangeType::Deletion)
            {
                return Err(MononokeError::InvalidRequest(format!(
                    concat!(
                        "Creating files inside '{}' requires deleting the file ",
                        "added earlier in the stack at that path"
                    ),
                    prefix_path
                )));
            }
        }
    }
    // Check that any prefix path that exists in any parent is being deleted.
    borrowed!(prefix_paths);
    stream::iter(parent_ctxs.iter().map(Ok))
        .try_for_each_concurrent(10, |parent_ctx| async move {
            parent_ctx
                .paths(prefix_paths.iter().cloned())
                .await?
                .try_for_each(|prefix_path| async move {
                    if prefix_path.is_file().await?
                        && path_changes.get(prefix_path.path().as_mpath().into())
                            != Some(&CreateChangeType::Deletion)
                    {
                        Err(MononokeError::InvalidRequest(format!(
                            "Creating files inside '{}' requires deleting the file at that path",
                            prefix_path.path()
                        )))
                    } else {
                        Ok(())
                    }
                })
                .await
        })
        .await
}

async fn check_addless_union_conflicts(
    ctx: &CoreContext,
    repo_blobstore: RepoBlobstore,
    changesets: &[ChangesetContext],
    fix_paths: &PathTree<CreateChangeType>,
) -> Result<(), MononokeError> {
    if changesets.len() < 2 {
        return Ok(());
    }

    let root_fsnodes: Vec<_> = stream::iter(changesets.iter().map(|cs_ctx| async move {
        Ok::<_, MononokeError>(cs_ctx.root_fsnode_id().await?.into_fsnode_id())
    }))
    .boxed()
    .buffered(10)
    .try_collect()
    .await?;

    let store = &repo_blobstore;

    let conflict_paths = bounded_traversal::bounded_traversal_stream(
        256,
        Some((root_fsnodes, MononokePath::new(None))),
        move |(fsnodes_to_check, current_path)| {
            Box::pin(async move {
                let mut leaf_content: BTreeMap<MPathElement, HashSet<_>> = BTreeMap::new();
                let mut trees: BTreeMap<MPathElement, BTreeSet<_>> = BTreeMap::new();

                for fsnode in fsnodes_to_check {
                    let fsnode = fsnode.load(ctx, store).await?;
                    for (path_element, entry) in fsnode.list() {
                        match entry {
                            FsnodeEntry::Directory(directory) => trees
                                .entry(path_element.clone())
                                .or_default()
                                .insert(*directory.id()),
                            FsnodeEntry::File(file) => leaf_content
                                .entry(path_element.clone())
                                .or_default()
                                .insert(*file),
                        };
                    }
                }

                // Conflict rules only apply to leaves. A path in `fix_paths` means no conflict
                //
                // Two rules:
                // 1. If there are multiple choices for content, then there's a conflict
                // 2. If there's a tree and a leaf for this path, then there's a conflict
                let conflicts: Vec<_> = leaf_content
                    .into_iter()
                    .filter_map(|(path_element, contents)| {
                        let path = current_path.append(&path_element);
                        let fix_exists = fix_paths
                            .get(path.as_mpath().into())
                            .map_or(false, CreateChangeType::is_modification);
                        let conflict_exists =
                            contents.len() > 1 || trees.contains_key(&path_element);
                        if !fix_exists && conflict_exists {
                            Some(path)
                        } else {
                            None
                        }
                    })
                    .collect();
                // Recurse into trees that might reveal more conflicts.
                // If we already have new content for a path, then we don't recurse into it
                let recurse: Vec<_> = trees
                    .into_iter()
                    .filter_map(|(path_element, fsnodes)| {
                        let path = current_path.append(&path_element);
                        let fix_exists = fix_paths
                            .get(path.as_mpath().into())
                            .map_or(false, CreateChangeType::is_modification);

                        if !fix_exists && fsnodes.len() > 1 {
                            Some((fsnodes.into_iter().collect(), path))
                        } else {
                            None
                        }
                    })
                    .collect();
                anyhow::Ok((conflicts, recurse))
            })
        },
    )
    .try_concat()
    .await?;

    if conflict_paths.is_empty() {
        Ok(())
    } else {
        Err(MononokeError::MergeConflicts { conflict_paths })
    }
}

impl RepoContext {
    pub(crate) async fn save_changesets(
        &self,
        changesets: Vec<BonsaiChangeset>,
        repo: &(impl ChangesetsRef + RepoBlobstoreRef + RepoIdentityRef + RepoConfigRef),
        bubble: Option<&Bubble>,
    ) -> Result<(), MononokeError> {
        let bubble_id = bubble.map(|x| x.bubble_id());
        let commit_infos = changesets
            .iter()
            .map(|changeset| CommitInfo::new(changeset, bubble_id))
            .collect();
        changesets_creation::save_changesets(self.ctx(), repo, changesets).await?;

        log_new_commits(self.ctx(), repo, None, commit_infos).await;

        Ok(())
    }

    /// Create a new changeset in the repository.
    ///
    /// The new changeset is created with the given metadata by unioning the
    /// contents of all parent changesets and then applying the provided
    /// changes on top.
    ///
    /// Note that:
    ///   - The changes must be internally consistent (there must be no path
    ///     conflicts between changed files).
    ///   - If a file in any parent changeset is being replaced by a directory
    ///     then that file must be deleted in the set of changes.
    ///   - If a directory in any parent changeset is being replaced by a file,
    ///     then the contents of the parent directory do not need to be deleted.
    ///     If deletions for the contents of the directory are included they will
    ///     be checked for correctness (the files must exist), but they will
    ///     otherwise be ignored.
    ///   - Any merge conflicts introduced by merging the parent changesets
    ///     must be resolved by a corresponding change in the set of changes.
    pub async fn create_changeset(
        &self,
        parents: Vec<ChangesetId>,
        info: CreateInfo,
        changes: BTreeMap<MononokePath, CreateChange>,
        // If some, this changeset is a snapshot. Currently unsupported to upload a
        // normal commit to a bubble, though can be easily added.
        bubble: Option<&Bubble>,
    ) -> Result<ChangesetContext, MononokeError> {
        let changesets = self
            .create_changeset_stack(parents, vec![info], vec![changes], bubble)
            .await?;
        changesets
            .into_iter()
            .exactly_one()
            .map_err(|e| anyhow!("Expected 1 changeset, but created {}", e.len()).into())
    }

    /// Create a new stack of changesets in the repository.
    ///
    /// The first new changeset is created with the given metadata by unioning the
    /// contents of all parent changesets and then applying the provided
    /// changes on top.  The subsequent changesets are then stacked on top of
    /// the first changeset.
    ///
    /// The requirements for `create_changeset` must be met for each changeset
    /// in the stack.
    pub async fn create_changeset_stack(
        &self,
        stack_parents: Vec<ChangesetId>,
        info_stack: Vec<CreateInfo>,
        changes_stack: Vec<BTreeMap<MononokePath, CreateChange>>,
        // If some, this changeset is a snapshot. Currently unsupported to upload a
        // normal commit to a bubble, though can be easily added.
        bubble: Option<&Bubble>,
    ) -> Result<Vec<ChangesetContext>, MononokeError> {
        self.start_write()?;
        self.authorization_context()
            .require_repo_write(
                self.ctx(),
                self.inner_repo(),
                RepoWriteOperation::CreateChangeset,
            )
            .await?;

        let allowed_no_parents = self
            .config()
            .source_control_service
            .permit_commits_without_parents;
        if !allowed_no_parents && stack_parents.is_empty() {
            return Err(MononokeError::InvalidRequest(String::from(
                "Changesets with no parents cannot be created",
            )));
        }

        // Obtain contexts for each of the parents (which should exist).
        let stack_parent_ctxs: Vec<_> =
            stream::iter(stack_parents.iter().map(|parent_id| async move {
                let parent_ctx = self
                    .changeset(ChangesetSpecifier::Bonsai(parent_id.clone()))
                    .await?
                    .ok_or_else(|| {
                        MononokeError::InvalidRequest(format!(
                            "Parent {} does not exist",
                            parent_id
                        ))
                    })?;
                Ok::<_, MononokeError>(parent_ctx)
            }))
            .boxed()
            .buffered(10)
            .try_collect()
            .await?;
        borrowed!(stack_parent_ctxs);

        // Collect together information about each new commit.

        // Extract the set of deleted files.
        let tracked_deletion_files_stack: Vec<BTreeSet<_>> = changes_stack
            .iter()
            .map(|changes| {
                changes
                    .iter()
                    .filter(|(_path, change)| matches!(change, CreateChange::Deletion))
                    .map(|(path, _change)| path.clone())
                    .collect()
            })
            .collect();

        // Build a path tree recording each path that has been created or deleted.
        let path_changes_stack: Vec<_> = changes_stack
            .iter()
            .map(|changes| {
                PathTree::from_iter(changes.iter().map(|(path, change)| {
                    (MPath::from(path.as_mpath().cloned()), change.change_type())
                }))
            })
            .collect();
        let path_changes_stack = path_changes_stack.as_slice();

        // Determine the prefixes of all changed files.
        let prefix_paths_stack: Vec<BTreeSet<_>> = changes_stack
            .iter()
            .map(|changes| {
                changes
                    .iter()
                    .filter(|(_path, change)| change.change_type() == CreateChangeType::Change)
                    .flat_map(|(path, _change)| path.clone().prefixes())
                    .collect()
            })
            .collect();

        // Convert change paths into the form needed for the bonsai changeset.
        let changes_stack: Vec<Vec<(NonRootMPath, CreateChange)>> = changes_stack
            .into_iter()
            .zip(path_changes_stack.iter())
            .map(|(changes, path_changes)| {
                changes
                    .into_iter()
                    // Filter deletions that have a change at a path prefix. The
                    // deletion is implicit from the change. (2)
                    .filter(|(path, change)| {
                        change.change_type() != CreateChangeType::Deletion
                            || !is_prefix_changed(path, path_changes)
                    })
                    // Then convert the paths to MPaths. Do this before we start
                    // resolving any changes, so that we don't start storing data
                    // until we're happy that the changes are valid.
                    .map(|(path, change)| {
                        path.into_mpath()
                            .ok_or_else(|| {
                                MononokeError::InvalidRequest(String::from(
                                    "Cannot create a file with an empty path",
                                ))
                            })
                            .map(move |mpath| (mpath, change))
                    })
                    .collect::<Result<_, _>>()
            })
            .collect::<Result<_, _>>()?;

        // Track the changes already made so far at each step in the stack.
        let stack_changes_stack = {
            let mut stack_changes_stack = vec![None];
            let mut stack_changes = PathTree::default();
            for (index, path_changes) in path_changes_stack.iter().enumerate() {
                if index < path_changes_stack.len() - 1 {
                    for (path, change) in path_changes.clone().into_iter() {
                        match change {
                            CreateChangeType::Change => {
                                stack_changes.insert_and_prune(path, change);
                            }
                            CreateChangeType::Deletion => {
                                stack_changes.insert(path, change);
                            }
                            CreateChangeType::None => {}
                        }
                    }
                    stack_changes_stack.push(Some(stack_changes.clone()))
                }
            }
            stack_changes_stack
        };
        let stack_changes_stack = stack_changes_stack.as_slice();

        // Check that changes are valid according to bonsai rules:
        // (1) deletions and copy-from info must reference a real path in a
        //     valid parent.
        // (2) deletions for paths where a prefix directory has been replaced
        //     by a file should be dropped, as the deletion is implicit from the
        //     file change for the prefix path.
        // (3) conversely, when a file has been replaced by a directory, there
        //     must be a delete for the file.

        // Check deleted files existed in a parent. (1)
        let verify_deleted_files_existed_fut = async move {
            stream::iter(
                tracked_deletion_files_stack
                    .into_iter()
                    .zip(stack_changes_stack.iter())
                    .map(Ok),
            )
            .try_for_each_concurrent(10, |(tracked_deletion_files, stack_changes)| async move {
                // This does NOT consider "missing" (untracked deletion) files as it is NOT
                // necessary for them to exist in a parent. If they don't exist on a parent,
                // this means the file was "hg added" and then manually deleted.
                let (stats, result) = verify_deleted_files_existed_in_a_parent(
                    stack_parent_ctxs,
                    stack_changes.as_ref(),
                    tracked_deletion_files,
                )
                .timed()
                .await;
                let mut scuba = self.ctx().scuba().clone();
                scuba.add_future_stats(&stats);
                scuba.log_with_msg("Verify deleted files existed in a parent", None);
                result
            })
            .await
        };

        // Check changes that replace a file with a directory also delete
        // this replaced file. (3)
        let verify_prefix_files_deleted_fut = async move {
            stream::iter(
                prefix_paths_stack
                    .into_iter()
                    .zip(path_changes_stack.iter())
                    .zip(stack_changes_stack.iter())
                    .map(Ok),
            )
            .try_for_each_concurrent(
                10,
                |((prefix_paths, path_changes), stack_changes)| async move {
                    let (stats, result) = verify_prefix_files_deleted(
                        stack_parent_ctxs,
                        stack_changes.as_ref(),
                        prefix_paths,
                        path_changes,
                    )
                    .timed()
                    .await;
                    let mut scuba = self.ctx().scuba().clone();
                    scuba.add_future_stats(&stats);
                    scuba.log_with_msg("Verify prefix files in parents have been deleted", None);
                    result
                },
            )
            .await
        };

        // Check for merge conflicts.  This only applies to the first commit
        // in a stack.
        let verify_no_merge_conflicts_fut = async {
            let (stats, result) = check_addless_union_conflicts(
                self.ctx(),
                match &bubble {
                    Some(bubble) => {
                        bubble.wrap_repo_blobstore(self.blob_repo().repo_blobstore().clone())
                    }
                    None => self.blob_repo().repo_blobstore().clone(),
                },
                stack_parent_ctxs,
                path_changes_stack
                    .first()
                    .ok_or_else(|| anyhow!("Should be at least one commit"))?,
            )
            .timed()
            .await;

            let mut scuba = self.ctx().scuba().clone();
            scuba.add_future_stats(&stats);
            scuba.log_with_msg("Verify all merge conflicts are resolved", None);
            result
        };

        // Resolve the changes so that they are ready to be converted into
        // bonsai changes. This also checks (1) for copy-from info.
        let blobstore = match &bubble {
            Some(bubble) => bubble.wrap_repo_blobstore(self.blob_repo().repo_blobstore().clone()),
            None => self.blob_repo().repo_blobstore().clone(),
        };
        borrowed!(blobstore);
        let resolve_file_changes_fut = async move {
            stream::iter(
                changes_stack
                    .into_iter()
                    .zip(stack_changes_stack.iter())
                    .map(|(changes, stack_changes)| async move {
                        let stack_changes = stack_changes.as_ref();
                        let (stats, result) = stream::iter(changes.into_iter().map(
                            |(path, mut change)| async move {
                                change
                                    .resolve(
                                        self.ctx(),
                                        *self.blob_repo().filestore_config(),
                                        blobstore.clone(),
                                        stack_changes,
                                        stack_parent_ctxs,
                                    )
                                    .await?;
                                Ok::<_, MononokeError>((path, change))
                            },
                        ))
                        .buffered(1000)
                        .try_collect::<SortedVectorMap<NonRootMPath, CreateChange>>()
                        .timed()
                        .await;
                        let mut scuba = self.ctx().scuba().clone();
                        scuba.add_future_stats(&stats);
                        scuba.log_with_msg(
                            "Convert create changeset parameters to bonsai changes",
                            None,
                        );
                        result
                    }),
            )
            .boxed()
            .buffered(10)
            .try_collect::<Vec<_>>()
            .await
        };

        let ((), (), (), file_changes_stack) = try_join!(
            verify_deleted_files_existed_fut,
            verify_prefix_files_deleted_fut,
            verify_no_merge_conflicts_fut,
            resolve_file_changes_fut,
        )?;

        let mut new_changesets = Vec::new();
        let mut new_changeset_ids = Vec::new();
        let mut parents = stack_parents;
        for (info, file_changes) in info_stack.into_iter().zip(file_changes_stack.into_iter()) {
            let author_date = MononokeDateTime::new(info.author_date);
            let committer_date = info.committer_date.map(MononokeDateTime::new);
            let hg_extra = info.extra.into();
            let git_extra_headers = info.git_extra_headers.map(SortedVectorMap::from);
            let file_changes = file_changes
                .into_iter()
                .map(|(path, change)| Ok((path, change.into_file_change(&parents)?)))
                .collect::<Result<SortedVectorMap<NonRootMPath, FileChange>, MononokeError>>()?;

            // Create the new Bonsai Changeset. The `freeze` method validates
            // that the bonsai changeset is internally consistent.

            let new_changeset = BonsaiChangesetMut {
                parents,
                author: info.author,
                author_date,
                committer: info.committer,
                committer_date,
                message: info.message,
                hg_extra,
                git_extra_headers,
                git_tree_hash: None,
                file_changes,
                is_snapshot: bubble.is_some(),
                git_annotated_tag: None,
            }
            .freeze()
            .map_err(|e| {
                MononokeError::InvalidRequest(format!(
                    "Changes create invalid bonsai changeset: {}",
                    e
                ))
            })?;

            let new_changeset_id = new_changeset.get_changeset_id();
            parents = vec![new_changeset_id];
            new_changesets.push(new_changeset);
            new_changeset_ids.push(new_changeset_id);
        }

        if let Some(bubble) = &bubble {
            self.save_changesets(
                new_changesets,
                &bubble.repo_view(self.inner_repo()),
                Some(bubble),
            )
            .await?;
        } else {
            self.save_changesets(new_changesets, self.inner_repo(), None)
                .await?;
        }

        Ok(new_changeset_ids
            .into_iter()
            .map(|new_changeset_id| ChangesetContext::new(self.clone(), new_changeset_id))
            .collect())
    }
}
