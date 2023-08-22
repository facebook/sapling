/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Error;
use blobrepo::save_bonsai_changesets;
use blobrepo_utils::convert_diff_result_into_file_change_for_diamond_merge;
use blobstore::Loadable;
use blobsync::copy_content;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bookmarks::BookmarksRef;
use borrowed::borrowed;
use changeset_fetcher::ChangesetFetcherArc;
use changesets::ChangesetsRef;
use cloned::cloned;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use filestore::FilestoreConfigRef;
use futures::future::try_join_all;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use manifest::get_implicit_deletes;
use megarepo_configs::types::SourceMappingRules;
use mercurial_derivation::DeriveHgChangeset;
use mercurial_types::HgManifestId;
use mononoke_types::mpath_element_iter;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::FileChange;
use mononoke_types::MPath;
use mononoke_types::TrackedFileChange;
use pushrebase::find_bonsai_diff;
use repo_blobstore::RepoBlobstoreArc;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use slog::info;
use slog::Logger;
use sorted_vector_map::SortedVectorMap;
use thiserror::Error;
use tunables::tunables;

pub type MultiMover = Arc<dyn Fn(&MPath) -> Result<Vec<MPath>, Error> + Send + Sync + 'static>;
pub type DirectoryMultiMover =
    Arc<dyn Fn(&Option<MPath>) -> Result<Vec<Option<MPath>>, Error> + Send + Sync + 'static>;

pub trait Repo = RepoIdentityRef
    + RepoBlobstoreArc
    + ChangesetsRef
    + ChangesetFetcherArc
    + BookmarksRef
    + BonsaiHgMappingRef
    + RepoDerivedDataRef
    + RepoBlobstoreRef
    + CommitGraphRef
    + Send
    + Sync;

const SQUASH_DELIMITER_MESSAGE: &str = r#"

============================

This commit created by squashing the following git commits:
"#;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Remapped commit {0} expected in target repo, but not present")]
    MissingRemappedCommit(ChangesetId),
    #[error(
        "Can't reoder changesets parents to put {0} first because it's not a changeset's parent."
    )]
    MissingForcedParent(ChangesetId),
}

pub fn create_source_to_target_multi_mover(
    mapping_rules: SourceMappingRules,
) -> Result<MultiMover, Error> {
    // We apply the longest prefix first
    let mut overrides = mapping_rules.overrides.into_iter().collect::<Vec<_>>();
    overrides.sort_unstable_by_key(|(ref prefix, _)| prefix.len());
    overrides.reverse();
    let prefix = MPath::new_opt(mapping_rules.default_prefix)?;

    Ok(Arc::new(move |path: &MPath| -> Result<Vec<MPath>, Error> {
        for (override_prefix_src, dsts) in &overrides {
            let override_prefix_src = MPath::new(override_prefix_src.clone())?;
            if override_prefix_src.is_prefix_of(path) {
                let suffix: Vec<_> = path
                    .into_iter()
                    .skip(override_prefix_src.num_components())
                    .collect();

                return dsts
                    .iter()
                    .map(|dst| {
                        let override_prefix = MPath::new_opt(dst)?;
                        MPath::join_opt(override_prefix.as_ref(), suffix.clone())
                            .ok_or_else(|| anyhow!("unexpected empty path"))
                    })
                    .collect::<Result<_, _>>();
            }
        }

        Ok(vec![
            MPath::join_opt(prefix.as_ref(), path)
                .ok_or_else(|| anyhow!("unexpected empty path"))?,
        ])
    }))
}

pub fn create_directory_source_to_target_multi_mover(
    mapping_rules: SourceMappingRules,
) -> Result<DirectoryMultiMover, Error> {
    // We apply the longest prefix first
    let mut overrides = mapping_rules.overrides.into_iter().collect::<Vec<_>>();
    overrides.sort_unstable_by_key(|(ref prefix, _)| prefix.len());
    overrides.reverse();
    let prefix = MPath::new_opt(mapping_rules.default_prefix)?;

    Ok(Arc::new(
        move |path: &Option<MPath>| -> Result<Vec<Option<MPath>>, Error> {
            for (override_prefix_src, dsts) in &overrides {
                let override_prefix_src = MPath::new(override_prefix_src.clone())?;
                if override_prefix_src.is_prefix_of(mpath_element_iter(path)) {
                    let suffix: Vec<_> = mpath_element_iter(path)
                        .skip(override_prefix_src.num_components())
                        .collect();

                    return dsts
                        .iter()
                        .map(|dst| {
                            let override_prefix = MPath::new_opt(dst)?;
                            Ok(MPath::join_opt(override_prefix.as_ref(), suffix.clone()))
                        })
                        .collect::<Result<_, _>>();
                }
            }

            Ok(vec![MPath::join_opt(
                prefix.as_ref(),
                mpath_element_iter(path),
            )])
        },
    ))
}

/// Get `HgManifestId`s for a set of `ChangesetId`s
/// This is needed for the purposes of implicit delete detection
async fn get_manifest_ids<'a, I: IntoIterator<Item = ChangesetId>>(
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
    bcs_ids: I,
) -> Result<Vec<HgManifestId>, Error> {
    try_join_all(bcs_ids.into_iter().map({
        |bcs_id| {
            cloned!(ctx, repo);
            async move {
                let cs_id = repo.derive_hg_changeset(&ctx, bcs_id).await?;
                let hg_blob_changeset = cs_id.load(&ctx, repo.repo_blobstore()).await?;
                Ok(hg_blob_changeset.manifestid())
            }
        }
    }))
    .await
}

