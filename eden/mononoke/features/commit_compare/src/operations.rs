/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;

use anyhow::Result;
use context::CoreContext;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use itertools::Either;
use itertools::Itertools;
use maplit::btreeset;
use mononoke_api::ChangesetContext;
use mononoke_api::ChangesetDiffItem;
use mononoke_api::ChangesetFileOrdering;
use mononoke_api::MononokeError;
use mononoke_api::Repo;
use mononoke_api::RepoContext;
use mononoke_types::path::MPath;
use source_control as source_control_thrift;

use crate::commit_compare_path::CommitComparePath;
use crate::commit_compare_path::add_mutable_renames;
use crate::commit_compare_path::find_commit_compare_parent;
use crate::identity::map_commit_identity;

// Magic number used when we want to limit concurrency with buffered.
const CONCURRENCY_LIMIT: usize = 100;

/// Result of `commit_compare` including both the thrift response and
/// the number of diff entries, which callers can use for admission
/// control or observability.
pub struct CommitCompareResult {
    pub response: source_control_thrift::CommitCompareResponse,
    pub diff_count: usize,
}

/// Returns the maximum number of paths allowed in an unordered commit_compare,
/// read from JustKnobs (`scm/mononoke:commit_compare_unordered_max_paths`).
pub fn unordered_max_paths() -> Result<usize> {
    justknobs::get_as::<usize>("scm/mononoke:commit_compare_unordered_max_paths", None)
}

