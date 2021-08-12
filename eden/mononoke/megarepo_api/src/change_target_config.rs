/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::common::{find_target_bookmark_and_value, find_target_sync_config, MegarepoOp};
use anyhow::{anyhow, Error};
use blobrepo::save_bonsai_changesets;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use commit_transformation::create_source_to_target_multi_mover;
use context::CoreContext;
use core::cmp::Ordering;
use derived_data_utils::derived_data_utils;
use futures::future;
use futures::{
    future::{try_join, try_join_all},
    stream::FuturesUnordered,
    TryStreamExt,
};
use itertools::{EitherOrBoth, Itertools};
use manifest::{bonsai_diff, BonsaiDiffFileChange};
use megarepo_config::{
    MononokeMegarepoConfigs, Source, SyncConfigVersion, SyncTargetConfig, Target,
};
use megarepo_error::MegarepoError;
use megarepo_mapping::{CommitRemappingState, SourceName};
use mercurial_types::HgFileNodeId;
use mononoke_api::{ChangesetContext, Mononoke, MononokePath, RepoContext};
use mononoke_types::{BonsaiChangesetMut, ChangesetId, DateTime, FileChange, MPath};
use sorted_vector_map::SortedVectorMap;
use std::collections::{BTreeMap, HashSet};
use std::convert::TryInto;
use std::sync::Arc;

/// Structure representing changes needed to be applied onto target to change its
/// config.
/// All the changes will be realized by removing and readding the source. In the
/// future we can make this datastructure richer and include less disruptive
/// methods of introducing small changes (like for example adding a linkfile).
struct SyncTargetConfigChanges {
    added: Vec<(Source, ChangesetId)>,
    removed: Vec<(Source, ChangesetId)>,
}

/// Comparator used for sorting the sources.
fn cmp_by_name(a: &Source, b: &Source) -> Ordering {
    Ord::cmp(&a.source_name, &b.source_name)
}

/// Compares the current state with the desired end state and returns the changes
/// needed to apply to current state.
fn diff_configs(
    old_config: &SyncTargetConfig,
    old_changesets: &BTreeMap<SourceName, ChangesetId>,
    new_config: &SyncTargetConfig,
    new_changesets: &BTreeMap<SourceName, ChangesetId>,
) -> Result<SyncTargetConfigChanges, MegarepoError> {
    let old_sources = old_config
        .sources
        .clone()
        .into_iter()
        .sorted_by(cmp_by_name);
    let new_sources = new_config
        .sources
        .clone()
        .into_iter()
        .sorted_by(cmp_by_name);
    let mut added = Vec::new();
    let mut removed = Vec::new();

    for merged in old_sources.merge_join_by(new_sources, cmp_by_name) {
        match merged {
            EitherOrBoth::Left(old_source) => {
                let cs_id = old_changesets
                    .get(&SourceName::new(&old_source.source_name))
                    .ok_or_else(|| {
                        MegarepoError::request(anyhow!(
                            "remapping state is missing mapping for {}",
                            &old_source.source_name
                        ))
                    })?;
                removed.push((old_source, *cs_id));
            }
            EitherOrBoth::Right(new_source) => {
                let cs_id = new_changesets
                    .get(&SourceName::new(&new_source.source_name))
                    .ok_or_else(|| {
                        MegarepoError::request(anyhow!(
                            "changesets_to_merge is missing mapping for {}",
                            &new_source.source_name
                        ))
                    })?;
                added.push((new_source, *cs_id));
            }
            EitherOrBoth::Both(old_source, new_source) => {
                let old_cs_id = old_changesets
                    .get(&SourceName::new(&old_source.source_name))
                    .ok_or_else(|| {
                        MegarepoError::request(anyhow!(
                            "remapping state is missing mapping for {}",
                            &old_source.source_name
                        ))
                    })?;
                let new_cs_id = new_changesets
                    .get(&SourceName::new(&new_source.source_name))
                    .ok_or_else(|| {
                        MegarepoError::request(anyhow!(
                            "changesets_to_merge is missing mapping for {}",
                            &new_source.source_name
                        ))
                    })?;
                if old_source != new_source || old_cs_id != new_cs_id {
                    removed.push((old_source, *old_cs_id));
                    added.push((new_source, *new_cs_id));
                }
            }
        }
    }
    Ok(SyncTargetConfigChanges { added, removed })
}