/// Take an iterator of file changes, which may contain implicit deletes
/// and produce a `SortedVectorMap` suitable to be used in the `BonsaiChangeset`,
/// without any implicit deletes.
fn minimize_file_change_set<I: IntoIterator<Item = (MPath, FileChange)>>(
    file_changes: I,
) -> SortedVectorMap<MPath, FileChange> {
    let (adds, removes): (Vec<_>, Vec<_>) = file_changes
        .into_iter()
        .partition(|(_, fc)| fc.is_changed());
    let adds: HashMap<MPath, FileChange> = adds.into_iter().collect();

    let prefix_path_was_added = |removed_path: MPath| {
        removed_path
            .into_parent_dir_iter()
            .any(|parent_dir| adds.contains_key(&parent_dir))
    };

    let filtered_removes = removes
        .into_iter()
        .filter(|(ref mpath, _)| !prefix_path_was_added(mpath.clone()));
    let mut result: SortedVectorMap<_, _> = filtered_removes.collect();
    result.extend(adds.into_iter());
    result
}

/// Given a changeset and it's parents, get the list of file
/// changes, which arise from "implicit deletes" as opposed
/// to naive `MPath` rewriting in `cs.file_changes`. For
/// more information about implicit deletes, please see
/// `manifest/src/implici_deletes.rs`
async fn get_renamed_implicit_deletes<'a, I: IntoIterator<Item = ChangesetId>>(
    ctx: &'a CoreContext,
    cs: BonsaiChangesetMut,
    parent_changeset_ids: I,
    mover: MultiMover,
    source_repo: &'a impl Repo,
) -> Result<Vec<Vec<MPath>>, Error> {
    let parent_manifest_ids = get_manifest_ids(ctx, source_repo, parent_changeset_ids).await?;
    let file_adds: Vec<_> = cs
        .file_changes
        .iter()
        .filter_map(|(mpath, file_change)| file_change.is_changed().then(|| mpath.clone()))
        .collect();
    let store = source_repo.repo_blobstore().clone();
    let implicit_deletes: Vec<MPath> =
        get_implicit_deletes(ctx, store, file_adds, parent_manifest_ids)
            .try_collect()
            .await?;
    implicit_deletes.iter().map(|mpath| mover(mpath)).collect()
}

/// Determines what to do in commits rewriting to empty commit in small repo.
///
/// NOTE: The empty commits from large repo are kept regardless of this flag.
#[derive(PartialEq, Debug, Copy, Clone, Default)]
pub enum CommitRewrittenToEmpty {
    Keep,
    #[default]
    Discard,
}

/// Determines what to do with commits that are empty in large repo.  They may
/// be useful to keep them in small repo if they have some special meaning.
///
/// NOTE: This flag doesn't affect non-empty commits from large repo rewriting
/// to empty commits in small repo. Use CommitsRewrittenToEmpty to control that.
#[derive(PartialEq, Debug, Copy, Clone, Default)]
pub enum EmptyCommitFromLargeRepo {
    #[default]
    Keep,
    Discard,
}

#[derive(PartialEq, Debug, Copy, Clone, Default)]
pub struct RewriteOpts {
    pub commit_rewritten_to_empty: CommitRewrittenToEmpty,
    pub empty_commit_from_large_repo: EmptyCommitFromLargeRepo,
}

/// Create a version of `cs` with `Mover` applied to all changes
/// The return value can be:
/// - `Err` if the rewrite failed
/// - `Ok(None)` if the rewrite decided that this commit should
///              not be present in the rewrite target
/// - `Ok(Some(rewritten))` for a successful rewrite, which should be
///                         present in the rewrite target
/// The notion that the commit "should not be present in the rewrite
/// target" means that the commit is not a merge and all of its changes
/// were rewritten into nothingness by the `Mover`.
///
/// Precondition: this function expects all `cs` parents to be present
/// in `remapped_parents` as keys, and their remapped versions as values.
///
/// If `force_first_parent` is set commit parents are reordered to ensure that
/// the specified changeset comes first.
pub async fn rewrite_commit<'a>(
    ctx: &'a CoreContext,
    cs: BonsaiChangesetMut,
    remapped_parents: &'a HashMap<ChangesetId, ChangesetId>,
    mover: MultiMover,
    source_repo: &'a impl Repo,
    force_first_parent: Option<ChangesetId>,
    rewrite_opts: RewriteOpts,
) -> Result<Option<BonsaiChangesetMut>, Error> {
    let renamed_implicit_deletes = if !cs.file_changes.is_empty() {
        get_renamed_implicit_deletes(
            ctx,
            cs.clone(),
            remapped_parents.keys().cloned(),
            mover.clone(),
            source_repo,
        )
        .await?
    } else {
        vec![]
    };

    internal_rewrite_commit_with_implicit_deletes(
        ctx.logger(),
        cs,
        remapped_parents,
        mover,
        force_first_parent,
        renamed_implicit_deletes,
        rewrite_opts,
        source_repo,
    )
}

