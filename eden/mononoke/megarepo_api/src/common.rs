/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Error;
use anyhow::anyhow;
use async_trait::async_trait;
use blobstore::Loadable;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateReason;
use bookmarks::BookmarksRef;
use bulk_derivation::BulkDerivation;
use bytes::Bytes;
use changesets_creation::save_changesets;
use commit_transformation::DirectoryMultiMover;
use commit_transformation::MultiMover;
use commit_transformation::create_directory_source_to_target_multi_mover;
use commit_transformation::create_source_to_target_multi_mover;
use content_manifest_derivation::RootContentManifestId;
use context::CoreContext;
use fsnodes::RootFsnodeId;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use futures::future::try_join;
use futures::future::try_join_all;
use futures::stream;
use futures_retry::retry;
use futures_stats::TimedTryFutureExt;
use itertools::EitherOrBoth;
use itertools::Itertools;
use manifest::BonsaiDiffFileChange;
use manifest::ManifestOps;
use manifest::bonsai_diff;
use megarepo_config::MononokeMegarepoConfigs;
use megarepo_config::Source;
use megarepo_config::SourceRevision;
use megarepo_config::SyncConfigVersion;
use megarepo_config::SyncTargetConfig;
use megarepo_config::Target;
use megarepo_error::MegarepoError;
use megarepo_mapping::CommitRemappingState;
use megarepo_mapping::SourceName;
use mercurial_derivation::DeriveHgChangeset;
use mercurial_types::HgFileNodeId;
use metaconfig_types::RepoConfigArc;
use mononoke_api::ChangesetContext;
use mononoke_api::Mononoke;
use mononoke_api::MononokeRepo;
use mononoke_api::RepoContext;
use mononoke_api::path::MononokePathPrefixes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::DerivableType;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::GitLfs;
use mononoke_types::NonRootMPath;
use mononoke_types::RepositoryId;
use mononoke_types::content_manifest::compat::ContentManifestFile;
use mononoke_types::content_manifest::compat::ContentManifestId;
use mononoke_types::path::MPath;
use repo_authorization::AuthorizationContext;
use slog::info;
use sorted_vector_map::SortedVectorMap;
use tracing::Instrument;

use crate::Repo;

pub struct SourceAndMovedChangesets {
    pub source: ChangesetId,
    pub moved: BonsaiChangeset,
}

const MAX_BOOKMARK_MOVE_ATTEMPTS: usize = 5;
const DEFAULT_NUM_HEADS_TO_DERIVE_AT_ONCE: usize = 10;

#[async_trait]
pub trait MegarepoOp<R> {
    fn mononoke(&self) -> &Arc<Mononoke<R>>;

    async fn find_repo_by_id(
        &self,
        ctx: &CoreContext,
        repo_id: i64,
    ) -> Result<RepoContext<R>, MegarepoError>
    where
        R: MononokeRepo,
    {
        let target_repo_id = RepositoryId::new(repo_id.try_into().unwrap());
        let target_repo = self
            .mononoke()
            .repo_by_id(ctx.clone(), target_repo_id)
            .await
            .map_err(MegarepoError::internal)?
            .ok_or_else(|| MegarepoError::request(anyhow!("repo not found {}", target_repo_id)))?
            .with_authorization_context(AuthorizationContext::new_bypass_access_control())
            .build()
            .await
            .map_err(MegarepoError::internal)?;
        Ok(target_repo)
    }

