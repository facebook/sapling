/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Context, Error};
use async_trait::async_trait;
use blobrepo::{save_bonsai_changesets, BlobRepo};
use bookmarks::{BookmarkName, BookmarkUpdateReason};
use bytes::Bytes;
use commit_transformation::{create_source_to_target_multi_mover, MultiMover};
use context::CoreContext;
use derived_data::BonsaiDerived;
use fsnodes::RootFsnodeId;
use futures::{stream, StreamExt, TryFutureExt, TryStreamExt};
use itertools::{EitherOrBoth, Itertools};
use manifest::ManifestOps;
use megarepo_config::{
    MononokeMegarepoConfigs, Source, SourceRevision, SyncConfigVersion, SyncTargetConfig, Target,
};
use megarepo_error::MegarepoError;
use megarepo_mapping::{CommitRemappingState, SourceName};
use mononoke_api::{Mononoke, RepoContext};
use mononoke_types::{
    BonsaiChangeset, BonsaiChangesetMut, ChangesetId, DateTime, FileChange, FileType, MPath,
    RepositoryId,
};
use reachabilityindex::LeastCommonAncestorsHint;
use sorted_vector_map::SortedVectorMap;
use std::collections::{BTreeMap, HashMap};
use std::{convert::TryInto, sync::Arc};

pub struct SourceAndMovedChangesets {
    pub source: ChangesetId,
    pub moved: BonsaiChangeset,
}

#[async_trait]
pub trait MegarepoOp {
    fn mononoke(&self) -> &Arc<Mononoke>;

    async fn find_repo_by_id(
        &self,
        ctx: &CoreContext,
        repo_id: i64,
    ) -> Result<RepoContext, MegarepoError> {
        let target_repo_id = RepositoryId::new(repo_id.try_into().unwrap());
        let target_repo = self
            .mononoke()
            .repo_by_id_bypass_acl_check(ctx.clone(), target_repo_id)
            .await
            .map_err(MegarepoError::internal)?
            .ok_or_else(|| MegarepoError::request(anyhow!("repo not found {}", target_repo_id)))?;
        Ok(target_repo)
    }

    async fn create_single_move_commit(
        &self,
        ctx: &CoreContext,
        repo: &BlobRepo,
        cs_id: ChangesetId,
        mover: &MultiMover,
        linkfiles: BTreeMap<MPath, FileChange>,
        source_name: &SourceName,
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
                file_changes.push((path.clone(), FileChange::Deletion));
            }

            file_changes.extend(moved.into_iter().map(|target| {
                let fc = FileChange::tracked(
                    *fsnode.content_id(),
                    *fsnode.file_type(),
                    fsnode.size(),
                    Some((path.clone(), cs_id)),
                );

                (target, fc)
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
            message: format!("move commit for source {}", source_name.0),
            extra: SortedVectorMap::new(),
            file_changes: file_changes.into_iter().collect(),
            is_snapshot: false,
        }
        .freeze()?;

        let source_and_moved_changeset = SourceAndMovedChangesets {
            source: cs_id,
            moved: moved_bcs,
        };
        Ok(source_and_moved_changeset)
    }

    // Creates move commits on top of source changesets that we want to merge
    // into the target. These move commits put all source files into a correct place
    // in a target.
    async fn create_move_commits<'b>(
        &'b self,
        ctx: &'b CoreContext,
        repo: &'b BlobRepo,
        sources: &[Source],
        changesets_to_merge: &'b BTreeMap<SourceName, ChangesetId>,
    ) -> Result<Vec<(SourceName, SourceAndMovedChangesets)>, Error> {
        let moved_commits = stream::iter(sources.iter().cloned().map(Ok))
            .map_ok(|source_config| {
                async move {
                    let source_repo = self.find_repo_by_id(ctx, source_config.repo_id).await?;

                    let source_name = SourceName(source_config.source_name.clone());

                    let changeset_id = self
                        .validate_changeset_to_merge(
                            ctx,
                            &source_repo,
                            &source_config,
                            changesets_to_merge,
                        )
                        .await?;
                    let mover = create_source_to_target_multi_mover(source_config.mapping.clone())
                        .map_err(MegarepoError::request)?;

                    let linkfiles = self.prepare_linkfiles(&source_config, &mover)?;
                    let linkfiles = self.upload_linkfiles(ctx, linkfiles, repo).await?;
                    // TODO(stash): it assumes that commit is present in target
                    let moved = self
                        .create_single_move_commit(
                            ctx,
                            repo,
                            changeset_id,
                            &mover,
                            linkfiles,
                            &source_name,
                        )
                        .await?;

                    Result::<(SourceName, SourceAndMovedChangesets), Error>::Ok((
                        source_name,
                        moved,
                    ))
                }
            })
            .try_buffer_unordered(10)
            .try_collect::<Vec<_>>()
            .await?;

        // Keep track of all files created in all sources so that we can check
        // if there's a conflict between
        let mut all_files_in_target = HashMap::new();
        for (source_name, moved) in &moved_commits {
            add_and_check_all_paths(
                &mut all_files_in_target,
                &source_name,
                moved
                    .moved
                    .file_changes()
                    // Do not check deleted files
                    .filter_map(|(path, fc)| fc.is_changed().then(|| path)),
            )?;
        }

        save_bonsai_changesets(
            moved_commits
                .iter()
                .map(|(_, css)| css.moved.clone())
                .collect(),
            ctx.clone(),
            repo.clone(),
        )
        .await?;
        Ok(moved_commits)
    }