pub async fn rewrite_as_squashed_commit<'a>(
    ctx: &'a CoreContext,
    source_repo: &'a impl Repo,
    source_cs_id: ChangesetId,
    (source_parent_cs_id, target_parent_cs_id): (ChangesetId, ChangesetId),
    mut cs: BonsaiChangesetMut,
    mover: MultiMover,
    side_commits_info: Vec<String>,
) -> Result<Option<BonsaiChangesetMut>, Error> {
    let diff_stream = find_bonsai_diff(ctx, source_repo, source_parent_cs_id, source_cs_id).await?;
    let diff_changes: Vec<_> = diff_stream
        .map_ok(|diff_result| async move {
            convert_diff_result_into_file_change_for_diamond_merge(ctx, source_repo, diff_result)
                .await
        })
        .try_buffered(100)
        .try_collect()
        .await?;

    let rewritten_changes = diff_changes
        .into_iter()
        .map(|(path, change)| {
            let new_paths = mover(&path)?;
            Ok(new_paths
                .into_iter()
                .map(|new_path| (new_path, change.clone()))
                .collect())
        })
        .collect::<Result<Vec<Vec<_>>, Error>>()?;

    let rewritten_changes: SortedVectorMap<_, _> = rewritten_changes
        .into_iter()
        .flat_map(|changes| changes.into_iter())
        .collect();

    cs.file_changes = rewritten_changes;
    // `validate_can_sync_changeset` already ensures
    // that target_parent_cs_id is one of the existing parents
    cs.parents = vec![target_parent_cs_id];
    let old_message = cs.message;
    cs.message = format!(
        "{}{}{}",
        old_message,
        SQUASH_DELIMITER_MESSAGE,
        side_commits_info.join("\n")
    );
    Ok(Some(cs))
}

pub async fn rewrite_stack_no_merges<'a>(
    ctx: &'a CoreContext,
    css: Vec<BonsaiChangeset>,
    mut rewritten_parent: ChangesetId,
    mover: MultiMover,
    source_repo: &'a impl Repo,
    force_first_parent: Option<ChangesetId>,
    mut modify_bonsai_cs: impl FnMut((ChangesetId, BonsaiChangesetMut)) -> BonsaiChangesetMut,
) -> Result<Vec<Option<BonsaiChangeset>>, Error> {
    borrowed!(mover: &Arc<_>, source_repo);

    for cs in &css {
        if cs.is_merge() {
            return Err(anyhow!(
                "cannot remap merges in a stack - {}",
                cs.get_changeset_id()
            ));
        }
    }

    let css = stream::iter(css)
        .map({
            |cs| async move {
                let deleted_file_changes = if cs.file_changes().next().is_some() {
                    let parents = cs.parents();
                    get_renamed_implicit_deletes(
                        ctx,
                        cs.clone().into_mut(),
                        parents,
                        mover.clone(),
                        source_repo,
                    )
                    .await?
                } else {
                    vec![]
                };

                anyhow::Ok((cs, deleted_file_changes))
            }
        })
        .buffered(100)
        .try_collect::<Vec<_>>()
        .await?;

    let mut res = vec![];
    for (from_cs, renamed_implicit_deletes) in css {
        let from_cs_id = from_cs.get_changeset_id();
        let from_cs = from_cs.into_mut();

        let mut remapped_parents = HashMap::new();
        if let Some(parent) = from_cs.parents.get(0) {
            remapped_parents.insert(*parent, rewritten_parent);
        }

        let maybe_cs = internal_rewrite_commit_with_implicit_deletes(
            ctx.logger(),
            from_cs,
            &remapped_parents,
            mover.clone(),
            force_first_parent,
            renamed_implicit_deletes,
            Default::default(),
            source_repo,
        )?;

        let maybe_cs = maybe_cs
            .map(|cs| modify_bonsai_cs((from_cs_id, cs)))
            .map(|bcs| bcs.freeze())
            .transpose()?;
        if let Some(ref cs) = maybe_cs {
            let to_cs_id = cs.get_changeset_id();
            rewritten_parent = to_cs_id;
        }

        res.push(maybe_cs);
    }

    Ok(res)
}

