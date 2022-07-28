/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashSet;

use blobstore::Loadable;
use bytes::Bytes;
use changesets::ChangesetsRef;
use chrono::DateTime;
use chrono::FixedOffset;
use context::CoreContext;
use ephemeral_blobstore::Bubble;
use filestore::FetchKey;
use filestore::FilestoreConfig;
use filestore::StoreRequest;
use futures::stream;
use futures::stream::FuturesOrdered;
use futures::stream::FuturesUnordered;
use futures::stream::Stream;
use futures::stream::TryStreamExt;
use futures::try_join;
use futures::StreamExt;
use futures_stats::TimedFutureExt;
use manifest::PathTree;
use mononoke_types::fsnode::FsnodeEntry;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime as MononokeDateTime;
use mononoke_types::FileChange;
use mononoke_types::MPath;
use mononoke_types::MPathElement;
use repo_authorization::RepoWriteOperation;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_identity::RepoIdentityRef;
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

    async fn resolve(
        self,
        parents: &[ChangesetContext],
    ) -> Result<(MPath, ChangesetId), MononokeError> {
        let parent_ctx = parents.get(self.parent_index).ok_or_else(|| {
            MononokeError::InvalidRequest(format!(
                "Parent index '{}' out of range for commit with {} parent(s)",
                self.parent_index,
                parents.len()
            ))
        })?;
        if !parent_ctx
            .path_with_content(self.path.clone())?
            .is_file()
            .await?
        {
            return Err(MononokeError::InvalidRequest(String::from(
                "Copy-from path must reference a file",
            )));
        }
        let mpath = self.path.into_mpath().ok_or_else(|| {
            MononokeError::InvalidRequest(String::from("Copy-from path cannot be the root"))
        })?;
        Ok((mpath, parent_ctx.id()))
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
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum CreateChangeType {
    None,
    Change,
    Deletion,
}

impl Default for CreateChangeType {
    fn default() -> Self {
        CreateChangeType::None
    }
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
    pub async fn resolve(
        self,
        ctx: &CoreContext,
        filestore_config: FilestoreConfig,
        repo_blobstore: RepoBlobstore,
        parents: &[ChangesetContext],
    ) -> Result<FileChange, MononokeError> {
        let (file, copy_info, tracked) = match self {
            CreateChange::Tracked(file, copy_info) => (
                file,
                match copy_info {
                    Some(copy_info) => Some(copy_info.resolve(parents).await?),
                    None => None,
                },
                true,
            ),
            CreateChange::Untracked(file) => (file, None, false),
            CreateChange::UntrackedDeletion => return Ok(FileChange::UntrackedDeletion),
            CreateChange::Deletion => return Ok(FileChange::Deletion),
        };
        let (file_id, file_type, size) = match file {
            CreateChangeFile::New { bytes, file_type } => {
                let meta = filestore::store(
                    &repo_blobstore,
                    filestore_config,
                    ctx,
                    &StoreRequest::new(bytes.len() as u64),
                    stream::once(async move { Ok(bytes) }),
                )
                .await?;
                (meta.content_id, file_type, meta.total_size)
            }
            CreateChangeFile::Existing {
                file_id,
                file_type,
                maybe_size,
            } => (
                file_id,
                file_type,
                match maybe_size {
                    Some(size) => size,
                    None => {
                        filestore::get_metadata(&repo_blobstore, ctx, &FetchKey::Canonical(file_id))
                            .await?
                            .ok_or_else(|| {
                                MononokeError::InvalidRequest(format!(
                                    "File id '{}' is not available in this repo",
                                    file_id
                                ))
                            })?
                            .total_size
                    }
                },
            ),
        };
        if tracked {
            Ok(FileChange::tracked(file_id, file_type, size, copy_info))
        } else {
            Ok(FileChange::untracked(file_id, file_type, size))
        }
    }

    fn change_type(&self) -> CreateChangeType {
        match self {
            CreateChange::Deletion | CreateChange::UntrackedDeletion => CreateChangeType::Deletion,
            CreateChange::Tracked(..) | CreateChange::Untracked(..) => CreateChangeType::Change,
        }
    }
}

/// Verify that all deleted files existed in at least one of the parents.
async fn verify_deleted_files_existed_in_a_parent(
    parent_ctxs: &[ChangesetContext],
    deleted_files: BTreeSet<MononokePath>,
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

    // Filter the deleted files to those that existed in a parent.
    let parent_files: BTreeSet<_> = parent_ctxs
        .iter()
        .map(|parent_ctx| get_matching_files(parent_ctx, &deleted_files))
        .collect::<FuturesUnordered<_>>()
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
        .any(|prefix| paths.get(prefix.as_mpath()) == Some(&CreateChangeType::Change))
}

/// Verify that any files in `prefix_paths` that exist in `parent_ctx` have
/// been marked as deleted in `path_changes`.
async fn verify_prefix_files_deleted(
    parent_ctx: &ChangesetContext,
    prefix_paths: &BTreeSet<MononokePath>,
    path_changes: &PathTree<CreateChangeType>,
) -> Result<(), MononokeError> {
    parent_ctx
        .paths(prefix_paths.iter().cloned())
        .await?
        .try_for_each(|prefix_path| async move {
            if prefix_path.is_file().await?
                && path_changes.get(prefix_path.path().as_mpath())
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

    let root_fsnodes: Vec<_> = {
        let futs: FuturesUnordered<_> = changesets
            .iter()
            .map(|cs_ctx| cs_ctx.root_fsnode_id())
            .collect();
        futs.map_ok(|root| root.into_fsnode_id())
            .try_collect()
            .await?
    };

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
                            .get(path.as_mpath())
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
                            .get(path.as_mpath())
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
    async fn save_changeset(
        &self,
        changeset: BonsaiChangeset,
        container: &(impl ChangesetsRef + RepoBlobstoreRef + RepoIdentityRef),
        bubble: Option<&Bubble>,
    ) -> Result<(), MononokeError> {
        blobrepo::save_bonsai_changesets(vec![changeset.clone()], self.ctx().clone(), container)
            .await?;

        if let Some(category) = self.config().infinitepush.commit_scribe_category.as_deref() {
            blobrepo::scribe::log_commit_to_scribe(
                self.ctx(),
                category,
                container,
                &changeset,
                bubble.map(|x| x.bubble_id()),
            )
            .await;
        }

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
        author: String,
        author_date: DateTime<FixedOffset>,
        committer: Option<String>,
        committer_date: Option<DateTime<FixedOffset>>,
        message: String,
        extra: BTreeMap<String, Vec<u8>>,
        changes: BTreeMap<MononokePath, CreateChange>,
        // If some, this changeset is a snapshot. Currently unsupported to upload a
        // normal commit to a bubble, though can be easily added.
        bubble: Option<&Bubble>,
    ) -> Result<ChangesetContext, MononokeError> {
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
        if !allowed_no_parents && parents.is_empty() {
            return Err(MononokeError::InvalidRequest(String::from(
                "Changesets with no parents cannot be created",
            )));
        }

        // Obtain contexts for each of the parents (which should exist).
        let parent_ctxs: Vec<_> = parents
            .iter()
            .map(|parent_id| async move {
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
            })
            .collect::<FuturesOrdered<_>>()
            .try_collect()
            .await?;

        // Check that changes are valid according to bonsai rules:
        // (1) deletions and copy-from info must reference a real path in a
        //     valid parent.
        // (2) deletions for paths where a prefix directory has been replaced
        //     by a file should be dropped, as the deletion is implicit from the
        //     file change for the prefix path.
        // (3) conversely, when a file has been replaced by a directory, there
        //     must be a delete for the file.
        //

        // Extract the set of deleted files.
        let tracked_deletion_files: BTreeSet<_> = changes
            .iter()
            .filter(|(_path, change)| matches!(change, CreateChange::Deletion))
            .map(|(path, _change)| path.clone())
            .collect();

        // Check deleted files existed in a parent. (1)
        let fut_verify_deleted_files_existed = async {
            // This does NOT consider "missing" (untracked deletion) files as it is NOT
            // necessary for them to exist in a parent. If they don't exist on a parent,
            // this means the file was "hg added" and then manually deleted.
            let (stats, result) =
                verify_deleted_files_existed_in_a_parent(&parent_ctxs, tracked_deletion_files)
                    .timed()
                    .await;
            let mut scuba = self.ctx().scuba().clone();
            scuba.add_future_stats(&stats);
            scuba.log_with_msg("Verify deleted files existed in a parent", None);
            result
        };

        // Build a path tree recording each path that has been created or deleted.
        let path_changes = PathTree::from_iter(
            changes
                .iter()
                .map(|(path, change)| (path.as_mpath().cloned(), change.change_type())),
        );

        // Determine the prefixes of all changed files.
        let prefix_paths: BTreeSet<_> = changes
            .iter()
            .filter(|(_path, change)| change.change_type() == CreateChangeType::Change)
            .flat_map(|(path, _change)| path.clone().prefixes())
            .collect();

        // Check changes that replace a file with a directory also delete
        // this replaced file. (3)
        let fut_verify_prefix_files_deleted = async {
            let (stats, result) = parent_ctxs
                .iter()
                .map(|parent_ctx| {
                    verify_prefix_files_deleted(parent_ctx, &prefix_paths, &path_changes)
                })
                .collect::<FuturesUnordered<_>>()
                .try_for_each(|_| async { Ok(()) })
                .timed()
                .await;
            let mut scuba = self.ctx().scuba().clone();
            scuba.add_future_stats(&stats);
            scuba.log_with_msg("Verify prefix files in parents have been deleted", None);
            result
        };

        // Check for merge conflicts
        let merge_conflicts_fut = async {
            let (stats, result) = check_addless_union_conflicts(
                self.ctx(),
                match &bubble {
                    Some(bubble) => {
                        bubble.wrap_repo_blobstore(self.blob_repo().blobstore().clone())
                    }
                    None => self.blob_repo().blobstore().clone(),
                },
                parent_ctxs.as_slice(),
                &path_changes,
            )
            .timed()
            .await;

            let mut scuba = self.ctx().scuba().clone();
            scuba.add_future_stats(&stats);
            scuba.log_with_msg("Verify all merge conflicts are resolved", None);
            result
        };

        // Convert change paths into the form needed for the bonsai changeset.
        let changes: Vec<(MPath, CreateChange)> = changes
            .into_iter()
            // Filter deletions that have a change at a path prefix. The
            // deletion is implicit from the change. (2)
            .filter(|(path, change)| {
                change.change_type() != CreateChangeType::Deletion
                    || !is_prefix_changed(path, &path_changes)
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
            .collect::<Result<_, _>>()?;

        // Resolve the changes into bonsai changes. This also checks (1) for
        // copy-from info.
        let file_changes_fut = async {
            let (stats, result) = changes
                .into_iter()
                .map(|(path, change)| {
                    let parent_ctxs = &parent_ctxs;
                    async move {
                        let change = change
                            .resolve(
                                self.ctx(),
                                self.blob_repo().filestore_config(),
                                match &bubble {
                                    Some(bubble) => bubble
                                        .wrap_repo_blobstore(self.blob_repo().blobstore().clone()),
                                    None => self.blob_repo().blobstore().clone(),
                                },
                                parent_ctxs,
                            )
                            .await?;
                        Ok::<_, MononokeError>((path, change))
                    }
                })
                .collect::<FuturesUnordered<_>>()
                .try_collect::<SortedVectorMap<MPath, FileChange>>()
                .timed()
                .await;
            let mut scuba = self.ctx().scuba().clone();
            scuba.add_future_stats(&stats);
            scuba.log_with_msg(
                "Convert create changeset parameters to bonsai changes",
                None,
            );
            result
        };

        let ((), (), (), file_changes) = try_join!(
            fut_verify_deleted_files_existed,
            fut_verify_prefix_files_deleted,
            merge_conflicts_fut,
            file_changes_fut,
        )?;

        let author_date = MononokeDateTime::new(author_date);
        let committer_date = committer_date.map(MononokeDateTime::new);
        let extra = extra.into();

        // Create the new Bonsai Changeset. The `freeze` method validates
        // that the bonsai changeset is internally consistent.
        let new_changeset = BonsaiChangesetMut {
            parents,
            author,
            author_date,
            committer,
            committer_date,
            message,
            extra,
            file_changes,
            is_snapshot: bubble.is_some(),
        }
        .freeze()
        .map_err(|e| {
            MononokeError::InvalidRequest(format!("Changes create invalid bonsai changeset: {}", e))
        })?;

        let new_changeset_id = new_changeset.get_changeset_id();

        if let Some(bubble) = &bubble {
            self.save_changeset(
                new_changeset,
                &bubble.repo_view(self.blob_repo()),
                Some(bubble),
            )
            .await?;
        } else {
            self.save_changeset(new_changeset, self.blob_repo(), None)
                .await?;
        }

        Ok(ChangesetContext::new(self.clone(), new_changeset_id))
    }
}
