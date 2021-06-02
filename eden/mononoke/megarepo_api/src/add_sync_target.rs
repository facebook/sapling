/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::common::{find_bookmark_and_value, MegarepoOp, SourceName};
use anyhow::{anyhow, Error};
use blobrepo::{save_bonsai_changesets, BlobRepo};
use bookmarks::{BookmarkName, BookmarkUpdateReason};
use bytes::Bytes;
use commit_transformation::{create_source_to_target_multi_mover, MultiMover};
use context::CoreContext;
use derived_data::BonsaiDerived;
use fsnodes::RootFsnodeId;
use futures::{stream, StreamExt, TryStreamExt};
use manifest::ManifestOps;
use megarepo_config::{MononokeMegarepoConfigs, Source, SourceRevision, SyncTargetConfig};
use megarepo_error::MegarepoError;
use megarepo_mapping::CommitRemappingState;
use mononoke_api::Mononoke;
use mononoke_api::RepoContext;
use mononoke_types::{
    BonsaiChangeset, BonsaiChangesetMut, ChangesetId, DateTime, FileChange, FileType, MPath,
};
use reachabilityindex::LeastCommonAncestorsHint;
use sorted_vector_map::SortedVectorMap;
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

// Create a new sync target given a config.
// After this command finishes it creates
// move commits on top of source commits
// and also merges them all together.
//
//      Tn
//      | \
//     ...
//      |
//      T1
//     / \
//    M   M
//   /     \
//  S       S
//
// Tx - target merge commits
// M - move commits
// S - source commits that need to be merged
pub struct AddSyncTarget<'a> {
    pub megarepo_configs: &'a Arc<dyn MononokeMegarepoConfigs>,
    pub mononoke: &'a Arc<Mononoke>,
}

impl<'a> MegarepoOp for AddSyncTarget<'a> {
    fn mononoke(&self) -> &Arc<Mononoke> {
        &self.mononoke
    }
}

impl<'a> AddSyncTarget<'a> {
    pub fn new(
        megarepo_configs: &'a Arc<dyn MononokeMegarepoConfigs>,
        mononoke: &'a Arc<Mononoke>,
    ) -> Self {
        Self {
            megarepo_configs,
            mononoke,
        }
    }

    pub async fn run(
        self,
        ctx: &CoreContext,
        sync_target_config: SyncTargetConfig,
        changesets_to_merge: HashMap<SourceName, ChangesetId>,
        message: Option<String>,
    ) -> Result<ChangesetId, MegarepoError> {
        let repo = self
            .find_repo_by_id(ctx, sync_target_config.target.repo_id)
            .await?;

        // First let's create commit on top of all source commits that
        // move all files in a correct place
        let moved_commits = self
            .create_move_commits(
                ctx,
                repo.blob_repo(),
                &sync_target_config,
                &changesets_to_merge,
            )
            .await?;

        // Now let's merge all the moved commits together
        let top_merge_cs_id = self
            .create_merge_commits(
                ctx,
                repo.blob_repo(),
                moved_commits,
                &sync_target_config,
                message,
            )
            .await?;

        self.megarepo_configs
            .add_target_with_config_version(ctx.clone(), sync_target_config.clone())
            .await?;

        self.move_bookmark(
            ctx,
            repo.blob_repo(),
            sync_target_config.target.bookmark,
            top_merge_cs_id,
        )
        .await?;

        Ok(top_merge_cs_id)
    }