pub fn internal_rewrite_commit_with_implicit_deletes<'a>(
    logger: &Logger,
    mut cs: BonsaiChangesetMut,
    remapped_parents: &'a HashMap<ChangesetId, ChangesetId>,
    mover: MultiMover,
    force_first_parent: Option<ChangesetId>,
    renamed_implicit_deletes: Vec<Vec<MPath>>,
    rewrite_opts: RewriteOpts,
    source_repo: &impl RepoIdentityRef,
) -> Result<Option<BonsaiChangesetMut>, Error> {
    let empty_commit = cs.file_changes.is_empty();
    if !empty_commit
        || rewrite_opts.empty_commit_from_large_repo == EmptyCommitFromLargeRepo::Discard
    {
        let path_rewritten_changes = cs
            .file_changes
            .iter()
            .map(|(path, change)| {
                // Just rewrite copy_from information, when we have it
                fn rewrite_copy_from(
                    copy_from: &(MPath, ChangesetId),
                    remapped_parents: &HashMap<ChangesetId, ChangesetId>,
                    mover: MultiMover,
                ) -> Result<Option<(MPath, ChangesetId)>, Error> {
                    let (path, copy_from_commit) = copy_from;
                    let new_paths = mover(path)?;
                    let copy_from_commit =
                        remapped_parents.get(copy_from_commit).ok_or_else(|| {
                            Error::from(ErrorKind::MissingRemappedCommit(*copy_from_commit))
                        })?;

                    // If the source path doesn't remap, drop this copy info.

                    // TODO(stash): a path can be remapped to multiple other paths,
                    // but for copy_from path we pick only the first one. Instead of
                    // picking only the first one, it's a better to have a dedicated
                    // field in a thrift struct which says which path should be picked
                    // as copy from
                    Ok(new_paths
                        .get(0)
                        .cloned()
                        .map(|new_path| (new_path, *copy_from_commit)))
                }

                // Extract any copy_from information, and use rewrite_copy_from on it
                fn rewrite_file_change(
                    change: &TrackedFileChange,
                    remapped_parents: &HashMap<ChangesetId, ChangesetId>,
                    mover: MultiMover,
                ) -> Result<FileChange, Error> {
                    let new_copy_from = change
                        .copy_from()
                        .and_then(|copy_from| {
                            rewrite_copy_from(copy_from, remapped_parents, mover).transpose()
                        })
                        .transpose()?;

                    Ok(FileChange::Change(change.with_new_copy_from(new_copy_from)))
                }

                // Rewrite both path and changes
                fn do_rewrite(
                    path: &MPath,
                    change: &FileChange,
                    remapped_parents: &HashMap<ChangesetId, ChangesetId>,
                    mover: MultiMover,
                ) -> Result<Vec<(MPath, FileChange)>, Error> {
                    let new_paths = mover(path)?;
                    let change = match change {
                        FileChange::Change(tc) => {
                            rewrite_file_change(tc, remapped_parents, mover.clone())?
                        }
                        FileChange::Deletion => FileChange::Deletion,
                        FileChange::UntrackedDeletion | FileChange::UntrackedChange(_) => {
                            bail!("Can't rewrite untracked changes")
                        }
                    };
                    Ok(new_paths
                        .into_iter()
                        .map(|new_path| (new_path, change.clone()))
                        .collect())
                }
                do_rewrite(path, change, remapped_parents, mover.clone())
            })
            .collect::<Result<Vec<Vec<_>>, _>>()?;

        // If any file change in the source bonsai changeset has no equivalent file change in the destination
        // changeset, the conversion was lossy
        if path_rewritten_changes
            .iter()
            .any(|rewritten| rewritten.is_empty())
        {
            mark_as_created_by_lossy_conversion(
                logger,
                &mut cs,
                LossyConversionReason::FileChanges,
                source_repo,
            );
        // If any implicit delete in the source bonsai changeset has no equivalent file change in the destination
        // changeset, the conversion was lossy
        } else if renamed_implicit_deletes
            .iter()
            .any(|rewritten| rewritten.is_empty())
        {
            mark_as_created_by_lossy_conversion(
                logger,
                &mut cs,
                LossyConversionReason::ImplicitFileChanges,
                source_repo,
            );
        }

        let mut path_rewritten_changes: SortedVectorMap<_, _> = path_rewritten_changes
            .into_iter()
            .flat_map(|changes| changes.into_iter())
            .collect();

        let implicit_delete_file_changes: Vec<(MPath, FileChange)> = renamed_implicit_deletes
            .into_iter()
            .flatten()
            .map(|implicit_delete_mpath| (implicit_delete_mpath, FileChange::Deletion))
            .collect();
        path_rewritten_changes.extend(implicit_delete_file_changes.into_iter());
        let path_rewritten_changes = minimize_file_change_set(path_rewritten_changes.into_iter());

        let is_merge = cs.parents.len() >= 2;

        // If all parent has < 2 commits then it's not a merge, and it was completely rewritten
        // out. In that case we can just discard it because there are not changes to the working copy.
        // However if it's a merge then we can't discard it, because even
        // though bonsai merge commit might not have file changes inside it can still change
        // a working copy. E.g. if p1 has fileA, p2 has fileB, then empty merge(p1, p2)
        // contains both fileA and fileB.
        if !is_merge
            && ((path_rewritten_changes.is_empty()
                && rewrite_opts.commit_rewritten_to_empty == CommitRewrittenToEmpty::Discard)
                || (empty_commit
                    && rewrite_opts.empty_commit_from_large_repo
                        == EmptyCommitFromLargeRepo::Discard))
        {
            return Ok(None);
        } else {
            cs.file_changes = path_rewritten_changes;
        }
    }

    // Update hashes
    for commit in cs.parents.iter_mut() {
        let remapped = remapped_parents
            .get(commit)
            .ok_or_else(|| Error::from(ErrorKind::MissingRemappedCommit(*commit)))?;

        *commit = *remapped;
    }
    if let Some(first_parent) = force_first_parent {
        if !cs.parents.contains(&first_parent) {
            return Err(Error::from(ErrorKind::MissingForcedParent(first_parent)));
        }
        let mut new_parents = vec![first_parent];
        new_parents.extend(cs.parents.into_iter().filter(|cs| *cs != first_parent));
        cs.parents = new_parents
    }

    Ok(Some(cs))
}

enum LossyConversionReason {
    FileChanges,
    ImplicitFileChanges,
}

fn mark_as_created_by_lossy_conversion(
    logger: &Logger,
    cs: &mut BonsaiChangesetMut,
    reason: LossyConversionReason,
    source_repo: &impl RepoIdentityRef,
) {
    if tunables().by_repo_cross_repo_mark_changesets_as_created_by_lossy_conversion(
        source_repo.repo_identity().name(),
    ) == Some(true)
    {
        let reason = match reason {
            LossyConversionReason::FileChanges => {
                "the file changes from the source changeset don't all have an equivalent file change in the target changeset"
            }
            LossyConversionReason::ImplicitFileChanges => {
                "implicit file changes from the source changeset don't all have an equivalent implicit file change in the target changeset"
            }
        };
        info!(
            logger,
            "Marking changeset as created by lossy conversion because {}", reason
        );
        cs.hg_extra
            .insert("created_by_lossy_conversion".to_string(), Vec::new());
    }
}