    async fn validate_changeset_to_merge(
        &self,
        ctx: &CoreContext,
        source_repo: &RepoContext,
        source_config: &Source,
        changesets_to_merge: &BTreeMap<SourceName, ChangesetId>,
    ) -> Result<ChangesetId, MegarepoError> {
        let changeset_id = changesets_to_merge
            .get(&SourceName(source_config.source_name.clone()))
            .ok_or_else(|| {
                MegarepoError::request(anyhow!(
                    "Not found changeset to merge for {}",
                    source_config.source_name
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

    fn prepare_linkfiles(
        &self,
        source_config: &Source,
        mover: &MultiMover,
    ) -> Result<BTreeMap<MPath, Bytes>, MegarepoError> {
        let mut links = BTreeMap::new();
        for (dst, src) in &source_config.mapping.linkfiles {
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

            let content = create_relative_symlink(&moved_src, &dst)?;
            links.insert(dst, content);
        }
        Ok(links)
    }


    async fn upload_linkfiles(
        &self,
        ctx: &CoreContext,
        links: BTreeMap<MPath, Bytes>,
        repo: &BlobRepo,
    ) -> Result<BTreeMap<MPath, FileChange>, Error> {
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

                let fc = FileChange::tracked(content_id, FileType::Symlink, size, None);

                Result::<_, Error>::Ok((path, fc))
            })
            .try_buffer_unordered(100)
            .try_collect::<BTreeMap<_, _>>()
            .await?;
        Ok(linkfiles)
    }

    // Merge moved commits from a lot of sources together
    // Instead of creating a single merge commits with lots of parents
    // we create a stack of merge commits (the primary reason for that is
    // that mercurial doesn't support more than 2 parents)
    //
    //      Merge_n
    //    /         \
    //  Merge_n-1   Move_n
    //    |    \
    //    |      Move_n-1
    //  Merge_n-2
    //    |    \
    //          Move_n-2
    //
    // write_commit_remapping_state controls whether the top merge commit
    // should contain the commit remapping state file.
    async fn create_merge_commits(
        &self,
        ctx: &CoreContext,
        repo: &BlobRepo,
        moved_commits: Vec<(SourceName, SourceAndMovedChangesets)>,
        write_commit_remapping_state: bool,
        sync_config_version: SyncConfigVersion,
        message: Option<String>,
    ) -> Result<ChangesetId, MegarepoError> {
        // Now let's create a merge commit that merges all moved changesets

        // We need to create a file with the latest commits that were synced from
        // sources to target repo. Note that we are writing non-moved commits to the
        // state file, since state file the latest synced commit
        let state = if write_commit_remapping_state {
            Some(CommitRemappingState::new(
                moved_commits
                    .iter()
                    .map(|(source, css)| (source.clone(), css.source))
                    .collect(),
                sync_config_version.clone(),
            ))
        } else {
            None
        };

        let (last_moved_commit, first_moved_commits) = match moved_commits.split_last() {
            Some((last_moved_commit, first_moved_commits)) => {
                (last_moved_commit, first_moved_commits)
            }
            None => {
                return Err(MegarepoError::request(anyhow!(
                    "no move commits were set - target has no sources?"
                )));
            }
        };

        let mut merges = vec![];
        let mut cur_parents = vec![];
        for (source_name, css) in first_moved_commits {
            cur_parents.push(css.moved.get_changeset_id());
            if cur_parents.len() > 1 {
                let bcs = self.create_merge_commit(
                    message.clone(),
                    cur_parents,
                    sync_config_version.clone(),
                    &source_name,
                )?;
                let merge = bcs.freeze()?;
                cur_parents = vec![merge.get_changeset_id()];
                merges.push(merge);
            }
        }

        let (last_source_name, last_moved_commit) = last_moved_commit;
        cur_parents.push(last_moved_commit.moved.get_changeset_id());
        let mut final_merge =
            self.create_merge_commit(message, cur_parents, sync_config_version, &last_source_name)?;
        if let Some(state) = state {
            state.save_in_changeset(ctx, repo, &mut final_merge).await?;
        }
        let final_merge = final_merge.freeze()?;
        merges.push(final_merge.clone());
        save_bonsai_changesets(merges, ctx.clone(), repo.clone()).await?;

        Ok(final_merge.get_changeset_id())
    }

    fn create_merge_commit(
        &self,
        message: Option<String>,
        parents: Vec<ChangesetId>,
        version: SyncConfigVersion,
        source_name: &SourceName,
    ) -> Result<BonsaiChangesetMut, Error> {
        // TODO(stash, mateusz, simonfar): figure out what fields
        // we need to set here
        let message = message.unwrap_or(format!(
            "Merging source {} for target version {}",
            source_name.0, version
        ));
        let bcs = BonsaiChangesetMut {
            parents,
            author: "svcscm".to_string(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message,
            extra: SortedVectorMap::new(),
            file_changes: SortedVectorMap::new(),
            is_snapshot: false,
        };

        Ok(bcs)
    }

    async fn create_bookmark(
        &self,
        ctx: &CoreContext,
        repo: &BlobRepo,
        bookmark: String,
        cs_id: ChangesetId,
    ) -> Result<(), MegarepoError> {
        let mut txn = repo.bookmarks().create_transaction(ctx.clone());
        let bookmark = BookmarkName::new(bookmark).map_err(MegarepoError::request)?;

        txn.create(&bookmark, cs_id, BookmarkUpdateReason::XRepoSync, None)?;

        let success = txn.commit().await.map_err(MegarepoError::internal)?;
        if !success {
            return Err(MegarepoError::internal(anyhow!(
                "failed to create a bookmark, possibly because of race condition"
            )));
        }
        Ok(())
    }

    async fn move_bookmark_conditionally(
        &self,
        ctx: &CoreContext,
        repo: &BlobRepo,
        bookmark: String,
        (from_cs_id, to_cs_id): (ChangesetId, ChangesetId),
    ) -> Result<(), MegarepoError> {
        let mut txn = repo.bookmarks().create_transaction(ctx.clone());
        let bookmark = BookmarkName::new(bookmark).map_err(MegarepoError::request)?;
        txn.update(
            &bookmark,
            to_cs_id,
            from_cs_id,
            BookmarkUpdateReason::XRepoSync,
            None,
        )?;

        let success = txn.commit().await.map_err(MegarepoError::internal)?;
        if !success {
            return Err(MegarepoError::internal(anyhow!(
                "failed to move a bookmark, possibly because of race condition"
            )));
        }
        Ok(())
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


    async fn check_if_new_sync_target_config_is_equivalent_to_already_existing(
        &self,
        ctx: &CoreContext,
        megarepo_configs: &Arc<dyn MononokeMegarepoConfigs>,
        sync_target_config: &SyncTargetConfig,
    ) -> Result<(), MegarepoError> {
        let existing_config = megarepo_configs
            .get_config_by_version(
                ctx.clone(),
                sync_target_config.target.clone(),
                sync_target_config.version.clone(),
            )
            .with_context(|| {
                format!(
                    "while checking existence of {} config",
                    sync_target_config.version
                )
            })
            .map_err(MegarepoError::request)?;

        if &existing_config != sync_target_config {
            return Err(MegarepoError::request(anyhow!(
                "config with version {} is stored, but it's different from the one sent in request parameters",
                sync_target_config.version,
            )));
        }

        Ok(())
    }

    async fn check_if_commit_has_expected_remapping_state(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
        version: &SyncConfigVersion,
        changesets_to_merge: &BTreeMap<SourceName, ChangesetId>,
        repo: &RepoContext,
    ) -> Result<Option<ChangesetId>, MegarepoError> {
        let state = self.read_remapping_state_file(ctx, repo, cs_id).await?;

        if version != state.sync_config_version() {
            return Err(MegarepoError::request(anyhow!(
                "Commit {} which has different config version: {}",
                cs_id,
                state.sync_config_version(),
            )));
        }

        let state_changesets_to_merge = state.get_all_latest_synced_changesets();
        if changesets_to_merge != state.get_all_latest_synced_changesets() {
            // // Find at least one different source commit that we can put in error message
            let mut error = None;

            let merged_iterator = changesets_to_merge
                .iter()
                .merge_join_by(state_changesets_to_merge, |i, j| i.cmp(j));

            for entry in merged_iterator {
                match entry {
                    EitherOrBoth::Left((key, value)) => {
                        error = Some(format!(
                            "{} -> {} is not present in the state file, but present in request",
                            key, value,
                        ));
                        break;
                    }
                    EitherOrBoth::Right((key, value)) => {
                        error = Some(format!(
                            "{} -> {} is present in the state file, but not present in request",
                            key, value,
                        ));
                        break;
                    }
                    EitherOrBoth::Both(request, state) => {
                        if request != state {
                            error = Some(format!(
                                "{:?} is present in request, but {:?} in state file",
                                request, state
                            ));
                            break;
                        }
                    }
                }
            }

            return Err(MegarepoError::request(anyhow!(
                "{} which was built from different source commits. Example - {}",
                cs_id,
                error.unwrap_or_else(|| "".to_string())
            )));
        }

        Ok(Some(cs_id))
    }

    async fn read_remapping_state_file(
        &self,
        ctx: &CoreContext,
        repo: &RepoContext,
        cs_id: ChangesetId,
    ) -> Result<CommitRemappingState, MegarepoError> {
        let maybe_state =
            CommitRemappingState::read_state_from_commit_opt(ctx, repo.blob_repo(), cs_id)
                .await
                .context("While reading remapping state file")
                .map_err(MegarepoError::request)?;

        maybe_state.ok_or_else(|| {
            MegarepoError::request(anyhow!("no remapping state file exist for {}", cs_id))
        })
    }
}

pub async fn find_bookmark_and_value(
    ctx: &CoreContext,
    repo: &RepoContext,
    bookmark_name: &str,
) -> Result<(BookmarkName, ChangesetId), MegarepoError> {
    let bookmark = BookmarkName::new(bookmark_name.to_string()).map_err(MegarepoError::request)?;

    let cs_id = repo
        .blob_repo()
        .bookmarks()
        .get(ctx.clone(), &bookmark)
        .map_err(MegarepoError::internal)
        .await?
        .ok_or_else(|| MegarepoError::request(anyhow!("bookmark {} not found", bookmark)))?;

    Ok((bookmark, cs_id))
}

fn create_relative_symlink(path: &MPath, base: &MPath) -> Result<Bytes, Error> {
    let common_components = path.common_components(base);
    let path_no_prefix = path.into_iter().skip(common_components).collect::<Vec<_>>();
    let base_no_prefix = base.into_iter().skip(common_components).collect::<Vec<_>>();

    if path_no_prefix.is_empty() || base_no_prefix.is_empty() {
        return Err(anyhow!(
            "Can't create symlink for {} and {}: one path is a parent of another"
        ));
    }

    let path = path_no_prefix;
    let base = base_no_prefix;
    let mut result = vec![];

    for _ in 0..(base.len() - 1) {
        result.push(b".."[..].to_vec())
    }

    for component in path.into_iter() {
        result.push(component.as_ref().to_vec());
    }

    let result: Vec<u8> = result.join(&b"/"[..]);
    Ok(Bytes::from(result))
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

pub(crate) async fn find_target_sync_config<'a>(
    ctx: &'a CoreContext,
    target_repo: &'a BlobRepo,
    target_cs_id: ChangesetId,
    target: &Target,
    megarepo_configs: &Arc<dyn MononokeMegarepoConfigs>,
) -> Result<(CommitRemappingState, SyncTargetConfig), MegarepoError> {
    let state =
        CommitRemappingState::read_state_from_commit(ctx, target_repo, target_cs_id).await?;

    // We have a target config version - let's fetch target config itself.
    let target_config = megarepo_configs.get_config_by_version(
        ctx.clone(),
        target.clone(),
        state.sync_config_version().clone(),
    )?;

    Ok((state, target_config))
}

pub(crate) async fn find_target_bookmark_and_value(
    ctx: &CoreContext,
    target_repo: &RepoContext,
    target: &Target,
) -> Result<(BookmarkName, ChangesetId), MegarepoError> {
    find_bookmark_and_value(ctx, target_repo, &target.bookmark).await
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_create_relative_symlink() -> Result<(), Error> {
        let path = MPath::new(&b"dir/1.txt"[..])?;
        let base = MPath::new(&b"dir/2.txt"[..])?;
        let bytes = create_relative_symlink(&path, &base)?;
        assert_eq!(bytes, Bytes::from(&b"1.txt"[..]));

        let path = MPath::new(&b"dir/1.txt"[..])?;
        let base = MPath::new(&b"base/2.txt"[..])?;
        let bytes = create_relative_symlink(&path, &base)?;
        assert_eq!(bytes, Bytes::from(&b"../dir/1.txt"[..]));

        let path = MPath::new(&b"dir/subdir/1.txt"[..])?;
        let base = MPath::new(&b"dir/2.txt"[..])?;
        let bytes = create_relative_symlink(&path, &base)?;
        assert_eq!(bytes, Bytes::from(&b"subdir/1.txt"[..]));

        let path = MPath::new(&b"dir1/subdir1/1.txt"[..])?;
        let base = MPath::new(&b"dir2/subdir2/2.txt"[..])?;
        let bytes = create_relative_symlink(&path, &base)?;
        assert_eq!(bytes, Bytes::from(&b"../../dir1/subdir1/1.txt"[..]));

        Ok(())
    }
}