    // Creates move commits on top of source changesets that we want to merge
    // into the target. These move commits put all source files into a correct place
    // in a target.
    async fn create_move_commits<'b>(
        &'b self,
        ctx: &'b CoreContext,
        repo: &'b BlobRepo,
        sync_target_config: &'b SyncTargetConfig,
        changesets_to_merge: &'b HashMap<SourceName, ChangesetId>,
    ) -> Result<Vec<(SourceName, SourceAndMovedChangesets)>, Error> {
        // Keep track of all files created in all sources so that we can check
        // if there's a conflict between
        let mut all_files_in_target = HashMap::new();
        let mut moved_commits = vec![];
        for source_config in &sync_target_config.sources {
            let source_repo = self.find_repo_by_id(ctx, source_config.repo_id).await?;

            let source_name = SourceName(source_config.source_name.clone());

            let changeset_id = self
                .validate_changeset_to_merge(ctx, &source_repo, source_config, changesets_to_merge)
                .await?;
            let mover = create_source_to_target_multi_mover(source_config.mapping.clone())
                .map_err(MegarepoError::request)?;

            let linkfiles = self.prepare_linkfiles(source_config, &mover)?;
            let linkfiles = self.upload_linkfiles(ctx, linkfiles, repo).await?;
            // TODO(stash): it assumes that commit is present in target
            let moved = self
                .create_single_move_commit(ctx, repo, changeset_id, &mover, linkfiles)
                .await?;
            add_and_check_all_paths(
                &mut all_files_in_target,
                &source_name,
                moved
                    .moved
                    .file_changes()
                    // Do not check deleted files
                    .filter_map(|(path, maybe_fc)| maybe_fc.map(|_| path)),
            )?;
            moved_commits.push((source_name, moved));
        }

        Ok(moved_commits)
    }

    async fn validate_changeset_to_merge(
        &self,
        ctx: &CoreContext,
        source_repo: &RepoContext,
        source_config: &Source,
        changesets_to_merge: &HashMap<SourceName, ChangesetId>,
    ) -> Result<ChangesetId, MegarepoError> {
        let changeset_id = changesets_to_merge
            .get(&SourceName(source_config.name.clone()))
            .ok_or_else(|| {
                MegarepoError::request(anyhow!(
                    "Not found changeset to merge for {}",
                    source_config.name
                ))
            })?;


        match &source_config.revision {
            SourceRevision::hash(expected_changeset_id) => {
                let expected_changeset_id = ChangesetId::from_bytes(expected_changeset_id)
                    .map_err(MegarepoError::request)?;
                if &expected_changeset_id != changeset_id {
                    return Err(MegarepoError::request(anyhow!(
                        "unexpected source revision for {}: expected {}, found {}",
                        source_config.source_name,
                        expected_changeset_id,
                        changeset_id,
                    )));
                }
            }
            SourceRevision::bookmark(bookmark) => {
                let (_, source_bookmark_value) =
                    find_bookmark_and_value(ctx, source_repo, &bookmark).await?;

                if &source_bookmark_value != changeset_id {
                    let is_ancestor = source_repo
                        .skiplist_index()
                        .is_ancestor(
                            ctx,
                            &source_repo.blob_repo().get_changeset_fetcher(),
                            *changeset_id,
                            source_bookmark_value,
                        )
                        .await
                        .map_err(MegarepoError::internal)?;

                    if !is_ancestor {
                        return Err(MegarepoError::request(anyhow!(
                            "{} is not an ancestor of source bookmark {}",
                            changeset_id,
                            bookmark,
                        )));
                    }
                }
            }
            SourceRevision::UnknownField(_) => {
                return Err(MegarepoError::internal(anyhow!(
                    "unexpected source revision!"
                )));
            }
        };

        Ok(*changeset_id)
    }

    async fn create_merge_commits(
        &self,
        ctx: &CoreContext,
        repo: &BlobRepo,
        moved_commits: Vec<(SourceName, SourceAndMovedChangesets)>,
        sync_target_config: &SyncTargetConfig,
        message: Option<String>,
    ) -> Result<ChangesetId, Error> {
        // Now let's create a merge commit that merges all moved changesets

        // We need to create a file with the latest commits that were synced from
        // sources to target repo. Note that we are writing non-moved commits to the
        // state file, since state file the latest synced commit
        let state = CommitRemappingState::new(
            moved_commits
                .iter()
                .map(|(source, css)| (source.0.clone(), css.source))
                .collect(),
            sync_target_config.version.clone(),
        );

        // TODO(stash): avoid doing a single merge commit, and do a stack of merges instead
        let mut bcs = BonsaiChangesetMut {
            parents: moved_commits
                .into_iter()
                .map(|(_, css)| css.moved.get_changeset_id())
                .collect(),
            author: "svcscm".to_string(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message: message.unwrap_or(format!(
                "Add new sync target with version {}",
                sync_target_config.version
            )),
            extra: SortedVectorMap::new(),
            file_changes: SortedVectorMap::new(),
        };
        state.save_in_changeset(ctx, repo, &mut bcs).await?;
        let bcs = bcs.freeze()?;
        save_bonsai_changesets(vec![bcs.clone()], ctx.clone(), repo.clone()).await?;

        Ok(bcs.get_changeset_id())
    }

    async fn create_single_move_commit<'b>(
        &'b self,
        ctx: &'b CoreContext,
        repo: &'b BlobRepo,
        cs_id: ChangesetId,
        mover: &MultiMover,
        linkfiles: BTreeMap<MPath, Option<FileChange>>,
    ) -> Result<SourceAndMovedChangesets, MegarepoError> {
        let root_fsnode_id = RootFsnodeId::derive(ctx, repo, cs_id)
            .await
            .map_err(Error::from)?;
        let fsnode_id = root_fsnode_id.fsnode_id();
        let entries = fsnode_id
            .list_leaf_entries(ctx.clone(), repo.get_blobstore())
            .try_collect::<Vec<_>>()
            .await?;

        let mut file_changes = vec![];
        for (path, fsnode) in entries {
            let moved = mover(&path)?;

            // Check that path doesn't move to itself - in that case we don't need to
            // delete file
            if moved.iter().find(|cur_path| cur_path == &&path).is_none() {
                file_changes.push((path.clone(), None));
            }

            file_changes.extend(moved.into_iter().map(|target| {
                let fc = FileChange::new(
                    *fsnode.content_id(),
                    *fsnode.file_type(),
                    fsnode.size(),
                    Some((path.clone(), cs_id)),
                );

                (target, Some(fc))
            }));
        }
        file_changes.extend(linkfiles.into_iter());

        // TODO(stash): we need to figure out what parameters to set here
        let moved_bcs = BonsaiChangesetMut {
            parents: vec![cs_id],
            author: "svcscm".to_string(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message: "move commit".to_string(),
            extra: SortedVectorMap::new(),
            file_changes: file_changes.into_iter().collect(),
        }
        .freeze()?;

        save_bonsai_changesets(vec![moved_bcs.clone()], ctx.clone(), repo.clone()).await?;

        let source_and_moved_changeset = SourceAndMovedChangesets {
            source: cs_id,
            moved: moved_bcs,
        };
        Ok(source_and_moved_changeset)
    }

    fn prepare_linkfiles(
        &self,
        source_config: &Source,
        mover: &MultiMover,
    ) -> Result<BTreeMap<MPath, Bytes>, MegarepoError> {
        let mut links = BTreeMap::new();
        for (src, dst) in &source_config.mapping.linkfiles {
            // src is a file inside a given source, so mover needs to be applied to it
            let src = MPath::new(src).map_err(MegarepoError::request)?;
            let dst = MPath::new(dst).map_err(MegarepoError::request)?;
            let moved_srcs = mover(&src).map_err(MegarepoError::request)?;

            let mut iter = moved_srcs.into_iter();
            let moved_src = match (iter.next(), iter.next()) {
                (Some(moved_src), None) => moved_src,
                (None, None) => {
                    return Err(MegarepoError::request(anyhow!(
                        "linkfile source {} does not map to any file inside source {}",
                        src,
                        source_config.name
                    )));
                }
                _ => {
                    return Err(MegarepoError::request(anyhow!(
                        "linkfile source {} maps to too many files inside source {}",
                        src,
                        source_config.name
                    )));
                }
            };

            let content = Bytes::from(moved_src.to_vec());
            links.insert(dst, content);
        }
        Ok(links)
    }

    async fn upload_linkfiles(
        &self,
        ctx: &CoreContext,
        links: BTreeMap<MPath, Bytes>,
        repo: &BlobRepo,
    ) -> Result<BTreeMap<MPath, Option<FileChange>>, Error> {
        let linkfiles = stream::iter(links.into_iter())
            .map(Ok)
            .map_ok(|(path, content)| async {
                let ((content_id, size), fut) = filestore::store_bytes(
                    repo.blobstore(),
                    repo.filestore_config(),
                    &ctx,
                    content,
                );
                fut.await?;

                let fc = Some(FileChange::new(content_id, FileType::Symlink, size, None));

                Result::<_, Error>::Ok((path, fc))
            })
            .try_buffer_unordered(100)
            .try_collect::<BTreeMap<_, _>>()
            .await?;
        Ok(linkfiles)
    }

    async fn move_bookmark(
        &self,
        ctx: &CoreContext,
        repo: &BlobRepo,
        bookmark: String,
        cs_id: ChangesetId,
    ) -> Result<(), MegarepoError> {
        let mut txn = repo.bookmarks().create_transaction(ctx.clone());
        let bookmark = BookmarkName::new(bookmark).map_err(MegarepoError::request)?;
        let maybe_book_value = repo.bookmarks().get(ctx.clone(), &bookmark).await?;

        match maybe_book_value {
            Some(old) => {
                txn.update(&bookmark, cs_id, old, BookmarkUpdateReason::XRepoSync, None)?;
            }
            None => {
                txn.create(&bookmark, cs_id, BookmarkUpdateReason::XRepoSync, None)?;
            }
        }

        let success = txn.commit().await.map_err(MegarepoError::internal)?;
        if !success {
            return Err(MegarepoError::internal(anyhow!(
                "failed to move a bookmark, possibly because of race condition"
            )));
        }
        Ok(())
    }
}

struct SourceAndMovedChangesets {
    source: ChangesetId,
    moved: BonsaiChangeset,
}

// Verifies that no two sources create the same path in the target
fn add_and_check_all_paths<'a>(
    all_files_in_target: &'a mut HashMap<MPath, SourceName>,
    source_name: &'a SourceName,
    iter: impl Iterator<Item = &'a MPath>,
) -> Result<(), MegarepoError> {
    for path in iter {
        add_and_check(all_files_in_target, source_name, path)?;
    }

    Ok(())
}

fn add_and_check<'a>(
    all_files_in_target: &'a mut HashMap<MPath, SourceName>,
    source_name: &'a SourceName,
    path: &MPath,
) -> Result<(), MegarepoError> {
    let existing_source = all_files_in_target.insert(path.clone(), source_name.clone());
    if let Some(existing_source) = existing_source {
        let err = MegarepoError::request(anyhow!(
            "File {} is remapped from two different sources: {} and {}",
            path,
            source_name.0,
            existing_source.0,
        ));

        return Err(err);
    }

    Ok(())
}