pub async fn upload_commits<'a>(
    ctx: &'a CoreContext,
    rewritten_list: Vec<BonsaiChangeset>,
    source_repo: &'a (impl RepoBlobstoreRef + ChangesetsRef),
    target_repo: &'a (impl RepoBlobstoreRef + ChangesetsRef + FilestoreConfigRef),
) -> Result<(), Error> {
    let mut files_to_sync = vec![];
    for rewritten in &rewritten_list {
        let rewritten_mut = rewritten.clone().into_mut();
        let new_files_to_sync =
            rewritten_mut
                .file_changes
                .values()
                .filter_map(|change| match change {
                    FileChange::Change(tc) => Some(tc.content_id()),
                    FileChange::UntrackedChange(uc) => Some(uc.content_id()),
                    FileChange::Deletion | FileChange::UntrackedDeletion => None,
                });
        files_to_sync.extend(new_files_to_sync);
    }
    copy_file_contents(ctx, source_repo, target_repo, files_to_sync, |_| {}).await?;
    save_bonsai_changesets(rewritten_list.clone(), ctx.clone(), target_repo).await?;
    Ok(())
}

pub async fn copy_file_contents<'a>(
    ctx: &'a CoreContext,
    source_repo: &'a impl RepoBlobstoreRef,
    target_repo: &'a (impl RepoBlobstoreRef + FilestoreConfigRef),
    content_ids: impl IntoIterator<Item = ContentId>,
    progress_reporter: impl Fn(usize),
) -> Result<(), Error> {
    let source_blobstore = source_repo.repo_blobstore();
    let target_blobstore = target_repo.repo_blobstore();
    let target_filestore_config = target_repo.filestore_config();

    let mut i = 0;
    stream::iter(content_ids.into_iter().map({
        |content_id| {
            copy_content(
                ctx,
                source_blobstore,
                target_blobstore,
                target_filestore_config.clone(),
                content_id,
            )
        }
    }))
    .buffer_unordered(100)
    .try_for_each(|_| {
        i += 1;
        progress_reporter(i);
        async { Ok(()) }
    })
    .await
}

#[cfg(test)]
mod test {
    use std::collections::BTreeMap;

    use anyhow::bail;
    use blobrepo::save_bonsai_changesets;
    use fbinit::FacebookInit;
    use maplit::btreemap;
    use maplit::hashmap;
    use mononoke_types::ContentId;
    use mononoke_types::FileType;
    use test_repo_factory::TestRepoFactory;
    use tests_utils::list_working_copy_utf8;
    use tests_utils::CreateCommitContext;
    use tunables::MononokeTunables;

    use super::*;

    #[test]
    fn test_multi_mover_simple() -> Result<(), Error> {
        let mapping_rules = SourceMappingRules {
            default_prefix: "".to_string(),
            ..Default::default()
        };
        let multi_mover = create_source_to_target_multi_mover(mapping_rules)?;
        assert_eq!(
            multi_mover(&MPath::new("path")?)?,
            vec![MPath::new("path")?]
        );
        Ok(())
    }

    #[test]
    fn test_multi_mover_prefixed() -> Result<(), Error> {
        let mapping_rules = SourceMappingRules {
            default_prefix: "prefix".to_string(),
            ..Default::default()
        };
        let multi_mover = create_source_to_target_multi_mover(mapping_rules)?;
        assert_eq!(
            multi_mover(&MPath::new("path")?)?,
            vec![MPath::new("prefix/path")?]
        );
        Ok(())
    }

    #[test]
    fn test_multi_mover_prefixed_with_exceptions() -> Result<(), Error> {
        let mapping_rules = SourceMappingRules {
            default_prefix: "prefix".to_string(),
            overrides: btreemap! {
                "override".to_string() => vec![
                    "overriden_1".to_string(),
                    "overriden_2".to_string(),
                ]
            },
            ..Default::default()
        };
        let multi_mover = create_source_to_target_multi_mover(mapping_rules)?;
        assert_eq!(
            multi_mover(&MPath::new("path")?)?,
            vec![MPath::new("prefix/path")?]
        );

        assert_eq!(
            multi_mover(&MPath::new("override/path")?)?,
            vec![
                MPath::new("overriden_1/path")?,
                MPath::new("overriden_2/path")?,
            ]
        );
        Ok(())
    }

    #[test]
    fn test_multi_mover_longest_prefix_first() -> Result<(), Error> {
        let mapping_rules = SourceMappingRules {
            default_prefix: "prefix".to_string(),
            overrides: btreemap! {
                "prefix".to_string() => vec![
                    "prefix_1".to_string(),
                ],
                "prefix/sub".to_string() => vec![
                    "prefix/sub_1".to_string(),
                ]
            },
            ..Default::default()
        };
        let multi_mover = create_source_to_target_multi_mover(mapping_rules)?;
        assert_eq!(
            multi_mover(&MPath::new("prefix/path")?)?,
            vec![MPath::new("prefix_1/path")?]
        );

        assert_eq!(
            multi_mover(&MPath::new("prefix/sub/path")?)?,
            vec![MPath::new("prefix/sub_1/path")?]
        );

        Ok(())
    }

    fn path(p: &str) -> MPath {
        MPath::new(p).unwrap()
    }

    fn verify_minimized(changes: Vec<(&str, Option<()>)>, expected: BTreeMap<&str, Option<()>>) {
        fn to_file_change(o: Option<()>) -> FileChange {
            match o {
                Some(_) => FileChange::tracked(
                    ContentId::from_bytes([1; 32]).unwrap(),
                    FileType::Regular,
                    0,
                    None,
                ),
                None => FileChange::Deletion,
            }
        }
        let changes: Vec<_> = changes
            .into_iter()
            .map(|(p, c)| (path(p), to_file_change(c)))
            .collect();
        let minimized = minimize_file_change_set(changes);
        let expected: SortedVectorMap<MPath, FileChange> = expected
            .into_iter()
            .map(|(p, c)| (path(p), to_file_change(c)))
            .collect();
        assert_eq!(expected, minimized);
    }

