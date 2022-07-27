/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use async_trait::async_trait;
use blobrepo::save_bonsai_changesets;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateReason;
use bytes::Bytes;
use changesets::Changesets;
use changesets::ChangesetsRef;
use commit_transformation::create_directory_source_to_target_multi_mover;
use commit_transformation::create_source_to_target_multi_mover;
use commit_transformation::DirectoryMultiMover;
use commit_transformation::MultiMover;
use context::CoreContext;
use derived_data::BonsaiDerived;
use fsnodes::RootFsnodeId;
use futures::future::try_join;
use futures::future::try_join_all;
use futures::stream;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use itertools::EitherOrBoth;
use itertools::Itertools;
use manifest::bonsai_diff;
use manifest::BonsaiDiffFileChange;
use manifest::Entry;
use manifest::ManifestOps;
use megarepo_config::MononokeMegarepoConfigs;
use megarepo_config::Source;
use megarepo_config::SourceRevision;
use megarepo_config::SyncConfigVersion;
use megarepo_config::SyncTargetConfig;
use megarepo_config::Target;
use megarepo_error::MegarepoError;
use megarepo_mapping::CommitRemappingState;
use megarepo_mapping::SourceName;
use mercurial_derived_data::DeriveHgChangeset;
use mercurial_types::HgFileNodeId;
use mononoke_api::ChangesetContext;
use mononoke_api::Mononoke;
use mononoke_api::MononokePath;
use mononoke_api::RepoContext;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::MPath;
use mononoke_types::RepositoryId;
use mutable_renames::MutableRenameEntry;
use mutable_renames::MutableRenames;
use repo_authorization::AuthorizationContext;
use sorted_vector_map::SortedVectorMap;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use tunables::tunables;
use unodes::RootUnodeManifestId;

