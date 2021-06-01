/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::common::{MegarepoOp, SourceName};
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
use megarepo_config::{MononokeMegarepoConfigs, Source, SyncTargetConfig};
use megarepo_error::MegarepoError;
use megarepo_mapping::CommitRemappingState;
use mononoke_api::Mononoke;
use mononoke_types::{BonsaiChangesetMut, ChangesetId, DateTime, FileChange, FileType, MPath};
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

struct SourceAndMovedChangesets {
    source: ChangesetId,
    moved: ChangesetId,
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
    ) -> Result<(), MegarepoError> {
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

        Ok(())
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
        let mut moved_commits = vec![];
        for source_config in &sync_target_config.sources {
            // TODO(stash): check that changeset is allowed to be synced
            let changeset_id = changesets_to_merge
                .get(&SourceName(source_config.name.clone()))
                .ok_or_else(|| {
                    MegarepoError::request(anyhow!(
                        "Not found changeset to merge for {}",
                        source_config.name
                    ))
                })?;

            let mover = create_source_to_target_multi_mover(source_config.mapping.clone())
                .map_err(MegarepoError::internal)?;

            let linkfiles = self.prepare_linkfiles(source_config, &mover)?;
            let linkfiles = self.upload_linkfiles(ctx, linkfiles, repo).await?;
            // TODO(stash): it assumes that commit is present in target
            let moved = self
                .create_single_move_commit(ctx, repo, *changeset_id, &mover, linkfiles)
                .await?;
            let source_and_moved_changeset = SourceAndMovedChangesets {
                source: *changeset_id,
                moved,
            };
            moved_commits.push((
                SourceName(source_config.name.clone()),
                source_and_moved_changeset,
            ));
        }

        Ok(moved_commits)
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
                .map(|(_, css)| css.moved)
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
    ) -> Result<ChangesetId, Error> {
        let root_fsnode_id = RootFsnodeId::derive(ctx, repo, cs_id).await?;
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
        let bcs = BonsaiChangesetMut {
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

        let move_cs_id = bcs.get_changeset_id();
        save_bonsai_changesets(vec![bcs], ctx.clone(), repo.clone()).await?;

        Ok(move_cs_id)
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

#[cfg(test)]
mod test {
    use super::*;
    use crate::megarepo_test_utils::{MegarepoTest, SyncTargetConfigBuilder};
    use crate::sync_changeset::SyncChangeset;
    use anyhow::Error;
    use fbinit::FacebookInit;
    use maplit::hashmap;
    use megarepo_config::Target;
    use megarepo_mapping::REMAPPING_STATE_FILE;
    use mononoke_types::MPath;
    use tests_utils::{
        bookmark, list_working_copy_utf8, list_working_copy_utf8_with_types, resolve_cs_id,
        CreateCommitContext,
    };

    #[fbinit::test]
    async fn test_add_sync_target_simple(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut test = MegarepoTest::new(&ctx).await?;
        let target: Target = test.target("target".to_string());

        let first_source_name = "source_1".to_string();
        let second_source_name = "source_2".to_string();
        let version = "version_1".to_string();
        SyncTargetConfigBuilder::new(test.repo_id(), target.clone(), version.clone())
            .source_builder(first_source_name.clone())
            .set_prefix_bookmark_to_source_name()
            .build_source()?
            .source_builder(second_source_name.clone())
            .set_prefix_bookmark_to_source_name()
            .build_source()?
            .build(&mut test.configs_storage);

        println!("Create initial source commits and bookmarks");
        let first_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
            .add_file("first", "first")
            .commit()
            .await?;

        bookmark(&ctx, &test.blobrepo, first_source_name.clone())
            .set_to(first_source_cs_id)
            .await?;

        let second_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
            .add_file("second", "second")
            .commit()
            .await?;

        bookmark(&ctx, &test.blobrepo, second_source_name.clone())
            .set_to(second_source_cs_id)
            .await?;

        let configs_storage: Arc<dyn MononokeMegarepoConfigs> =
            Arc::new(test.configs_storage.clone());

        let sync_target_config = test.configs_storage.get_config_by_version(
            ctx.clone(),
            target.clone(),
            version.clone(),
        )?;
        let add_sync_target = AddSyncTarget::new(&configs_storage, &test.mononoke);
        add_sync_target
            .run(
                &ctx,
                sync_target_config,
                hashmap! {
                    SourceName(first_source_name.clone()) => first_source_cs_id,
                    SourceName(second_source_name.clone()) => second_source_cs_id,
                },
                None,
            )
            .await?;

        let target_cs_id = resolve_cs_id(&ctx, &test.blobrepo, "target").await?;
        let mut wc = list_working_copy_utf8(&ctx, &test.blobrepo, target_cs_id).await?;

        let state =
            CommitRemappingState::read_state_from_commit(&ctx, &test.blobrepo, target_cs_id)
                .await?;
        assert_eq!(
            state.get_latest_synced_changeset(&first_source_name),
            Some(&first_source_cs_id),
        );
        assert_eq!(
            state.get_latest_synced_changeset(&second_source_name),
            Some(&second_source_cs_id),
        );
        assert_eq!(state.sync_config_version(), &version);

        // Remove file with commit remapping state because it's never present in source
        assert!(wc.remove(&MPath::new(REMAPPING_STATE_FILE)?).is_some());

        assert_eq!(
            wc,
            hashmap! {
                MPath::new("source_1/first")? => "first".to_string(),
                MPath::new("source_2/second")? => "second".to_string(),
            }
        );

        // Sync a few changesets on top of target
        let cs_id = CreateCommitContext::new(&ctx, &test.blobrepo, vec![first_source_cs_id])
            .add_file("first", "first_updated")
            .commit()
            .await?;

        bookmark(&ctx, &test.blobrepo, first_source_name.clone())
            .set_to(cs_id)
            .await?;

        let sync_changeset =
            SyncChangeset::new(&configs_storage, &test.mononoke, &test.megarepo_mapping);

        sync_changeset
            .sync(&ctx, cs_id, &first_source_name, &target)
            .await?;

        let target_cs_id = resolve_cs_id(&ctx, &test.blobrepo, "target").await?;
        let mut wc = list_working_copy_utf8(&ctx, &test.blobrepo, target_cs_id).await?;
        // Remove file with commit remapping state because it's never present in source
        assert!(wc.remove(&MPath::new(REMAPPING_STATE_FILE)?).is_some());

        assert_eq!(
            wc,
            hashmap! {
                MPath::new("source_1/first")? => "first_updated".to_string(),
                MPath::new("source_2/second")? => "second".to_string(),
            }
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_add_sync_target_with_linkfiles(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut test = MegarepoTest::new(&ctx).await?;
        let target: Target = test.target("target".to_string());

        let first_source_name = "source_1".to_string();
        let second_source_name = "source_2".to_string();
        let version = "version_1".to_string();
        SyncTargetConfigBuilder::new(test.repo_id(), target.clone(), version.clone())
            .source_builder(first_source_name.clone())
            .set_prefix_bookmark_to_source_name()
            .linkfile("first", "linkfiles/first")
            .build_source()?
            .source_builder(second_source_name.clone())
            .set_prefix_bookmark_to_source_name()
            .linkfile("second", "linkfiles/second")
            .build_source()?
            .build(&mut test.configs_storage);

        println!("Create initial source commits and bookmarks");
        let first_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
            .add_file("first", "first")
            .commit()
            .await?;

        bookmark(&ctx, &test.blobrepo, first_source_name.clone())
            .set_to(first_source_cs_id)
            .await?;

        let second_source_cs_id = CreateCommitContext::new_root(&ctx, &test.blobrepo)
            .add_file("second", "second")
            .commit()
            .await?;

        bookmark(&ctx, &test.blobrepo, second_source_name.clone())
            .set_to(second_source_cs_id)
            .await?;

        let configs_storage: Arc<dyn MononokeMegarepoConfigs> =
            Arc::new(test.configs_storage.clone());

        let sync_target_config =
            test.configs_storage
                .get_config_by_version(ctx.clone(), target, version.clone())?;
        let add_sync_target = AddSyncTarget::new(&configs_storage, &test.mononoke);
        add_sync_target
            .run(
                &ctx,
                sync_target_config,
                hashmap! {
                    SourceName(first_source_name.clone()) => first_source_cs_id,
                    SourceName(second_source_name.clone()) => second_source_cs_id,
                },
                None,
            )
            .await?;

        let target_cs_id = resolve_cs_id(&ctx, &test.blobrepo, "target").await?;
        let mut wc = list_working_copy_utf8_with_types(&ctx, &test.blobrepo, target_cs_id).await?;

        // Remove file with commit remapping state because it's never present in source
        assert!(wc.remove(&MPath::new(REMAPPING_STATE_FILE)?).is_some());

        assert_eq!(
            wc,
            hashmap! {
                MPath::new("source_1/first")? => ("first".to_string(), FileType::Regular),
                MPath::new("source_2/second")? => ("second".to_string(), FileType::Regular),
                MPath::new("linkfiles/first")? => ("source_1/first".to_string(), FileType::Symlink),
                MPath::new("linkfiles/second")? => ("source_2/second".to_string(), FileType::Symlink),
            }
        );

        Ok(())
    }
}
