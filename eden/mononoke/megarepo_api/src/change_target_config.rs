/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::common::{find_target_bookmark_and_value, find_target_sync_config, MegarepoOp};
use anyhow::anyhow;
use blobrepo::save_bonsai_changesets;
use commit_transformation::create_source_to_target_multi_mover;
use context::CoreContext;
use core::cmp::Ordering;
use derived_data_utils::derived_data_utils;
use futures::future;
use futures::{stream::FuturesUnordered, TryStreamExt};
use itertools::{EitherOrBoth, Itertools};
use megarepo_config::{
    MononokeMegarepoConfigs, Source, SyncConfigVersion, SyncTargetConfig, Target,
};
use megarepo_error::MegarepoError;
use megarepo_mapping::{CommitRemappingState, SourceName};
use mononoke_api::{ChangesetContext, Mononoke, MononokePath, RepoContext};
use mononoke_types::{BonsaiChangesetMut, ChangesetId, DateTime, FileChange};
use sorted_vector_map::SortedVectorMap;
use std::collections::{BTreeMap, HashMap, HashSet};
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
        let (target_bookmark, old_target_cs_id) =
            find_target_bookmark_and_value(&ctx, &target_repo, &target).await?;
        if old_target_cs_id != target_location {
            return Err(MegarepoError::request(anyhow!(
                "Can't change target config because \
                 target_location is set to {} which is different \
                 from actual target location {}.",
                target_location,
                old_target_cs_id,
            )));
        }
        let old_target_cs = &target_repo
            .changeset(old_target_cs_id)
            .await?
            .ok_or_else(|| {
                MegarepoError::internal(anyhow!("programming error - target changeset not found!"))
            })?;
        let (old_remapping_state, old_config) = find_target_sync_config(
            &ctx,
            target_repo.blob_repo(),
            old_target_cs_id,
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
        self.move_bookmark(
            ctx,
            target_repo.blob_repo(),
            target_bookmark.to_string(),
            final_merge,
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

    async fn create_final_merge_commit_with_removals(
        &self,
        ctx: &CoreContext,
        repo: &RepoContext,
        diff: &SyncTargetConfigChanges,
        message: Option<String>,
        additions_merge: &Option<ChangesetContext>,
        old_target_cs: &ChangesetContext,
        state: &CommitRemappingState,
    ) -> Result<ChangesetId, MegarepoError> {
        let mut file_changes = HashMap::new();
        let mut seen_paths = HashSet::new();
        // For each file in each removed source, map it paths in Target it's
        // mapped to and:
        //  * stage it for removal if it's missing in the new target.
        //  * set it's content to the one from additions_cs if it's present there
        //    and different from the one in additions changeset.
        //  * ignore it if it's the same as in additions changeset.
        for (source, source_cs_id) in &diff.removed {
            let paths_in_target_belonging_to_source = self
                .paths_in_target_belonging_to_source(ctx, source, *source_cs_id)
                .await?;

            if let Some(additions_merge) = additions_merge {
                // None means no file change added to bonsai. Some(None) means
                // file deletion will be logged in bosai.
                let changes_for_source: Vec<(MononokePath, Option<Option<FileChange>>)> = additions_merge
                    .paths_with_content(paths_in_target_belonging_to_source.clone().into_iter())
                    .await
                    .map_err(MegarepoError::internal)?
                    .map_err(MegarepoError::internal)
                    .and_then(async move |path_ctx| {
                        let file_type = path_ctx.file_type().await.map_err(MegarepoError::internal)?;
                        let change = if let Some(file) =
                            path_ctx.file().await.map_err(MegarepoError::internal)?
                        {
                            let cs_path_in_old_target = old_target_cs.path_with_content(path_ctx.path().clone())?;
                            let file_id_in_old_target = cs_path_in_old_target.file().await?.ok_or_else(|| {
                                MegarepoError::internal(anyhow!(
                                    "programming error - the path {} should be present in target", path_ctx.path()
                                ))
                            })?.id().await?;
                            let file_type_in_old_target = cs_path_in_old_target.file_type().await?;

                            let content_id = file.id().await?;
                            if content_id == file_id_in_old_target && file_type == file_type_in_old_target {
                                // The content hash is the same on both sides of
                                // the merge, no need to include it in bonsai.
                                return Ok((path_ctx.path().clone(), None));
                            }
                            // File is present in additions and different, let's
                            // resolve the conflict.
                            let change = FileChange::new(
                                content_id,
                                path_ctx.file_type().await?.ok_or_else(|| {
                                    MegarepoError::internal(anyhow!(
                                        "programming error - file type is missing"
                                    ))
                                })?,
                                file.metadata().await?.total_size,
                                None,
                            );
                            Ok::<_, MegarepoError>(Some(change))
                        } else {
                            // Path is present in additions but it is not a file,
                            // let's mark the file it as removed.
                            Ok(None)
                        }?;
                        Ok((path_ctx.path().clone(), Some(change)))
                    })
                    .try_collect()
                    .await?;

                for (path, maybe_change) in changes_for_source {
                    seen_paths.insert(path.clone());
                    if let Some(change) = maybe_change {
                        file_changes.insert(
                            path.into_mpath().ok_or_else(|| {
                                MegarepoError::internal(anyhow!(
                                    "programming error - file mpath can't be None"
                                ))
                            })?,
                            change,
                        );
                    }
                }
            }
            // Mark the paths missing from additions as removed.
            for path in paths_in_target_belonging_to_source {
                if !seen_paths.contains(&path) {
                    file_changes.insert(
                        path.into_mpath().ok_or_else(|| {
                            MegarepoError::internal(anyhow!(
                                "programming error - file mpath can't be None"
                            ))
                        })?,
                        None,
                    );
                }
            }
        }

        let mut parents = vec![old_target_cs.id()];

        // Verify that none of the files that will be merged in collides
        // with what's already in the target.
        if let Some(additions_merge) = additions_merge {
            additions_merge
                .find_files(None, None)
                .await?
                .map_err(MegarepoError::internal)
                .try_for_each({
                    let seen_paths = &seen_paths;
                    async move |path| {
                        if seen_paths.contains(&path) {
                            Ok(())
                        } else if old_target_cs
                            .path_with_content(path.clone())?
                            .exists()
                            .await?
                        {
                            Err(MegarepoError::request(anyhow!(
                                "path {} cannot be added to the target - it's already present",
                                &path
                            )))
                        } else {
                            Ok(())
                        }
                    }
                })
                .await?;
            parents.push(additions_merge.id())
        }

        let mut bcs = BonsaiChangesetMut {
            parents,
            author: "svcscm".to_string(),
            author_date: DateTime::now(),
            committer: None,
            committer_date: None,
            message: message.unwrap_or("target config change".to_string()),
            extra: SortedVectorMap::new(),
            file_changes: file_changes.into_iter().collect(),
        };
        state
            .save_in_changeset(ctx, repo.blob_repo(), &mut bcs)
            .await?;
        let merge = bcs.freeze()?;
        save_bonsai_changesets(vec![merge.clone()], ctx.clone(), repo.blob_repo().clone()).await?;
        Ok(merge.get_changeset_id())
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