/// Core commit_compare logic, shared between SCS and diff_service.
///
/// Takes resolved changesets and params, performs the diff, and returns
/// the thrift response along with the number of diff entries.
/// The caller is responsible for resolving changesets from whatever input
/// format they receive (CommitSpecifier, CommitId, etc.).
///
/// If `other_changeset` is `None`, the function will attempt to find the
/// parent changeset using `find_commit_compare_parent`.
pub async fn commit_compare(
    ctx: &CoreContext,
    repo: &RepoContext<Repo>,
    mut base_changeset: ChangesetContext<Repo>,
    other_changeset: Option<ChangesetContext<Repo>>,
    params: &source_control_thrift::CommitCompareParams,
) -> Result<CommitCompareResult> {
    add_mutable_renames(&mut base_changeset, params).await?;

    let other_changeset = match other_changeset {
        Some(cs) => Some(cs),
        None => find_commit_compare_parent(repo, &mut base_changeset, params).await?,
    };

    // Log generation difference
    let base_generation = base_changeset.generation().await?.value();
    let other_generation = match other_changeset {
        Some(ref cs) => cs.generation().await?.value(),
        None => base_generation,
    };
    let generation_diff = base_generation.abs_diff(other_generation);
    let mut scuba = ctx.scuba().clone();
    scuba.log_with_msg(
        "Commit compare generation difference",
        format!("{generation_diff}"),
    );

    // Parse diff items
    let mut last_path = None;
    let mut diff_items: BTreeSet<_> = params
        .compare_items
        .iter()
        .filter_map(|item| match *item {
            source_control_thrift::CommitCompareItem::FILES => Some(ChangesetDiffItem::FILES),
            source_control_thrift::CommitCompareItem::TREES => Some(ChangesetDiffItem::TREES),
            _ => None,
        })
        .collect();

    if diff_items.is_empty() {
        diff_items = btreeset! { ChangesetDiffItem::FILES };
    }

    let paths: Option<Vec<MPath>> = match &params.paths {
        None => None,
        Some(paths) => Some(
            paths
                .iter()
                .map(MPath::try_from)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| MononokeError::InvalidRequest(error.to_string()))?,
        ),
    };

    let (diff_count, diff_files, diff_trees) = match params.ordered_params {
        None => {
            let diff = match other_changeset {
                Some(ref other_changeset) => {
                    base_changeset
                        .diff_unordered(
                            other_changeset,
                            !params.skip_copies_renames,
                            params.compare_with_subtree_copy_sources.unwrap_or_default(),
                            paths,
                            diff_items,
                        )
                        .await?
                }
                None => {
                    base_changeset
                        .diff_root_unordered(paths, diff_items)
                        .await?
                }
            };

            let diff_count = diff.len();
            let max_paths = unordered_max_paths()?;
            if diff_count > max_paths {
                return Err(MononokeError::InvalidRequest(format!(
                    "commit_compare: unordered diff has {} entries, exceeding maximum {}. \
                     Use ordered_params with pagination instead.",
                    diff_count, max_paths,
                ))
                .into());
            }

            let (files, trees) = stream::iter(diff)
                .map(|diff| CommitComparePath::from_path_diff(diff, &params.identity_schemes))
                .buffered(CONCURRENCY_LIMIT)
                .try_collect::<Vec<_>>()
                .await?
                .into_iter()
                .partition_map(|diff| match diff {
                    CommitComparePath::File(entry) => Either::Left(entry),
                    CommitComparePath::Tree(entry) => Either::Right(entry),
                });
            (diff_count, files, trees)
        }
        Some(ref ordered_params) => {
            let limit: usize = ordered_params.limit.try_into().map_err(|_| {
                MononokeError::InvalidRequest(format!("invalid limit: {}", ordered_params.limit))
            })?;

            if limit > source_control_thrift::consts::COMMIT_COMPARE_ORDERED_MAX_LIMIT as usize {
                return Err(MononokeError::InvalidRequest(format!(
                    "limit {} exceeds maximum {}",
                    limit,
                    source_control_thrift::consts::COMMIT_COMPARE_ORDERED_MAX_LIMIT
                ))
                .into());
            }

            let after = ordered_params
                .after_path
                .as_ref()
                .map(|after| {
                    MPath::try_from(after).map_err(|e| {
                        MononokeError::InvalidRequest(format!(
                            "invalid continuation path '{}': {}",
                            after, e
                        ))
                    })
                })
                .transpose()?;

            let diff = match other_changeset {
                Some(ref other_changeset) => {
                    base_changeset
                        .diff(
                            other_changeset,
                            !params.skip_copies_renames,
                            params.compare_with_subtree_copy_sources.unwrap_or_default(),
                            paths,
                            diff_items,
                            ChangesetFileOrdering::Ordered { after },
                            Some(limit),
                        )
                        .await?
                }
                None => {
                    base_changeset
                        .diff_root(
                            paths,
                            diff_items,
                            ChangesetFileOrdering::Ordered { after },
                            Some(limit),
                        )
                        .await?
                }
            };

            let diff_count = diff.len();

            let diff_items: Vec<CommitComparePath> = stream::iter(diff)
                .map(|diff| CommitComparePath::from_path_diff(diff, &params.identity_schemes))
                .buffered(CONCURRENCY_LIMIT)
                .try_collect()
                .await?;

            if diff_items.len() >= limit {
                if let Some(item) = diff_items.last() {
                    last_path = Some(item.path()?.to_string());
                }
            }

            let (files, trees) = diff_items.into_iter().partition_map(|diff| match diff {
                CommitComparePath::File(entry) => Either::Left(entry),
                CommitComparePath::Tree(entry) => Either::Right(entry),
            });
            (diff_count, files, trees)
        }
    };

    let other_commit_ids = match other_changeset {
        None => None,
        Some(other_changeset) => {
            let is_snapshot = other_changeset.bonsai_changeset().await?.is_snapshot();
            let schemes = if is_snapshot {
                BTreeSet::from([source_control_thrift::CommitIdentityScheme::BONSAI])
            } else {
                params.identity_schemes.clone()
            };
            Some(map_commit_identity(&other_changeset, &schemes).await?)
        }
    };

    Ok(CommitCompareResult {
        response: source_control_thrift::CommitCompareResponse {
            diff_files,
            diff_trees,
            other_commit_ids,
            last_path,
            ..Default::default()
        },
        diff_count,
    })
}