    #[fbinit::test]
    fn test_minimize_file_change_set(_fb: FacebookInit) {
        verify_minimized(
            vec![("a", Some(())), ("a", None)],
            btreemap! { "a" => Some(())},
        );
        verify_minimized(vec![("a", Some(()))], btreemap! { "a" => Some(())});
        verify_minimized(vec![("a", None)], btreemap! { "a" => None});
        // directories are deleted implicitly, so explicit deletes are
        // minimized away
        verify_minimized(
            vec![("a/b", None), ("a/c", None), ("a", Some(()))],
            btreemap! { "a" => Some(()) },
        );
        // files, replaced with a directy at a longer path are not
        // deleted implicitly, so they aren't minimized away
        verify_minimized(
            vec![("a", None), ("a/b", Some(()))],
            btreemap! { "a" => None, "a/b" => Some(()) },
        );
    }

    #[fbinit::test]
    async fn test_rewrite_commit_marks_lossy_conversions(fb: FacebookInit) -> Result<(), Error> {
        let repo: blobrepo::BlobRepo = TestRepoFactory::new(fb)?.build().await?;
        let tunables = MononokeTunables::default();
        tunables.update_by_repo_bools(&hashmap! {
            repo.repo_identity().name().to_string() => hashmap! {
                "cross_repo_mark_changesets_as_created_by_lossy_conversion".to_string() => true,
            },
        });
        tunables::override_tunables(Some(Arc::new(tunables)));
        assert_eq!(
            tunables::tunables().by_repo_cross_repo_mark_changesets_as_created_by_lossy_conversion(
                repo.repo_identity().name(),
            ),
            Some(true)
        );
        let ctx = CoreContext::test_mock(fb);
        let mapping_rules = SourceMappingRules {
            default_prefix: "".to_string(), // Rewrite to root
            overrides: btreemap! {
                "www".to_string() => vec!["".to_string()], // map changes to www to root
                "xplat".to_string() => vec![], // swallow changes outside of www
            },
            ..Default::default()
        };
        let multi_mover = create_source_to_target_multi_mover(mapping_rules)?;
        // Add files to www and xplat (lossy)
        let first = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("www/foo.php", "foo content")
            .add_file("www/bar/baz.php", "baz content")
            .add_file("www/bar/crux.php", "crux content")
            .add_file("xplat/a/a.js", "a content")
            .add_file("xplat/a/b.js", "b content")
            .add_file("xplat/b/c.js", "c content")
            .commit()
            .await?;
        // Only add one file in xplat (No changeset will be generated)
        let second = CreateCommitContext::new(&ctx, &repo, vec![first])
            .add_file("xplat/c/d.js", "d content")
            .commit()
            .await?;
        // Only add one file in www (non-lossy)
        let third = CreateCommitContext::new(&ctx, &repo, vec![second])
            .add_file("www/baz/foobar.php", "foobar content")
            .commit()
            .await?;
        // Only change files in www (non-lossy)
        let fourth = CreateCommitContext::new(&ctx, &repo, vec![third])
            .add_file("www/baz/foobar.php", "more foobar content")
            .add_file("www/foo.php", "more foo content")
            .commit()
            .await?;
        // Only delete files in www (non-lossy)
        let fifth = CreateCommitContext::new(&ctx, &repo, vec![fourth])
            .delete_file("www/baz/crux.php")
            .commit()
            .await?;
        // Delete files in www and xplat (lossy)
        let sixth = CreateCommitContext::new(&ctx, &repo, vec![fifth])
            .delete_file("xplat/a/a.js")
            .delete_file("www/bar/baz.php")
            .commit()
            .await?;

        let first_rewritten_bcs_id = test_rewrite_commit_cs_id(
            &ctx,
            &repo,
            first,
            HashMap::new(),
            multi_mover.clone(),
            None,
        )
        .await?;
        assert_changeset_is_marked_lossy(&ctx, &repo, first_rewritten_bcs_id).await?;

        assert!(
            test_rewrite_commit_cs_id(
                &ctx,
                &repo,
                second,
                hashmap! {
                    first => first_rewritten_bcs_id,
                },
                multi_mover.clone(),
                None,
            )
            .await
            .is_err()
        );

        let third_rewritten_bcs_id = test_rewrite_commit_cs_id(
            &ctx,
            &repo,
            third,
            hashmap! {
                second => first_rewritten_bcs_id, // there is no second equivalent
            },
            multi_mover.clone(),
            None,
        )
        .await?;
        assert_changeset_is_not_marked_lossy(&ctx, &repo, third_rewritten_bcs_id).await?;

        let fourth_rewritten_bcs_id = test_rewrite_commit_cs_id(
            &ctx,
            &repo,
            fourth,
            hashmap! {
                third => third_rewritten_bcs_id,
            },
            multi_mover.clone(),
            None,
        )
        .await?;
        assert_changeset_is_not_marked_lossy(&ctx, &repo, fourth_rewritten_bcs_id).await?;

        let fifth_rewritten_bcs_id = test_rewrite_commit_cs_id(
            &ctx,
            &repo,
            fifth,
            hashmap! {
                fourth => fourth_rewritten_bcs_id,
            },
            multi_mover.clone(),
            None,
        )
        .await?;
        assert_changeset_is_not_marked_lossy(&ctx, &repo, fifth_rewritten_bcs_id).await?;

        let sixth_rewritten_bcs_id = test_rewrite_commit_cs_id(
            &ctx,
            &repo,
            sixth,
            hashmap! {
                fifth => fifth_rewritten_bcs_id,
            },
            multi_mover.clone(),
            None,
        )
        .await?;
        assert_changeset_is_marked_lossy(&ctx, &repo, sixth_rewritten_bcs_id).await?;

        Ok(())
    }