pub struct SourceAndMovedChangesets {
    pub source: ChangesetId,
    pub moved: BonsaiChangeset,
    pub mutable_renames: Vec<MutableRenameEntry>,
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
        repo: &RepoContext,
        removed: &[(Source, ChangesetId)],
        message: Option<String>,
        additions_merge: &Option<ChangesetContext>,
        old_target_cs: &ChangesetContext,
        state: &CommitRemappingState,
        new_version: Option<String>,
    ) -> Result<ChangesetId, MegarepoError> {
        let mut all_removed_files = HashSet::new();
        for (source, source_cs_id) in removed {
            let paths_in_target_belonging_to_source = self
                .paths_in_target_belonging_to_source(ctx, source, *source_cs_id)
                .await?;
            for path in &paths_in_target_belonging_to_source {
                if let Some(path) = path.clone().into_mpath() {
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
                    new_version,
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
        state
            .save_in_changeset(ctx, repo.blob_repo(), &mut bcs)
            .await?;
        let merge = bcs.freeze()?;
        save_bonsai_changesets(vec![merge.clone()], ctx.clone(), repo.blob_repo()).await?;

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
                repo,
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
        repo: &RepoContext,
        merge_commit: ChangesetId,
        new_parent_commits: Vec<ChangesetId>,
        message: Option<String>,
    ) -> Result<ChangesetId, MegarepoError> {
        let blob_repo = repo.blob_repo();
        let hg_cs_merge = async {
            let hg_cs_id = blob_repo.derive_hg_changeset(ctx, merge_commit).await?;
            let hg_cs = hg_cs_id.load(ctx, blob_repo.blobstore()).await?;
            Ok(hg_cs.manifestid())
        };
        let parent_hg_css = try_join_all(new_parent_commits.iter().map(|p| async move {
            let hg_cs_id = blob_repo.derive_hg_changeset(ctx, *p).await?;
            let hg_cs = hg_cs_id.load(ctx, blob_repo.blobstore()).await?;
            Result::<_, Error>::Ok(hg_cs.manifestid())
        }));

        let (hg_cs_merge, parent_hg_css) = try_join(hg_cs_merge, parent_hg_css)
            .await
            .map_err(Error::from)?;

        let file_changes = bonsai_diff(
            ctx.clone(),
            blob_repo.get_blobstore(),
            hg_cs_merge,
            parent_hg_css.into_iter().collect(),
        )
        .map_ok(|diff| async move {
            match diff {
                BonsaiDiffFileChange::Changed(path, ty, entry_id)
                | BonsaiDiffFileChange::ChangedReusedId(path, ty, entry_id) => {
                    let file_node_id = HgFileNodeId::new(entry_id.into_nodehash());
                    let envelope = file_node_id.load(ctx, blob_repo.blobstore()).await?;
                    let size = envelope.content_size();
                    let content_id = envelope.content_id();

                    Ok((path, FileChange::tracked(content_id, ty, size as u64, None)))
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
            committer: None,
            committer_date: None,
            message: message.unwrap_or_else(|| "target config change".to_string()),
            extra: SortedVectorMap::new(),
            file_changes: file_changes.into_iter().collect(),
            is_snapshot: false,
        };
        let merge = bcs.freeze()?;
        save_bonsai_changesets(vec![merge.clone()], ctx.clone(), repo.blob_repo()).await?;

        Ok(merge.get_changeset_id())
    }

    async fn create_deletion_commit(
        &self,
        ctx: &CoreContext,
        repo: &RepoContext,
        old_target_cs: &ChangesetContext,
        removed_files: HashSet<MPath>,
        new_version: Option<String>,
    ) -> Result<ChangesetId, MegarepoError> {
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
        save_bonsai_changesets(
            vec![old_target_with_removed_files.clone()],
            ctx.clone(),
            repo.blob_repo(),
        )
        .await?;

        Ok(old_target_with_removed_files.get_changeset_id())
    }

    async fn verify_no_file_conflicts(
        &self,
        repo: &RepoContext,
        additions_merge: &ChangesetContext,
        p1: ChangesetId,
    ) -> Result<(), MegarepoError> {
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
                async move |path_context| {
                    Result::<(), _>::Err(MegarepoError::request(anyhow!(
                        "path {} cannot be added to the target - it's already present",
                        &path_context.path()
                    )))
                }
            })
            .await?;

        // Now check if we have a file in target which has the same path
        // as a directory in additions_merge i.e. detect file-dir conflit
        // where file is from target and dir from additions_merge
        let mut addition_prefixes = vec![];
        for addition in additions {
            for dir in addition.prefixes() {
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
                        // TODO(stash): it would be good to show which file it conflicts with
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
        repo: &BlobRepo,
        cs_id: ChangesetId,
        mover: &MultiMover,
        directory_mover: &DirectoryMultiMover,
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
            if !moved.iter().any(|cur_path| cur_path == &path) {
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

        let moved_bcs = new_megarepo_automation_commit(
            vec![cs_id],
            format!("move commit for source {}", source_name.0),
            file_changes.into_iter().collect(),
        )
        .freeze()?;

        let mutable_renames = self
            .create_mutable_renames(
                ctx,
                repo,
                cs_id,
                moved_bcs.get_changeset_id(),
                mover,
                directory_mover,
            )
            .await?;

        let source_and_moved_changeset = SourceAndMovedChangesets {
            source: cs_id,
            moved: moved_bcs,
            mutable_renames,
        };
        Ok(source_and_moved_changeset)
    }

    // Return all paths from the given source as seen in target.
    async fn paths_in_target_belonging_to_source(
        &self,
        ctx: &CoreContext,
        source: &Source,
        source_changeset_id: ChangesetId,
    ) -> Result<HashSet<MononokePath>, MegarepoError> {
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
            .and_then(async move |path| {
                Ok(mover(&path.into_mpath().ok_or_else(|| {
                    MegarepoError::internal(anyhow!("mpath can't be null"))
                })?)?)
            })
            .try_collect()
            .await?;
        let mut all_paths: HashSet<MononokePath> = moved_paths
            .into_iter()
            .flatten()
            .map(|mpath| MononokePath::new(Some(mpath)))
            .collect();
        let linkfiles: HashSet<MononokePath> = source
            .mapping
            .linkfiles
            .iter()
            .map(|(dst, _src)| dst.try_into())
            .try_collect()?;
        all_paths.extend(linkfiles.into_iter());
        Ok(all_paths)
    }

    async fn create_mutable_renames(
        &self,
        ctx: &CoreContext,
        repo: &BlobRepo,
        cs_id: ChangesetId,
        dst_cs_id: ChangesetId,
        mover: &MultiMover,
        directory_mover: &DirectoryMultiMover,
    ) -> Result<Vec<MutableRenameEntry>, Error> {
        let root_unode_id = RootUnodeManifestId::derive(ctx, repo, cs_id)
            .await
            .map_err(Error::from)?;
        let unode_id = root_unode_id.manifest_unode_id();

        let entries = unode_id
            .list_all_entries(ctx.clone(), repo.get_blobstore())
            .try_collect::<Vec<_>>()
            .await?;

        let mut res = vec![];
        for (src_path, entry) in entries {
            match (src_path, entry) {
                (Some(src_path), Entry::Leaf(leaf)) => {
                    if tunables().get_megarepo_api_dont_set_file_mutable_renames() {
                        continue;
                    }

                    // TODO(stash, simonfar, mitrandir): we record file
                    // moves to mutable_renames even though these moves are already
                    // recorded in non-mutable renames. We have to do it because
                    // scsc log doesn't use non-mutable renames,
                    // but we'd like to use it
                    // (see https://fb.quip.com/GzYMAwil1JXX for more details)
                    let dst_paths = mover(&src_path)?;
                    for dst_path in dst_paths {
                        let mutable_rename_entry = MutableRenameEntry::new(
                            dst_cs_id,
                            Some(dst_path),
                            cs_id,
                            Some(src_path.clone()),
                            Entry::Leaf(leaf),
                        )?;
                        res.push(mutable_rename_entry);
                    }
                }
                (src_path, Entry::Tree(tree)) => {
                    if tunables().get_megarepo_api_dont_set_directory_mutable_renames() {
                        continue;
                    }

                    let dst_paths = directory_mover(&src_path)?;
                    for dst_path in dst_paths {
                        let mutable_rename_entry = MutableRenameEntry::new(
                            dst_cs_id,
                            dst_path,
                            cs_id,
                            src_path.clone(),
                            Entry::Tree(tree),
                        )?;
                        res.push(mutable_rename_entry);
                    }
                }
                _ => {
                    // We shouldn't end up in this branch
                    continue;
                }
            }
        }

        Ok(res)
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
        mutable_renames: &Arc<MutableRenames>,
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
                    let directory_mover = create_directory_source_to_target_multi_mover(
                        source_config.mapping.clone(),
                    )
                    .map_err(MegarepoError::request)?;

                    let linkfiles = self.prepare_linkfiles(&source_config, &directory_mover)?;
                    let linkfiles = self.upload_linkfiles(ctx, linkfiles, repo).await?;
                    // TODO(stash): it assumes that commit is present in target
                    let moved = self
                        .create_single_move_commit(
                            ctx,
                            repo,
                            changeset_id,
                            &mover,
                            &directory_mover,
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
                source_name,
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
            &repo,
        )
        .await?;

        let mutable_renames_count: usize = moved_commits
            .iter()
            .map(|(_, css)| css.mutable_renames.len())
            .sum();
        let mut scuba = ctx.scuba().clone();
        scuba.add("mutable_renames_count", mutable_renames_count);
        scuba.log_with_msg("Started saving mutable renames", None);
        self.save_mutable_renames(
            ctx,
            repo.changesets(),
            mutable_renames,
            moved_commits.iter().map(|(_, css)| &css.mutable_renames),
        )
        .await?;
        scuba.log_with_msg("Saved mutable renames", None);

        Ok(moved_commits)
    }

    async fn save_mutable_renames<'a>(
        &'a self,
        ctx: &'a CoreContext,
        changesets: &'a dyn Changesets,
        mutable_renames: &'a Arc<MutableRenames>,
        entries_iter: impl Iterator<Item = &'a Vec<MutableRenameEntry>> + Send + 'async_trait,
    ) -> Result<(), Error> {
        for entries in entries_iter {
            for chunk in entries.chunks(100) {
                mutable_renames
                    .add_or_overwrite_renames(ctx, changesets, chunk.to_vec())
                    .await?;
            }
        }

        Ok(())
    }

    async fn validate_changeset_to_merge(
        &self,
        _ctx: &CoreContext,
        _source_repo: &RepoContext,
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
    ) -> Result<BTreeMap<MPath, Bytes>, MegarepoError> {
        let mut links = BTreeMap::new();
        for (dst, src) in &source_config.mapping.linkfiles {
            // src is a file inside a given source, so mover needs to be applied to it
            let src = if src == "." {
                None
            } else {
                MPath::new_opt(src).map_err(MegarepoError::request)?
            };
            let dst = MPath::new(dst).map_err(MegarepoError::request)?;
            let moved_srcs = mover(&src).map_err(MegarepoError::request)?;

            let mut iter = moved_srcs.into_iter();
            let moved_src = match (iter.next(), iter.next()) {
                (Some(moved_src), None) => moved_src,
                (None, None) => {
                    let src = match src {
                        None => ".".to_string(),
                        Some(path) => path.to_string(),
                    };
                    return Err(MegarepoError::request(anyhow!(
                        "linkfile source {} does not map to any file inside source {}",
                        src,
                        source_config.name
                    )));
                }
                _ => {
                    let src = match src {
                        None => ".".to_string(),
                        Some(path) => path.to_string(),
                    };
                    return Err(MegarepoError::request(anyhow!(
                        "linkfile source {} maps to too many files inside source {}",
                        src,
                        source_config.name
                    )));
                }
            }
            .ok_or_else(|| {
                let src = match src {
                    None => ".".to_string(),
                    Some(path) => path.to_string(),
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
        links: BTreeMap<MPath, Bytes>,
        repo: &BlobRepo,
    ) -> Result<BTreeMap<MPath, FileChange>, Error> {
        let linkfiles = stream::iter(links.into_iter())
            .map(Ok)
            .map_ok(|(path, content)| async {
                let ((content_id, size), fut) =
                    filestore::store_bytes(repo.blobstore(), repo.filestore_config(), ctx, content);
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
                    source_name,
                )?;
                let merge = bcs.freeze()?;
                cur_parents = vec![merge.get_changeset_id()];
                merges.push(merge);
            }
        }

        let (last_source_name, last_moved_commit) = last_moved_commit;
        cur_parents.push(last_moved_commit.moved.get_changeset_id());
        let mut final_merge =
            self.create_merge_commit(message, cur_parents, sync_config_version, last_source_name)?;
        if let Some(state) = state {
            state.save_in_changeset(ctx, repo, &mut final_merge).await?;
        }
        let final_merge = final_merge.freeze()?;
        merges.push(final_merge.clone());
        save_bonsai_changesets(merges, ctx.clone(), repo).await?;

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
            "merging source {} for target version {}",
            source_name.0, version
        ));
        let bcs = new_megarepo_automation_commit(parents, message, Default::default());
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

        txn.create(&bookmark, cs_id, BookmarkUpdateReason::XRepoSync)?;

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
                txn.update(&bookmark, cs_id, old, BookmarkUpdateReason::XRepoSync)?;
            }
            None => {
                txn.create(&bookmark, cs_id, BookmarkUpdateReason::XRepoSync)?;
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
    let bookmark = BookmarkName::new(bookmark_name).map_err(MegarepoError::request)?;

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
    file_changes: SortedVectorMap<MPath, FileChange>,
) -> BonsaiChangesetMut {
    BonsaiChangesetMut {
        parents,
        author: "svcscm".to_string(),
        author_date: DateTime::now(),
        committer: None,
        committer_date: None,
        message,
        extra: SortedVectorMap::new(),
        file_changes,
        is_snapshot: false,
    }
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