    // In this diff we want to apply all file removals and add all the new
    // file additions from additions_merge commit.
    // The easiest way to do it is to create a deletion commit on top of
    // target commit and then merge it with `additions_merge` commit.
    // The problem is that deletion commit would be a broken commit
    // on the mainline, which can affect things like bisects.
    // To avoid having this deletion commit in the main line of development
    // we do the following:
    // 1) Produce a merge commit whose parents are additions_merge and deletion commit
    //
    //     M1
    //     | \
    //    Del  Adds
    //     |
    //   Old target
    //
    // 2) Use merge commit's manifest to produce a new bonsai commit merge whose parent is not
    //    a deletion commit.
    //
    //     M2
    //     | \
    //     |  Adds
    //     |
    //    Old target
    async fn create_final_merge_commit_with_removals(
        &self,
        ctx: &CoreContext,
        repo: &RepoContext<R>,
        removed: &[(Source, ChangesetId)],
        message: Option<String>,
        additions_merge: &Option<ChangesetContext<R>>,
        old_target_cs: &ChangesetContext<R>,
        state: &CommitRemappingState,
        new_config: Option<&SyncTargetConfig>,
    ) -> Result<ChangesetId, MegarepoError>
    where
        R: MononokeRepo,
    {
        let mut all_removed_files = HashSet::new();
        for (source, source_cs_id) in removed {
            let paths_in_target_belonging_to_source = self
                .paths_in_target_belonging_to_source(ctx, source, *source_cs_id)
                .await?;
            for path in &paths_in_target_belonging_to_source {
                if let Some(path) = path.clone().into_optional_non_root_path() {
                    all_removed_files.insert(path);
                }
            }
        }

        let maybe_deletion_commit = if !all_removed_files.is_empty() {
            Some(
                self.create_deletion_commit(
                    ctx,
                    repo,
                    old_target_cs,
                    all_removed_files.clone(),
                    new_config.map(|config| config.version.clone()),
                )
                .await?,
            )
        } else {
            None
        };

        let p1 = maybe_deletion_commit.unwrap_or_else(|| old_target_cs.id());

        let mut parents = vec![p1];
        // Verify that none of the files that will be merged in collides
        // with what's already in the target.
        if let Some(additions_merge) = additions_merge {
            self.verify_no_file_conflicts(repo, additions_merge, p1)
                .await?;

            parents.push(additions_merge.id())
        }

        let mut bcs = new_megarepo_automation_commit(
            parents,
            message
                .clone()
                .unwrap_or_else(|| "target config change".to_string()),
            Default::default(),
        );
        state.save_in_changeset(ctx, repo.repo(), &mut bcs).await?;
        if let Some(new_config) = new_config {
            save_sync_target_config_in_changeset(ctx, repo.repo(), new_config, &mut bcs).await?;
        }
        let merge = bcs.freeze()?;
        save_changesets(ctx, repo.repo(), vec![merge.clone()]).await?;

        // We don't want to have deletion commit on our mainline. So we'd like to create a new
        // merge commit whose parent is not a deletion commit. For that we take the manifest
        // from the merge commit we already have, and use bonsai_diff function to create a new
        // merge commit, whose parent is not an old_target changeset, not a deletion commit.

        let mut new_parents = vec![old_target_cs.id()];
        if let Some(additions_merge) = additions_merge {
            new_parents.push(additions_merge.id());
        }

        let result = self
            .create_new_changeset_using_parents(
                ctx,
                repo.repo(),
                merge.get_changeset_id(),
                new_parents,
                message,
            )
            .await?;

        Ok(result)
    }

    async fn create_new_changeset_using_parents(
        &self,
        ctx: &CoreContext,
        repo: &impl Repo,
        merge_commit: ChangesetId,
        new_parent_commits: Vec<ChangesetId>,
        message: Option<String>,
    ) -> Result<ChangesetId, MegarepoError> {
        let hg_cs_merge = async {
            let hg_cs_id = repo.derive_hg_changeset(ctx, merge_commit).await?;
            let hg_cs = hg_cs_id.load(ctx, repo.repo_blobstore()).await?;
            Ok(hg_cs.manifestid())
        };
        let parent_hg_css = try_join_all(new_parent_commits.iter().map(|p| async move {
            let hg_cs_id = repo.derive_hg_changeset(ctx, *p).await?;
            let hg_cs = hg_cs_id.load(ctx, repo.repo_blobstore()).await?;
            Result::<_, Error>::Ok(hg_cs.manifestid())
        }));

        let (hg_cs_merge, parent_hg_css) = try_join(hg_cs_merge, parent_hg_css).await?;

        let file_changes = bonsai_diff(
            ctx.clone(),
            repo.repo_blobstore().clone(),
            hg_cs_merge,
            parent_hg_css.into_iter().collect(),
        )
        .map_ok(|diff| async move {
            match diff {
                BonsaiDiffFileChange::Changed(path, (ty, entry_id))
                | BonsaiDiffFileChange::ChangedReusedId(path, (ty, entry_id)) => {
                    let file_node_id = HgFileNodeId::new(entry_id.into_nodehash());
                    let envelope = file_node_id.load(ctx, repo.repo_blobstore()).await?;
                    let size = envelope.content_size();
                    let content_id = envelope.content_id();

                    Ok((
                        path,
                        FileChange::tracked(content_id, ty, size, None, GitLfs::FullContent),
                    ))
                }
                BonsaiDiffFileChange::Deleted(path) => Ok((path, FileChange::Deletion)),
            }
        })
        .try_buffer_unordered(100)
        .try_collect::<std::collections::BTreeMap<_, _>>()
        .await?;

        let bcs = BonsaiChangesetMut {
            parents: new_parent_commits,
            author: "svcscm".to_string(),
            author_date: DateTime::now(),
            message: message.unwrap_or_else(|| "target config change".to_string()),
            file_changes: file_changes.into_iter().collect(),
            ..Default::default()
        };
        let merge = bcs.freeze()?;
        save_changesets(ctx, repo, vec![merge.clone()]).await?;

        Ok(merge.get_changeset_id())
    }