    #[fbinit::test]
    async fn test_rewrite_commit_marks_lossy_conversions_with_implicit_deletes(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let repo: blobrepo::BlobRepo = TestRepoFactory::new(fb)?.build().await?;
        let tunables = MononokeTunables::default();
        tunables.update_by_repo_bools(&hashmap! {
            repo.repo_identity().name().to_string() => hashmap! {
                "cross_repo_mark_changesets_as_created_by_lossy_conversion".to_string() => true,
            },
        });
        tunables::override_tunables(Some(Arc::new(tunables)));
        assert_eq!(
            tunables::tunables().by_repo_cross_repo_mark_changesets_as_created_by_lossy_conversion(
                repo.repo_identity().name(),
            ),
            Some(true)
        );
        let ctx = CoreContext::test_mock(fb);
        // This commit is not lossy because all paths will be mapped somewhere.
        let first = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("a/b/c/d", "d")
            .add_file("a/b/c/e", "e")
            .add_file("a/b/c/f/g", "g")
            .add_file("a/b/c/f/h", "h")
            .add_file("a/b/c/i", "i")
            .commit()
            .await?;
        let second = CreateCommitContext::new(&ctx, &repo, vec![first])
            .add_file("a/b/c", "new c") // This creates a file at `a/b/c`, implicitely deleting the directory
            // at `a/b/c` and all the files it contains (`d`, `e`, `f/g` and `f/h`)
            .add_file("a/b/i", "new i")
            .commit()
            .await?;

        // With the full mapping rules, all directories from the repo have a mapping
        let full_mapping_rules = SourceMappingRules {
            default_prefix: "".to_string(),
            overrides: btreemap! {
                "a/b".to_string() => vec!["ab".to_string()],
                "a/b/c".to_string() => vec!["abc".to_string()],
                "a/b/c/f".to_string() => vec!["abcf".to_string()],
            },
            ..Default::default()
        };
        let full_multi_mover = create_source_to_target_multi_mover(full_mapping_rules)?;
        // With the partial mapping rules, files under `a/b/c/f` don't have a mapping
        let partial_mapping_rules = SourceMappingRules {
            default_prefix: "".to_string(),
            overrides: btreemap! {
                "a/b".to_string() => vec!["ab".to_string()],
                "a/b/c".to_string() => vec!["abc".to_string()],
                "a/b/c/f".to_string() => vec![],
            },
            ..Default::default()
        };
        let partial_multi_mover = create_source_to_target_multi_mover(partial_mapping_rules)?;

        // We rewrite the first commit with the full_multi_mover.
        // This is not lossy.
        let first_rewritten_bcs_id = test_rewrite_commit_cs_id(
            &ctx,
            &repo,
            first,
            HashMap::new(),
            full_multi_mover.clone(),
            None,
        )
        .await?;
        assert_changeset_is_not_marked_lossy(&ctx, &repo, first_rewritten_bcs_id).await?;
        // When we rewrite the second commit with the full_multi_mover.
        // This is not lossy:
        // All file changes have a mapping and all implicitely deleted files have a mapping.
        let full_second_rewritten_bcs_id = test_rewrite_commit_cs_id(
            &ctx,
            &repo,
            second,
            hashmap! {
                first => first_rewritten_bcs_id
            },
            full_multi_mover.clone(),
            None,
        )
        .await?;
        assert_changeset_is_not_marked_lossy(&ctx, &repo, full_second_rewritten_bcs_id).await?;
        // When we rewrite the second commit with the partial_multi_mover.
        // This **is** lossy:
        // All file changes have a mapping but some implicitely deleted files don't have a mapping
        // (namely, `a/b/c/f/g` and `a/b/c/f/h`).
        let partial_second_rewritten_bcs_id = test_rewrite_commit_cs_id(
            &ctx,
            &repo,
            second,
            hashmap! {
                first => first_rewritten_bcs_id
            },
            partial_multi_mover.clone(),
            None,
        )
        .await?;
        assert_changeset_is_marked_lossy(&ctx, &repo, partial_second_rewritten_bcs_id).await?;
        Ok(())
    }