/// Change target config given a new config. After this command finishes it
/// creates move commits on top of source commits (for the newly added and
/// changed sources), merges them all together then merges them with current head
/// of the target while removing all files belonging to removed sources.
///
///        Tn+1
///       /    \
///      X      Tn
///     / \      \
///    M   M      \
///   /     \      \
///  S       S      \
///
/// Tn - pre-change (old) head of the target branch
/// Tn+1 - post-change (new) head of the target branch (merges in X, and does removals)
/// X - target merge commits
/// M - move commits
/// S - source commits that need to be merged
pub struct ChangeTargetConfig<'a> {
    pub megarepo_configs: &'a Arc<dyn MononokeMegarepoConfigs>,
    pub mononoke: &'a Arc<Mononoke>,
}

impl<'a> MegarepoOp for ChangeTargetConfig<'a> {
    fn mononoke(&self) -> &Arc<Mononoke> {
        &self.mononoke
    }
}

impl<'a> ChangeTargetConfig<'a> {
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
        target: &Target,
        new_version: SyncConfigVersion,
        target_location: ChangesetId,
        changesets_to_merge: BTreeMap<SourceName, ChangesetId>,
        message: Option<String>,
    ) -> Result<ChangesetId, MegarepoError> {
        let target_repo = self.find_repo_by_id(&ctx, target.repo_id).await?;

        // Find the target config version and remapping state that was used to
        // create the latest target commit. This config version will be used to
        // as a base for comparing with new config.
        let (target_bookmark, actual_target_location) =
            find_target_bookmark_and_value(&ctx, &target_repo, &target).await?;

        // target doesn't point to the commit we expect - check
        // if this method has already succeded and just immediately return the
        // result if so.
        if actual_target_location != target_location {
            return self
                .check_if_this_method_has_already_succeeded(
                    ctx,
                    &new_version,
                    (target_location, actual_target_location),
                    &changesets_to_merge,
                    &target_repo,
                )
                .await;
        }

        let old_target_cs = &target_repo
            .changeset(target_location)
            .await?
            .ok_or_else(|| {
                MegarepoError::internal(anyhow!("programming error - target changeset not found!"))
            })?;
        let (old_remapping_state, old_config) = find_target_sync_config(
            &ctx,
            target_repo.blob_repo(),
            target_location,
            &target,
            &self.megarepo_configs,
        )
        .await?;

        // Contruct the new config structure and the remapping state
        let new_config = self.megarepo_configs.get_config_by_version(
            ctx.clone(),
            target.clone(),
            new_version.clone(),
        )?;
        let new_remapping_state =
            CommitRemappingState::new(changesets_to_merge.clone(), new_version);

        // Diff the configs to find out action items.
        let diff = diff_configs(
            &old_config,
            &old_remapping_state.latest_synced_changesets,
            &new_config,
            &new_remapping_state.latest_synced_changesets,
        )?;

        // Construct the commit containing all the new content coming due to
        // config change.
        let additions_merge_cs_id = self
            .create_commit_with_new_sources(
                ctx,
                &target_repo,
                &diff,
                &changesets_to_merge,
                new_config.version.clone(),
                message.clone(),
            )
            .await?;
        let additions_merge = if let Some(additions_merge_cs_id) = additions_merge_cs_id {
            let mut scuba = ctx.scuba().clone();
            scuba.log_with_msg(
                "Created change target config merge commit for addtions",
                Some(format!("{}", &additions_merge_cs_id)),
            );
            target_repo
                .changeset(additions_merge_cs_id)
                .await
                .map_err(MegarepoError::internal)?
        } else {
            None
        };

        // Construct the commit merging in all the new additions and removing no
        // longer needed content.
        let final_merge = self
            .create_final_merge_commit_with_removals(
                ctx,
                &target_repo,
                &diff,
                message,
                &additions_merge,
                &old_target_cs,
                &new_remapping_state,
                new_config.version,
            )
            .await?;
        let mut scuba = ctx.scuba().clone();
        scuba.log_with_msg(
            "Created change target config merge commit connecting addtions to removals",
            Some(format!("{}", &final_merge)),
        );

        // Derrive all the necessary data before moving the bookmark
        let derived_data_types = target_repo
            .blob_repo()
            .get_derived_data_config()
            .enabled
            .types
            .iter();

        let derivers = FuturesUnordered::new();
        for ty in derived_data_types {
            let utils = derived_data_utils(target_repo.blob_repo(), ty)?;
            derivers.push(utils.derive(ctx.clone(), target_repo.blob_repo().clone(), final_merge));
        }
        derivers.try_for_each(|_| future::ready(Ok(()))).await?;

        // Move bookmark
        self.move_bookmark_conditionally(
            ctx,
            target_repo.blob_repo(),
            target_bookmark.to_string(),
            (target_location, final_merge),
        )
        .await?;

        Ok(final_merge)
    }

    /// For all newly added sources and new versions of those already existing,
    /// applies the right move transformations and merges them all together into
    /// a single commit containing all the new stuff ready to be merged into the
    /// target.
    async fn create_commit_with_new_sources(
        &self,
        ctx: &CoreContext,
        repo: &RepoContext,
        diff: &SyncTargetConfigChanges,
        changesets_to_merge: &BTreeMap<SourceName, ChangesetId>,
        sync_config_version: SyncConfigVersion,
        message: Option<String>,
    ) -> Result<Option<ChangesetId>, MegarepoError> {
        if diff.added.is_empty() {
            return Ok(None);
        }

        let sources_to_add: Vec<_> = diff
            .added
            .iter()
            .map(|(source, _cs_id)| source.clone())
            .collect();
        let moved_commits = self
            .create_move_commits(ctx, repo.blob_repo(), &sources_to_add, changesets_to_merge)
            .await?;

        if moved_commits.len() == 1 {
            return Ok(Some(moved_commits[0].1.moved.get_changeset_id()));
        }

        // Now let's merge all the moved commits together
        Ok(Some(
            self.create_merge_commits(
                ctx,
                repo.blob_repo(),
                moved_commits,
                false, /* write_commit_remapping_state */
                sync_config_version,
                message,
            )
            .await?,
        ))
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
        diff: &SyncTargetConfigChanges,
        message: Option<String>,
        additions_merge: &Option<ChangesetContext>,
        old_target_cs: &ChangesetContext,
        state: &CommitRemappingState,
        new_version: String,
    ) -> Result<ChangesetId, MegarepoError> {
        let mut all_removed_files = HashSet::new();
        for (source, source_cs_id) in &diff.removed {
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

        let p1 = maybe_deletion_commit.unwrap_or(old_target_cs.id());

        let mut parents = vec![p1];
        // Verify that none of the files that will be merged in collides
        // with what's already in the target.
        if let Some(additions_merge) = additions_merge {
            self.verify_no_file_conflicts(repo, additions_merge, p1)
                .await?;

            parents.push(additions_merge.id())
        }

        let mut bcs = BonsaiChangesetMut {
            parents,
            author: "svcscm".to_string(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message: message
                .clone()
                .unwrap_or("target config change".to_string()),
            extra: SortedVectorMap::new(),
            file_changes: SortedVectorMap::new(),
        };
        state
            .save_in_changeset(ctx, repo.blob_repo(), &mut bcs)
            .await?;
        let merge = bcs.freeze()?;
        save_bonsai_changesets(vec![merge.clone()], ctx.clone(), repo.blob_repo().clone()).await?;

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
            .find_files(None, None)
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

    async fn create_deletion_commit(
        &self,
        ctx: &CoreContext,
        repo: &RepoContext,
        old_target_cs: &ChangesetContext,
        removed_files: HashSet<MPath>,
        new_version: String,
    ) -> Result<ChangesetId, MegarepoError> {
        let file_changes = removed_files
            .into_iter()
            .map(|path| (path, FileChange::Deletion))
            .collect();
        let old_target_with_removed_files = BonsaiChangesetMut {
            parents: vec![old_target_cs.id()],
            author: "svcscm".to_string(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message: format!("Deletion commit for {}", new_version),
            extra: SortedVectorMap::new(),
            file_changes,
        };
        let old_target_with_removed_files = old_target_with_removed_files.freeze()?;
        save_bonsai_changesets(
            vec![old_target_with_removed_files.clone()],
            ctx.clone(),
            repo.blob_repo().clone(),
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
            .find_files(None, None)
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
            let hg_cs_id = blob_repo
                .get_hg_from_bonsai_changeset(ctx.clone(), merge_commit)
                .await?;
            let hg_cs = hg_cs_id.load(ctx, blob_repo.blobstore()).await?;
            Ok(hg_cs.manifestid())
        };
        let parent_hg_css = try_join_all(new_parent_commits.iter().map(|p| async move {
            let hg_cs_id = blob_repo
                .get_hg_from_bonsai_changeset(ctx.clone(), *p)
                .await?;
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
            message: message.unwrap_or("target config change".to_string()),
            extra: SortedVectorMap::new(),
            file_changes: file_changes.into_iter().collect(),
        };
        let merge = bcs.freeze()?;
        save_bonsai_changesets(vec![merge.clone()], ctx.clone(), repo.blob_repo().clone()).await?;

        Ok(merge.get_changeset_id())
    }

    // If that change_target_config() call was successful, but failed to send
    // successful result to the client (e.g. network issues) then
    // client will retry a request. We need to detect this situation and
    // send a successful response to the client.
    async fn check_if_this_method_has_already_succeeded(
        &self,
        ctx: &CoreContext,
        new_version: &SyncConfigVersion,
        (expected_target_location, actual_target_location): (ChangesetId, ChangesetId),
        changesets_to_merge: &BTreeMap<SourceName, ChangesetId>,
        repo: &RepoContext,
    ) -> Result<ChangesetId, MegarepoError> {
        // Bookmark points a non-expected commit - let's see if changeset it points to was created
        // by a previous change_target_config call

        // Check that first parent is a target location
        let parents = repo
            .blob_repo()
            .get_changeset_parents_by_bonsai(ctx.clone(), actual_target_location)
            .await?;
        if parents.get(0) != Some(&expected_target_location) {
            return Err(MegarepoError::request(anyhow!(
                "Neither {} nor its first parent {:?} point to a target location {}",
                actual_target_location,
                parents.get(0),
                expected_target_location,
            )));
        }

        self.check_if_commit_has_expected_remapping_state(
            ctx,
            actual_target_location,
            new_version,
            changesets_to_merge,
            repo,
        )
        .await?;

        Ok(actual_target_location)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::megarepo_test_utils::SyncTargetConfigBuilder;
    use anyhow::Error;
    use maplit::btreemap;
    use megarepo_config::Target;
    use mononoke_types::RepositoryId;
    use mononoke_types_mocks::changesetid::{ONES_CSID, THREES_CSID, TWOS_CSID};

    fn source_names(sources: &[(Source, ChangesetId)]) -> Vec<String> {
        sources
            .iter()
            .map(|(source, _cs_id)| source.source_name.clone())
            .collect()
    }

    #[test]
    fn test_diff_configs() -> Result<(), Error> {
        let repo_id = RepositoryId::new(1);
        let target = Target {
            repo_id: repo_id.id() as i64,
            bookmark: "target".to_string(),
        };

        let removed_source = SourceName::new("removed_source");
        let added_source = SourceName::new("added_source");
        let unchanged_source = SourceName::new("unchanged_source");
        let changed_source = SourceName::new("changed_source");
        let version_old = "version_old".to_string();
        let version_new = "version_old".to_string();
        let config_old = SyncTargetConfigBuilder::new(repo_id, target.clone(), version_old.clone())
            .source_builder(removed_source.clone())
            .set_prefix_bookmark_to_source_name()
            .build_source()?
            .source_builder(unchanged_source.clone())
            .set_prefix_bookmark_to_source_name()
            .build_source()?
            .source_builder(changed_source.clone())
            .set_prefix_bookmark_to_source_name()
            .build_source()?
            .no_storage_build();

        let old_changesets = btreemap! {
            removed_source.clone() => ONES_CSID,
            changed_source.clone() =>ONES_CSID,
            unchanged_source.clone() =>ONES_CSID,
        };

        let config_new = SyncTargetConfigBuilder::new(repo_id, target.clone(), version_new.clone())
            .source_builder(added_source.clone())
            .set_prefix_bookmark_to_source_name()
            .build_source()?
            .source_builder(unchanged_source.clone())
            .set_prefix_bookmark_to_source_name()
            .build_source()?
            .source_builder(changed_source.clone())
            .set_prefix_bookmark_to_source_name()
            .linkfile("first", "linkfiles/first")
            .build_source()?
            .no_storage_build();


        let new_changesets = btreemap! {
            added_source.clone() => TWOS_CSID,
            changed_source.clone() => THREES_CSID,
            unchanged_source.clone() => ONES_CSID,
        };

        let diff = diff_configs(&config_old, &old_changesets, &config_new, &new_changesets)?;

        assert_eq!(
            source_names(&diff.added),
            vec!["added_source", "changed_source"]
        );
        assert_eq!(
            source_names(&diff.removed),
            vec!["changed_source", "removed_source"]
        );
        Ok(())
    }
}