    async fn create_deletion_commit(
        &self,
        ctx: &CoreContext,
        repo: &RepoContext<R>,
        old_target_cs: &ChangesetContext<R>,
        removed_files: HashSet<NonRootMPath>,
        new_version: Option<String>,
    ) -> Result<ChangesetId, MegarepoError>
    where
        R: MononokeRepo,
    {
        let file_changes = removed_files
            .into_iter()
            .map(|path| (path, FileChange::Deletion))
            .collect();
        let message = match new_version {
            Some(new_version) => {
                format!("deletion commit for {}", new_version)
            }
            None => "deletion commit".to_string(),
        };

        let old_target_with_removed_files =
            new_megarepo_automation_commit(vec![old_target_cs.id()], message, file_changes);
        let old_target_with_removed_files = old_target_with_removed_files.freeze()?;
        save_changesets(
            ctx,
            repo.repo(),
            vec![old_target_with_removed_files.clone()],
        )
        .await?;

        Ok(old_target_with_removed_files.get_changeset_id())
    }

    async fn verify_no_file_conflicts(
        &self,
        repo: &RepoContext<R>,
        additions_merge: &ChangesetContext<R>,
        p1: ChangesetId,
    ) -> Result<(), MegarepoError>
    where
        R: MononokeRepo,
    {
        let p1 = repo
            .changeset(p1)
            .await?
            .ok_or_else(|| anyhow!("p1 commit {} not found", p1))?;

        // First find if any of the files from additions merge conflict
        // with a file or a directory from the target - if target commit
        // has these entries then we have a conflict
        let additions = additions_merge
            .find_files_unordered(None, None)
            .await?
            .map_err(MegarepoError::internal)
            .try_collect::<Vec<_>>()
            .await?;

        p1.paths(additions.clone().into_iter())
            .await?
            .map_err(MegarepoError::internal)
            .try_for_each({
                |path_context| async move {
                    Result::<(), _>::Err(MegarepoError::request(anyhow!(
                        "path {} cannot be added to the target - it's already present",
                        &path_context.path()
                    )))
                }
            })
            .await?;

        // Now check if we have a file in target which has the same path
        // as a directory in additions_merge i.e. detect file-dir conflict
        // where file is from target and dir from additions_merge
        let mut addition_prefixes = vec![];
        for addition in additions {
            for dir in MononokePathPrefixes::new(&addition) {
                addition_prefixes.push(dir);
            }
        }

        p1.paths(addition_prefixes.into_iter())
            .await?
            .map_err(MegarepoError::internal)
            .try_for_each({
                |path_context| async move {
                    // We got file/dir conflict - old target has a file
                    // with the same path as a directory in merge commit with additions
                    if path_context.is_file().await? {
                        Result::<(), _>::Err(MegarepoError::request(anyhow!(
                            "File in target path {} conflicts with newly added files",
                            &path_context.path()
                        )))
                    } else {
                        Ok(())
                    }
                }
            })
            .await?;

        Ok(())
    }

    async fn create_single_move_commit(
        &self,
        ctx: &CoreContext,
        repo: &R,
        cs_id: ChangesetId,
        mover: &dyn MultiMover,
        linkfiles: BTreeMap<NonRootMPath, FileChange>,
        source_name: &SourceName,
    ) -> Result<SourceAndMovedChangesets, MegarepoError>
    where
        R: Repo,
    {
        let root_id: ContentManifestId = if let Ok(true) = justknobs::eval(
            "scm/mononoke:derived_data_use_content_manifests",
            None,
            Some(repo.repo_identity().name()),
        ) {
            repo.repo_derived_data()
                .derive::<RootContentManifestId>(ctx, cs_id)
                .await
                .map_err(Error::from)?
                .into_content_manifest_id()
                .into()
        } else {
            repo.repo_derived_data()
                .derive::<RootFsnodeId>(ctx, cs_id)
                .await
                .map_err(Error::from)?
                .into_fsnode_id()
                .into()
        };

        let entries = root_id
            .list_leaf_entries(ctx.clone(), repo.repo_blobstore().clone())
            .map_ok(|(path, file)| (path, ContentManifestFile(file)))
            .try_collect::<Vec<_>>()
            .await?;

        let mut file_changes: Vec<(NonRootMPath, FileChange)> = vec![];
        for (path, file) in entries {
            let moved = mover.multi_move_path(&path)?;

            // Check that path doesn't move to itself - in that case we don't need to
            // delete file
            if !moved.iter().any(|cur_path| cur_path == &path) {
                file_changes.push((path.clone(), FileChange::Deletion));
            }

            file_changes.extend(moved.into_iter().map(|target| {
                let fc = FileChange::tracked(
                    file.content_id(),
                    file.file_type(),
                    file.size(),
                    Some((path.clone(), cs_id)),
                    GitLfs::FullContent,
                );

                (target, fc)
            }));
        }
        file_changes.extend(linkfiles);

        let moved_bcs = new_megarepo_automation_commit(
            vec![cs_id],
            format!("move commit for source {}", source_name.0),
            file_changes.into_iter().collect(),
        )
        .freeze()?;

        let source_and_moved_changeset = SourceAndMovedChangesets {
            source: cs_id,
            moved: moved_bcs,
        };
        Ok(source_and_moved_changeset)
    }