    #[fbinit::test]
    async fn test_rewrite_commit(fb: FacebookInit) -> Result<(), Error> {
        let repo: blobrepo::BlobRepo = TestRepoFactory::new(fb)?.build().await?;
        let ctx = CoreContext::test_mock(fb);
        let first = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("path", "path")
            .commit()
            .await?;
        let second = CreateCommitContext::new(&ctx, &repo, vec![first])
            .add_file_with_copy_info("pathsecondcommit", "pathsecondcommit", (first, "path"))
            .commit()
            .await?;
        let third = CreateCommitContext::new(&ctx, &repo, vec![first, second])
            .add_file("path", "pathmodified")
            .commit()
            .await?;

        let mapping_rules = SourceMappingRules {
            default_prefix: "prefix".to_string(),
            overrides: btreemap! {
                "path".to_string() => vec![
                    "path_1".to_string(),
                    "path_2".to_string(),
                ]
            },
            ..Default::default()
        };
        let multi_mover = create_source_to_target_multi_mover(mapping_rules)?;

        let first_rewritten_bcs_id = test_rewrite_commit_cs_id(
            &ctx,
            &repo,
            first,
            HashMap::new(),
            multi_mover.clone(),
            None,
        )
        .await?;

        let first_rewritten_wc =
            list_working_copy_utf8(&ctx, &repo, first_rewritten_bcs_id).await?;
        assert_eq!(
            first_rewritten_wc,
            hashmap! {
                MPath::new("path_1")? => "path".to_string(),
                MPath::new("path_2")? => "path".to_string(),
            }
        );

        let second_rewritten_bcs_id = test_rewrite_commit_cs_id(
            &ctx,
            &repo,
            second,
            hashmap! {
                first => first_rewritten_bcs_id
            },
            multi_mover.clone(),
            None,
        )
        .await?;

        let second_bcs = second_rewritten_bcs_id
            .load(&ctx, &repo.repo_blobstore())
            .await?;
        let maybe_copy_from = match second_bcs
            .file_changes_map()
            .get(&MPath::new("prefix/pathsecondcommit")?)
            .ok_or_else(|| anyhow!("path not found"))?
        {
            FileChange::Change(tc) => tc.copy_from().cloned(),
            _ => bail!("path_is_deleted"),
        };

        assert_eq!(
            maybe_copy_from,
            Some((MPath::new("path_1")?, first_rewritten_bcs_id))
        );

        let second_rewritten_wc =
            list_working_copy_utf8(&ctx, &repo, second_rewritten_bcs_id).await?;
        assert_eq!(
            second_rewritten_wc,
            hashmap! {
                MPath::new("path_1")? => "path".to_string(),
                MPath::new("path_2")? => "path".to_string(),
                MPath::new("prefix/pathsecondcommit")? => "pathsecondcommit".to_string(),
            }
        );

        // Diamond merge test with error during parent reordering
        assert!(
            test_rewrite_commit_cs_id(
                &ctx,
                &repo,
                third,
                hashmap! {
                    first => first_rewritten_bcs_id,
                    second => second_rewritten_bcs_id
                },
                multi_mover.clone(),
                Some(second), // wrong, should be after-rewrite id
            )
            .await
            .is_err()
        );

        // Diamond merge test with success
        let third_rewritten_bcs_id = test_rewrite_commit_cs_id(
            &ctx,
            &repo,
            third,
            hashmap! {
                first => first_rewritten_bcs_id,
                second => second_rewritten_bcs_id
            },
            multi_mover,
            Some(second_rewritten_bcs_id),
        )
        .await?;

        let third_bcs = third_rewritten_bcs_id
            .load(&ctx, &repo.repo_blobstore().clone())
            .await?;

        assert_eq!(
            third_bcs.parents().collect::<Vec<_>>(),
            vec![second_rewritten_bcs_id, first_rewritten_bcs_id],
        );

        Ok(())
    }

    async fn test_rewrite_commit_cs_id<'a>(
        ctx: &'a CoreContext,
        repo: &'a impl Repo,
        bcs_id: ChangesetId,
        parents: HashMap<ChangesetId, ChangesetId>,
        multi_mover: MultiMover,
        force_first_parent: Option<ChangesetId>,
    ) -> Result<ChangesetId, Error> {
        let bcs = bcs_id.load(ctx, &repo.repo_blobstore()).await?;
        let bcs = bcs.into_mut();

        let maybe_rewritten = rewrite_commit(
            ctx,
            bcs,
            &parents,
            multi_mover,
            repo,
            force_first_parent,
            Default::default(),
        )
        .await?;
        let rewritten =
            maybe_rewritten.ok_or_else(|| anyhow!("can't rewrite commit {}", bcs_id))?;
        let rewritten = rewritten.freeze()?;

        save_bonsai_changesets(vec![rewritten.clone()], ctx.clone(), repo).await?;

        Ok(rewritten.get_changeset_id())
    }

    async fn assert_changeset_is_marked_lossy<'a>(
        ctx: &'a CoreContext,
        repo: &'a impl Repo,
        bcs_id: ChangesetId,
    ) -> Result<(), Error> {
        let bcs = bcs_id.load(ctx, &repo.repo_blobstore()).await?;
        assert!(
            bcs.hg_extra()
                .any(|(key, _)| key == "created_by_lossy_conversion"),
            "Failed with {:?}",
            bcs
        );
        Ok(())
    }

    async fn assert_changeset_is_not_marked_lossy<'a>(
        ctx: &'a CoreContext,
        repo: &'a impl Repo,
        bcs_id: ChangesetId,
    ) -> Result<(), Error> {
        let bcs = bcs_id.load(ctx, &repo.repo_blobstore()).await?;
        assert!(
            !bcs.hg_extra()
                .any(|(key, _)| key == "created_by_lossy_conversion"),
            "Failed with {:?}",
            bcs
        );
        Ok(())
    }

    #[test]
    fn test_directory_multi_mover() -> Result<(), Error> {
        let mapping_rules = SourceMappingRules {
            default_prefix: "prefix".to_string(),
            ..Default::default()
        };
        let multi_mover = create_directory_source_to_target_multi_mover(mapping_rules)?;
        assert_eq!(
            multi_mover(&Some(MPath::new("path")?))?,
            vec![Some(MPath::new("prefix/path")?)]
        );

        assert_eq!(multi_mover(&None)?, vec![Some(MPath::new("prefix")?)]);
        Ok(())
    }
}