    // Return all paths from the given source as seen in target.
    async fn paths_in_target_belonging_to_source(
        &self,
        ctx: &CoreContext,
        source: &Source,
        source_changeset_id: ChangesetId,
    ) -> Result<HashSet<MPath>, MegarepoError>
    where
        R: MononokeRepo,
    {
        let source_repo = self.find_repo_by_id(ctx, source.repo_id).await?;
        let mover = &create_source_to_target_multi_mover(source.mapping.clone())?;
        let source_changeset = source_repo
            .changeset(source_changeset_id)
            .await?
            .ok_or_else(|| MegarepoError::internal(anyhow!("changeset not found")))?;
        let moved_paths: Vec<_> = source_changeset
            .find_files_unordered(None, None)
            .await
            .map_err(MegarepoError::internal)?
            .map_err(MegarepoError::internal)
            .and_then(|path| async move {
                Ok(mover.multi_move_path(
                    &path
                        .into_optional_non_root_path()
                        .ok_or_else(|| MegarepoError::internal(anyhow!("mpath can't be null")))?,
                )?)
            })
            .try_collect()
            .await?;
        let mut all_paths: HashSet<MPath> =
            moved_paths.into_iter().flatten().map(MPath::from).collect();
        let linkfiles: HashSet<MPath> = source
            .mapping
            .linkfiles
            .keys()
            .map(|dst| MPath::new(dst.as_bytes()))
            .try_collect()?;
        all_paths.extend(linkfiles);
        Ok(all_paths)
    }

    // Creates move commits on top of source changesets that we want to merge
    // into the target. These move commits put all source files into a correct place
    // in a target.
    async fn create_move_commits<'b>(
        &'b self,
        ctx: &'b CoreContext,
        repo: &'b R,
        sources: &[Source],
        changesets_to_merge: &'b BTreeMap<SourceName, ChangesetId>,
    ) -> Result<Vec<(SourceName, SourceAndMovedChangesets)>, Error>
    where
        R: MononokeRepo,
    {
        let move_commits = stream::iter(sources.iter().cloned().map(Ok))
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
                    let directory_mover = create_directory_source_to_target_multi_mover(
                        source_config.mapping.clone(),
                    )
                    .map_err(MegarepoError::request)?;

                    let linkfiles = self.prepare_linkfiles(&source_config, &directory_mover)?;
                    let linkfiles = self.upload_linkfiles(ctx, linkfiles, repo).await?;
                    // NOTE: it assumes that commit is present in target
                    let moved = self
                        .create_single_move_commit(
                            ctx,
                            repo,
                            changeset_id,
                            mover.as_ref(),
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
        for (source_name, moved) in &move_commits {
            add_and_check_all_paths(
                &mut all_files_in_target,
                source_name,
                moved
                    .moved
                    .file_changes()
                    // Do not check deleted files
                    .filter_map(|(path, fc)| fc.is_changed().then_some(path)),
            )?;
        }

        save_changesets(
            ctx,
            repo,
            move_commits
                .iter()
                .map(|(_, css)| css.moved.clone())
                .collect(),
        )
        .await?;

        let mut scuba = ctx.scuba().clone();
        scuba.add("move_commits_count", move_commits.len());
        scuba.log_with_msg("Started deriving move commits", None);
        let cs_ids = move_commits
            .iter()
            .map(|(_, css)| css.moved.get_changeset_id())
            .collect::<Vec<_>>();

        let (stats, _) = derive_all_types(ctx, repo, &cs_ids).try_timed().await?;
        scuba.add_future_stats(&stats);
        scuba.log_with_msg("Derived move commits", None);

        Ok(move_commits)
    }

    async fn validate_changeset_to_merge(
        &self,
        _ctx: &CoreContext,
        _source_repo: &RepoContext<R>,
        source_config: &Source,
        changesets_to_merge: &BTreeMap<SourceName, ChangesetId>,
    ) -> Result<ChangesetId, MegarepoError>
    where
        R: MononokeRepo,
    {
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
            SourceRevision::bookmark(_bookmark) => {
                /* If the source is following a git repo branch we can't verify much as the bookmark
                doesn't have to exist in the megarepo */
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
        mover: &DirectoryMultiMover,
    ) -> Result<BTreeMap<NonRootMPath, Bytes>, MegarepoError> {
        let mut links = BTreeMap::new();
        for (dst, src) in &source_config.mapping.linkfiles {
            // src is a file inside a given source, so mover needs to be applied to it
            let src = if src == "." {
                MPath::ROOT
            } else {
                MPath::new(src).map_err(MegarepoError::request)?
            };
            let dst = NonRootMPath::new(dst).map_err(MegarepoError::request)?;
            let moved_srcs = mover(&src).map_err(MegarepoError::request)?;

            let mut iter = moved_srcs.into_iter();
            let moved_src = match (iter.next(), iter.next()) {
                // If the source maps to many files we use the first one as the symlink
                // source this choice doesn't matter for the symlinked content - just the
                // symlinked path.
                (Some(moved_src), _) => moved_src.into_optional_non_root_path(),
                (None, _) => {
                    let src = if src.is_root() {
                        ".".to_string()
                    } else {
                        src.to_string()
                    };
                    return Err(MegarepoError::request(anyhow!(
                        "linkfile source {} does not map to any file inside source {}",
                        src,
                        source_config.name
                    )));
                }
            }
            .ok_or_else(|| {
                let src = if src.is_root() {
                    ".".to_string()
                } else {
                    src.to_string()
                };
                MegarepoError::request(anyhow!(
                    "linkfile source {} does not map to any file inside the destination from source {}",
                    src,
                    source_config.name
                ))
            })?;

            let content = create_relative_symlink(&moved_src, &dst)?;
            links.insert(dst, content);
        }
        Ok(links)
    }

    async fn upload_linkfiles(
        &self,
        ctx: &CoreContext,
        links: BTreeMap<NonRootMPath, Bytes>,
        repo: &R,
    ) -> Result<BTreeMap<NonRootMPath, FileChange>, Error>
    where
        R: Repo,
    {
        let linkfiles = stream::iter(links.into_iter())
            .map(Ok)
            .map_ok(|(path, content)| async {
                let ((content_id, size), fut) = filestore::store_bytes(
                    repo.repo_blobstore(),
                    *repo.filestore_config(),
                    ctx,
                    content,
                );
                fut.await?;

                let fc = FileChange::tracked(
                    content_id,
                    FileType::Symlink,
                    size,
                    None,
                    GitLfs::FullContent,
                );

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
        repo: &R,
        moved_commits: Vec<(SourceName, SourceAndMovedChangesets)>,
        write_commit_remapping_state: bool,
        sync_target_config: &SyncTargetConfig,
        message: Option<String>,
        bookmark: String,
    ) -> Result<ChangesetId, MegarepoError>
    where
        R: Repo,
    {
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
                sync_target_config.version.clone(),
                Some(bookmark),
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
        let mut merge_cs_ids = vec![];
        let mut cur_parents = vec![];
        for (source_name, css) in first_moved_commits {
            cur_parents.push(css.moved.get_changeset_id());
            if cur_parents.len() > 1 {
                let bcs = self.create_merge_commit(
                    message.clone(),
                    cur_parents,
                    sync_target_config.version.clone(),
                    source_name,
                )?;
                let merge = bcs.freeze()?;
                cur_parents = vec![merge.get_changeset_id()];
                merge_cs_ids.push(merge.get_changeset_id());
                merges.push(merge);
            }
        }

        let (last_source_name, last_moved_commit) = last_moved_commit;
        cur_parents.push(last_moved_commit.moved.get_changeset_id());
        let mut final_merge = self.create_merge_commit(
            message,
            cur_parents,
            sync_target_config.version.clone(),
            last_source_name,
        )?;
        if let Some(state) = state {
            state.save_in_changeset(ctx, repo, &mut final_merge).await?;
            save_sync_target_config_in_changeset(ctx, repo, sync_target_config, &mut final_merge)
                .await?;
        }
        let final_merge = final_merge.freeze()?;
        merges.push(final_merge.clone());
        merge_cs_ids.push(final_merge.get_changeset_id());
        save_changesets(ctx, repo, merges).await?;

        let mut scuba = ctx.scuba().clone();
        scuba.add("merge_commits_count", merge_cs_ids.len());
        scuba.log_with_msg("Started deriving merge commits", None);
        let (stats, _) = derive_all_types(ctx, repo, &merge_cs_ids)
            .try_timed()
            .await?;
        scuba.add_future_stats(&stats);
        scuba.log_with_msg("Derived merge commits", None);

        Ok(final_merge.get_changeset_id())
    }

    fn create_merge_commit(
        &self,
        message: Option<String>,
        parents: Vec<ChangesetId>,
        version: SyncConfigVersion,
        source_name: &SourceName,
    ) -> Result<BonsaiChangesetMut, Error> {
        // TODO(mateusz): figure out what fields
        // we need to set here
        let message = message.unwrap_or(format!(
            "merging source {} for target version {}",
            source_name.0, version
        ));
        let bcs = new_megarepo_automation_commit(parents, message, Default::default());
        Ok(bcs)
    }

    async fn create_bookmark(
        &self,
        ctx: &CoreContext,
        repo: &R,
        bookmark: String,
        cs_id: ChangesetId,
    ) -> Result<(), MegarepoError>
    where
        R: Repo,
    {
        let mut txn = repo.bookmarks().create_transaction(ctx.clone());
        let bookmark = BookmarkKey::new(bookmark).map_err(MegarepoError::request)?;

        txn.create(&bookmark, cs_id, BookmarkUpdateReason::XRepoSync)?;

        let success = txn
            .commit()
            .await
            .map_err(MegarepoError::internal)?
            .is_some();
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
        repo: &R,
        bookmark: String,
        (from_cs_id, to_cs_id): (ChangesetId, ChangesetId),
    ) -> Result<(), MegarepoError>
    where
        R: Repo,
    {
        let mut res = Ok(());
        for _retry_num in 0..MAX_BOOKMARK_MOVE_ATTEMPTS {
            res = self
                .move_bookmark_conditionally_internal(
                    ctx,
                    repo,
                    bookmark.clone(),
                    (from_cs_id, to_cs_id),
                )
                .await;
            if res.is_ok() {
                break;
            }
        }
        return res;
    }

    async fn move_bookmark_conditionally_internal(
        &self,
        ctx: &CoreContext,
        repo: &R,
        bookmark: String,
        (from_cs_id, to_cs_id): (ChangesetId, ChangesetId),
    ) -> Result<(), MegarepoError>
    where
        R: Repo,
    {
        let mut txn = repo.bookmarks().create_transaction(ctx.clone());
        let bookmark = BookmarkKey::new(bookmark).map_err(MegarepoError::request)?;
        txn.update(
            &bookmark,
            to_cs_id,
            from_cs_id,
            BookmarkUpdateReason::XRepoSync,
        )?;

        let success = txn
            .commit()
            .await
            .map_err(MegarepoError::internal)?
            .is_some();
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
    ) -> Result<(), MegarepoError>
    where
        R: MononokeRepo,
    {
        let repo = self
            .find_repo_by_id(ctx, sync_target_config.target.repo_id)
            .await?;
        let repo_config = repo.repo().repo_config_arc();
        let existing_config = megarepo_configs
            .get_config_by_version(
                ctx.clone(),
                repo_config,
                sync_target_config.target.clone(),
                sync_target_config.version.clone(),
            )
            .await
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
        repo: &RepoContext<R>,
    ) -> Result<Option<ChangesetId>, MegarepoError>
    where
        R: MononokeRepo,
    {
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
        repo: &RepoContext<R>,
        cs_id: ChangesetId,
    ) -> Result<CommitRemappingState, MegarepoError>
    where
        R: MononokeRepo,
    {
        let maybe_state = CommitRemappingState::read_state_from_commit_opt(ctx, repo.repo(), cs_id)
            .await
            .context("While reading remapping state file")
            .map_err(MegarepoError::request)?;

        maybe_state.ok_or_else(|| {
            MegarepoError::request(anyhow!("no remapping state file exist for {}", cs_id))
        })
    }
}

pub async fn find_bookmark_and_value<R: MononokeRepo>(
    ctx: &CoreContext,
    repo: &RepoContext<R>,
    bookmark_name: &str,
) -> Result<(BookmarkKey, ChangesetId), MegarepoError> {
    let bookmark = BookmarkKey::new(bookmark_name).map_err(MegarepoError::request)?;

    let cs_id = repo
        .repo()
        .bookmarks()
        .get(ctx.clone(), &bookmark, bookmarks::Freshness::MostRecent)
        .map_err(MegarepoError::internal)
        .await?
        .ok_or_else(|| MegarepoError::request(anyhow!("bookmark {} not found", bookmark)))?;

    Ok((bookmark, cs_id))
}

fn create_relative_symlink(path: &NonRootMPath, base: &NonRootMPath) -> Result<Bytes, Error> {
    let common_components = path.common_components(base);
    let path_no_prefix = path.into_iter().skip(common_components).collect::<Vec<_>>();
    let base_no_prefix = base.into_iter().skip(common_components).collect::<Vec<_>>();

    if path_no_prefix.is_empty() || base_no_prefix.is_empty() {
        return Err(anyhow!(
            "Can't create symlink for {} and {}: one path is a parent of another",
            path,
            base,
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
    all_files_in_target: &'a mut HashMap<NonRootMPath, SourceName>,
    source_name: &'a SourceName,
    iter: impl Iterator<Item = &'a NonRootMPath>,
) -> Result<(), MegarepoError> {
    for path in iter {
        add_and_check(all_files_in_target, source_name, path)?;
    }

    Ok(())
}

fn add_and_check<'a>(
    all_files_in_target: &'a mut HashMap<NonRootMPath, SourceName>,
    source_name: &'a SourceName,
    path: &NonRootMPath,
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
    target_repo: &'a impl Repo,
    target_cs_id: ChangesetId,
    target: &Target,
    megarepo_configs: &Arc<dyn MononokeMegarepoConfigs>,
) -> Result<(CommitRemappingState, SyncTargetConfig), MegarepoError> {
    let state =
        CommitRemappingState::read_state_from_commit(ctx, target_repo, target_cs_id).await?;

    let repo_config = target_repo.repo_config_arc();
    // We have a target config version - let's fetch target config itself.
    let target_config = megarepo_configs
        .get_config_by_version(
            ctx.clone(),
            repo_config,
            target.clone(),
            state.sync_config_version().clone(),
        )
        .await?;

    Ok((state, target_config))
}

pub(crate) async fn find_target_bookmark_and_value<R: MononokeRepo>(
    ctx: &CoreContext,
    target_repo: &RepoContext<R>,
    target: &Target,
) -> Result<(BookmarkKey, ChangesetId), MegarepoError> {
    find_bookmark_and_value(ctx, target_repo, &target.bookmark).await
}

pub fn find_source_config<'a, 'b>(
    source_name: &'a SourceName,
    target_config: &'b SyncTargetConfig,
) -> Result<&'b Source, MegarepoError> {
    let mut maybe_source_config = None;
    for source in &target_config.sources {
        if source_name.as_str() == source.source_name {
            maybe_source_config = Some(source);
            break;
        }
    }
    let source_config = maybe_source_config.ok_or_else(|| {
        MegarepoError::request(anyhow!("config for source {} not found", source_name))
    })?;

    Ok(source_config)
}

/// Used by megarepo automation to create brand-new commits
pub(crate) fn new_megarepo_automation_commit(
    parents: Vec<ChangesetId>,
    message: String,
    file_changes: SortedVectorMap<NonRootMPath, FileChange>,
) -> BonsaiChangesetMut {
    BonsaiChangesetMut {
        parents,
        author: "svcscm".to_string(),
        author_date: DateTime::now(),
        message,
        file_changes,
        ..Default::default()
    }
}

pub const SYNC_TARGET_CONFIG_FILE: &str = ".megarepo/sync_target_config";
pub async fn save_sync_target_config_in_changeset(
    ctx: &CoreContext,
    repo: &impl Repo,
    config: &SyncTargetConfig,
    bcs: &mut BonsaiChangesetMut,
) -> Result<(), Error> {
    if let Ok(false) = justknobs::eval(
        "scm/mononoke:megarepo_serialize_target_config_into_working_copy",
        None,
        Some(repo.repo_identity().name()),
    ) {
        return Ok(());
    }

    let bytes = serde_json::to_vec_pretty(&config).map_err(Error::from)?;

    let ((content_id, size), fut) = filestore::store_bytes(
        repo.repo_blobstore(),
        *repo.filestore_config(),
        ctx,
        bytes.into(),
    );

    fut.await?;

    let path = NonRootMPath::new(SYNC_TARGET_CONFIG_FILE)?;

    let fc = FileChange::tracked(
        content_id,
        FileType::Regular,
        size,
        None,
        GitLfs::FullContent,
    );
    if bcs.file_changes.insert(path, fc).is_some() {
        return Err(anyhow!(
            "New bonsai changeset already has {} file",
            SYNC_TARGET_CONFIG_FILE,
        ));
    }

    Ok(())
}

pub(crate) async fn derive_all_types_locally(
    ctx: &CoreContext,
    repo: &impl Repo,
    csids: &[ChangesetId],
    derived_data_types: &[DerivableType],
) -> Result<(), Error> {
    let num_heads_to_derive_at_once = justknobs::get(
        "scm/mononoke:megarepo_override_num_heads_to_derive_at_once",
        None,
    )
    .map(|jk| jk.max(1) as usize)
    .unwrap_or(DEFAULT_NUM_HEADS_TO_DERIVE_AT_ONCE);

    let override_batch_size =
        justknobs::get("scm/mononoke:megarepo_override_derivation_batch_size", None)
            .map(|jk| jk.max(1) as u64)
            .ok();

    for chunk in csids.chunks(num_heads_to_derive_at_once) {
        retry(
            async |attempt| {
                if attempt > 1 {
                    let mut scuba = ctx.scuba().clone();
                    scuba.log_with_msg(
                        "Derived data failed, retrying. Num retries",
                        Some(format!("{attempt}")),
                    );
                }
                repo.repo_derived_data()
                    .manager()
                    .derive_bulk_locally(ctx, chunk, None, derived_data_types, override_batch_size)
                    .await
            },
            Duration::from_secs(1),
        )
        .exponential_backoff(1.2)
        .jitter(Duration::from_secs(2))
        .retry_if(|_attempt, e| {
            let description = format!("{e:?}").to_ascii_lowercase();
            description.contains("blobstore") || description.contains("timeout")
        })
        .max_attempts(5)
        .inspect_err(|attempt, _err| info!(ctx.logger(), "attempt {attempt} failed"))
        .await?;
    }
    Ok(())
}

pub(crate) async fn derive_all_types_remotely(
    ctx: &CoreContext,
    repo: &impl Repo,
    csids: &[ChangesetId],
    derived_data_types: &[DerivableType],
) -> Result<(), Error> {
    let num_heads_to_derive_at_once = justknobs::get(
        "scm/mononoke:megarepo_override_num_heads_to_derive_at_once",
        None,
    )
    .map(|jk| jk.max(1) as usize)
    .unwrap_or(DEFAULT_NUM_HEADS_TO_DERIVE_AT_ONCE);

    let override_concurrency = justknobs::get(
        "scm/mononoke:megarepo_override_remote_derivation_concurrency",
        None,
    )
    .map(|jk| jk.max(1) as usize)
    .ok();

    let manager = repo
        .repo_derived_data()
        .manager_for_config("megarepo_api_rollout_remote_derivation")
        .unwrap_or(repo.repo_derived_data().manager());
    for chunk in csids.chunks(num_heads_to_derive_at_once) {
        retry(
            async |attempt| {
                if attempt > 1 {
                    let mut scuba = ctx.scuba().clone();
                    scuba.log_with_msg(
                        "Derived data failed, retrying. Num retries",
                        Some(format!("{attempt}")),
                    );
                }
                manager
                    .derive_bulk(ctx, chunk, None, derived_data_types, override_concurrency)
                    .await
            },
            Duration::from_secs(1),
        )
        .exponential_backoff(1.2)
        .jitter(Duration::from_secs(2))
        .retry_if(|_attempt, e| {
            let description = format!("{e:?}").to_ascii_lowercase();
            description.contains("blobstore") || description.contains("timeout")
        })
        .max_attempts(5)
        .inspect_err(|attempt, _err| info!(ctx.logger(), "attempt {attempt} failed"))
        .await?;
    }
    Ok(())
}

pub(crate) async fn derive_all_types(
    ctx: &CoreContext,
    repo: &impl Repo,
    csids: &[ChangesetId],
) -> Result<(), Error> {
    let derive_remotely =
        justknobs::eval("scm/mononoke:megarepo_derive_remotely", None, None).unwrap_or(false);

    let derived_data_types = repo
        .repo_derived_data()
        .active_config()
        .types
        .iter()
        .copied()
        .filter(|t| {
            // Filenodes cannot be derived for draft commits
            *t != DerivableType::FileNodes
        })
        .collect::<Vec<_>>();
    if derive_remotely {
        derive_all_types_remotely(ctx, repo, csids, &derived_data_types)
            .instrument(tracing::info_span!("derive all types remotely"))
            .await?;
    } else {
        derive_all_types_locally(ctx, repo, csids, &derived_data_types)
            .instrument(tracing::info_span!("derive all types locally"))
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_create_relative_symlink() -> Result<(), Error> {
        let path = NonRootMPath::new(&b"dir/1.txt"[..])?;
        let base = NonRootMPath::new(&b"dir/2.txt"[..])?;
        let bytes = create_relative_symlink(&path, &base)?;
        assert_eq!(bytes, Bytes::from(&b"1.txt"[..]));

        let path = NonRootMPath::new(&b"dir/1.txt"[..])?;
        let base = NonRootMPath::new(&b"base/2.txt"[..])?;
        let bytes = create_relative_symlink(&path, &base)?;
        assert_eq!(bytes, Bytes::from(&b"../dir/1.txt"[..]));

        let path = NonRootMPath::new(&b"dir/subdir/1.txt"[..])?;
        let base = NonRootMPath::new(&b"dir/2.txt"[..])?;
        let bytes = create_relative_symlink(&path, &base)?;
        assert_eq!(bytes, Bytes::from(&b"subdir/1.txt"[..]));

        let path = NonRootMPath::new(&b"dir1/subdir1/1.txt"[..])?;
        let base = NonRootMPath::new(&b"dir2/subdir2/2.txt"[..])?;
        let bytes = create_relative_symlink(&path, &base)?;
        assert_eq!(bytes, Bytes::from(&b"../../dir1/subdir1/1.txt"[..]));

        Ok(())
    }
}
