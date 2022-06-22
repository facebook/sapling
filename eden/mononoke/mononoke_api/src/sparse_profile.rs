/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::errors::MononokeError;
use crate::ChangesetContext;
use crate::ChangesetDiffItem;
use crate::ChangesetFileOrdering;
use crate::ChangesetPathContentContext;
use crate::ChangesetPathDiffContext;
use crate::MononokePath;
use crate::PathEntry;
use anyhow::{anyhow, Context, Result};
use blobstore::Loadable;
use borrowed::borrowed;
use cloned::cloned;
use context::CoreContext;
use futures::{stream, try_join, FutureExt, StreamExt, TryStreamExt};
use maplit::btreeset;
use mononoke_types::{fsnode::FsnodeEntry, MPath};
use pathmatcher::{DirectoryMatch, Matcher};
use slog::{error, Logger};
use types::RepoPath;

use std::collections::HashMap;
use std::sync::Arc;

const SPARSE_PROFILES_LOCATION: &str = "tools/scm/sparse";

pub(crate) async fn fetch(path: String, changeset: &ChangesetContext) -> Result<Option<Vec<u8>>> {
    let path: &str = &path;
    let path = MPath::try_from(path)?;
    let path_with_content = changeset.path_with_content(path.clone())?;
    let file_ctx = path_with_content
        .file()
        .await?
        .ok_or_else(|| anyhow!("Sparse profile {} not found", path))?;
    file_ctx
        .content_concat()
        .await
        .with_context(|| format!("Couldn't fetch content of {path}"))
        .map(|b| Some(b.to_vec()))
}

async fn create_matchers(
    changeset: &ChangesetContext,
    paths: Vec<MPath>,
) -> Result<HashMap<String, Arc<dyn Matcher + Send + Sync>>> {
    stream::iter(paths)
        .map(|path| async move {
            let content = format!("%include {path}");
            let dummy_source = "repo_root".to_string();
            let profile = sparse::Root::from_bytes(content.as_bytes(), dummy_source)
                .with_context(|| format!("while constructing Profile for source {path}"))?;
            let matcher = profile
                .matcher(|path| fetch(path, changeset))
                .await
                .with_context(|| format!("While constructing matcher for source {path}"))?;
            anyhow::Ok((
                path.to_string(),
                Arc::new(matcher) as Arc<dyn Matcher + Send + Sync>,
            ))
        })
        .buffer_unordered(100)
        .try_collect()
        .await
}

pub async fn get_profile_size(
    ctx: &CoreContext,
    changeset: &ChangesetContext,
    paths: Vec<MPath>,
) -> Result<Out, MononokeError> {
    let matchers = create_matchers(changeset, paths).await?;
    calculate_size(ctx, changeset, matchers).await
}

type Out = HashMap<String, u64>;

async fn calculate_size<'a>(
    ctx: &'a CoreContext,
    changeset: &'a ChangesetContext,
    matchers: HashMap<String, Arc<dyn Matcher + Send + Sync>>,
) -> Result<Out, MononokeError> {
    let root_fsnode_id = changeset.root_fsnode_id().await?;
    let root: Option<MPath> = None;
    bounded_traversal::bounded_traversal(
        256,
        (root, *root_fsnode_id.fsnode_id(), matchers),
        |(path, fsnode_id, matchers)| {
            cloned!(ctx, matchers);
            let blobstore = changeset.repo().blob_repo().blobstore();
            async move {
                let mut sizes: Out = HashMap::new();
                let mut next: HashMap<_, HashMap<_, _>> = HashMap::new();
                let fsnode = fsnode_id.load(&ctx, blobstore).await?;
                for (base_name, entry) in fsnode.list() {
                    let path = MPath::join_opt_element(path.as_ref(), base_name);
                    let path_vec = path.to_vec();
                    let repo_path = RepoPath::from_utf8(&path_vec)?;
                    match entry {
                        FsnodeEntry::File(leaf) => {
                            for (source, matcher) in &matchers {
                                if matcher.matches_file(repo_path)? {
                                    *sizes.entry(source.to_string()).or_insert(0) += leaf.size();
                                }
                            }
                        }
                        FsnodeEntry::Directory(tree) => {
                            for (source, matcher) in &matchers {
                                match matcher.matches_directory(repo_path)? {
                                    DirectoryMatch::Everything => {
                                        *sizes.entry(source.to_string()).or_insert(0) +=
                                            tree.summary().descendant_files_total_size;
                                    }
                                    DirectoryMatch::ShouldTraverse => {
                                        next.entry((Some(path.clone()), *tree.id()))
                                            .or_default()
                                            .insert(source.clone(), matcher.clone());
                                    }
                                    DirectoryMatch::Nothing => {}
                                }
                            }
                        }
                    }
                }

                anyhow::Ok((
                    sizes,
                    next.into_iter()
                        .map(|((path, fsnode_id), matchers)| (path, fsnode_id, matchers)),
                ))
            }
            .boxed()
        },
        |sizes, children| {
            async move {
                let t = children.fold(HashMap::new(), fold_maps);
                Ok(fold_maps(t, sizes))
            }
            .boxed()
        },
    )
    .await
    .map_err(MononokeError::from)
}

fn fold_maps(mut a: Out, b: Out) -> Out {
    for (source, size) in b {
        *a.entry(source).or_insert(0) += size;
    }
    a
}

pub async fn get_all_profiles(changeset: &ChangesetContext) -> Result<Vec<MPath>, MononokeError> {
    // TODO: read profile location from config
    let prefixes = vec![MononokePath::try_from(SPARSE_PROFILES_LOCATION)?];
    let files = changeset
        .find_files(Some(prefixes), None, None, ChangesetFileOrdering::Unordered)
        .await?;
    let matcher = pathmatcher::TreeMatcher::from_rules(
        [
            "tools/scm/sparse/README.md",
            "tools/scm/sparse/arvr/.castle/definitions/validate_sparse_profiles.json",
            "tools/scm/sparse/validation/**",
        ]
        .iter(),
    )
    .context("Couldn't create matcher for excluded sparse profiles")?;
    files
        .try_filter_map(|path| {
            borrowed!(matcher);
            async move {
                Ok(match matcher.matches(path.to_string()) {
                    // Since None in MononokePath is a root repo directory
                    // and we are returning list of profiles
                    // we can safely filter that out.
                    false => path.into_mpath(),
                    true => None,
                })
            }
        })
        .try_collect()
        .await
}

async fn get_entry_size(content: &ChangesetPathContentContext) -> Result<u64, MononokeError> {
    let path = content.path();
    match content.entry().await? {
        PathEntry::File(file, _) => Ok(file.metadata().await?.total_size),
        PathEntry::Tree(_) => Err(MononokeError::from(anyhow!(
            "Got Tree entry for the diff, while requested Files only. Path {}",
            path
        ))),
        PathEntry::NotPresent => Ok(0),
    }
}

async fn get_bonsai_size_change(
    current: &ChangesetContext,
    other: &ChangesetContext,
) -> Result<Vec<BonsaiSizeChange>> {
    let diff_items = btreeset! { ChangesetDiffItem::FILES };
    let diff = current
        .diff_unordered(other, true, None, diff_items)
        .await?;
    let res = stream::iter(diff)
        .map(|diff| async move {
            match diff {
                ChangesetPathDiffContext::Added(content) => {
                    anyhow::Ok(vec![BonsaiSizeChange::Added {
                        path: content.path().clone(),
                        size_change: get_entry_size(&content).await?,
                    }])
                }
                ChangesetPathDiffContext::Removed(content) => {
                    anyhow::Ok(vec![BonsaiSizeChange::Removed {
                        path: content.path().clone(),
                        size_change: get_entry_size(&content).await?,
                    }])
                }
                ChangesetPathDiffContext::Changed(new, old) => {
                    let (new_size, old_size) =
                        try_join!(get_entry_size(&new), get_entry_size(&old))?;

                    let new_size = i64::try_from(new_size).with_context(|| {
                        format!(
                            "Size of the file {} can't be converted back to i64",
                            new.path()
                        )
                    })?;
                    let old_size = i64::try_from(old_size).with_context(|| {
                        format!(
                            "Size of the file {} can't be converted back to i64",
                            old.path()
                        )
                    })?;
                    let size_change = new_size - old_size;
                    anyhow::Ok(vec![BonsaiSizeChange::Changed {
                        path: new.path().clone(),
                        size_change,
                    }])
                }
                ChangesetPathDiffContext::Copied(to, _from) => {
                    anyhow::Ok(vec![BonsaiSizeChange::Added {
                        path: to.path().clone(),
                        size_change: get_entry_size(&to).await?,
                    }])
                }
                ChangesetPathDiffContext::Moved(to, from) => anyhow::Ok(vec![
                    BonsaiSizeChange::Added {
                        path: to.path().clone(),
                        size_change: get_entry_size(&to).await?,
                    },
                    BonsaiSizeChange::Removed {
                        path: from.path().clone(),
                        size_change: get_entry_size(&from).await?,
                    },
                ]),
            }
        })
        .buffered(100)
        .try_collect::<Vec<_>>()
        .await?;
    Ok(res.into_iter().flatten().collect())
}

fn match_path(matcher: &dyn Matcher, path: &MononokePath) -> Result<bool> {
    // None here means repo root which is empty RepoPath
    let maybe_path_vec = path.as_mpath().map(|path| path.to_vec());
    let path_vec = maybe_path_vec.unwrap_or_default();
    matcher.matches_file(RepoPath::from_utf8(&path_vec)?)
}

pub async fn get_profile_delta_size(
    ctx: &CoreContext,
    current: &ChangesetContext,
    other: &ChangesetContext,
    paths: Vec<MPath>,
) -> Result<HashMap<String, ProfileSizeChange>, MononokeError> {
    let matchers = create_matchers(current, paths).await?;
    calculate_delta_size(ctx, current, other, matchers).await
}

pub async fn calculate_delta_size<'a>(
    ctx: &'a CoreContext,
    current: &'a ChangesetContext,
    other: &'a ChangesetContext,
    matchers: HashMap<String, Arc<dyn Matcher + Send + Sync>>,
) -> Result<HashMap<String, ProfileSizeChange>, MononokeError> {
    let diff_change = get_bonsai_size_change(current, other).await?;
    let (sparse_config_change, other_changes): (Vec<_>, Vec<_>) = diff_change
        .into_iter()
        .partition(|entry| is_profile_config_change(ctx.logger(), entry.path()));
    let mut sizes: HashMap<_, _> = other_changes
        .iter()
        .try_fold(HashMap::new(), |mut sizes, entry| {
            let (path, size_change) = match entry {
                BonsaiSizeChange::Added { path, size_change } => (path, *size_change as i64),
                BonsaiSizeChange::Removed { path, size_change } => (path, -(*size_change as i64)),
                BonsaiSizeChange::Changed { path, size_change } => (path, *size_change),
            };
            for (source, matcher) in &matchers {
                if match_path(matcher, path)? {
                    *sizes.entry(source).or_insert(0) += size_change;
                }
            }
            anyhow::Ok(sizes)
        })?
        .into_iter()
        .map(|(source, size)| (source.clone(), ProfileSizeChange::Changed(size)))
        .collect();
    let profile_configs_change =
        calculate_profile_config_change(ctx, current, other, sparse_config_change).await?;
    sizes.extend(profile_configs_change.into_iter());
    Ok(sizes)
}

fn is_profile_config_change(logger: &Logger, path: &MononokePath) -> bool {
    let profiles_location = MononokePath::try_from(SPARSE_PROFILES_LOCATION);
    match profiles_location {
        Ok(prefix) => MPath::is_prefix_of_opt(prefix.as_mpath(), MPath::iter_opt(path.as_mpath())),
        Err(e) => {
            error!(
                logger,
                "Couldn't convert sparse profiles location {}: {}", path, e
            );
            false
        }
    }
}

async fn calculate_profile_config_change<'a>(
    ctx: &'a CoreContext,
    current: &'a ChangesetContext,
    other: &'a ChangesetContext,
    // This should be changes to the sparse profile configs only
    changes: Vec<BonsaiSizeChange>,
) -> Result<HashMap<String, ProfileSizeChange>, MononokeError> {
    let mut changed = vec![];
    let mut raw_increase = vec![];
    let mut raw_decrease = vec![];
    for entry in changes {
        match entry {
            BonsaiSizeChange::Added {
                path,
                size_change: _,
            } => {
                if let Some(path) = path.into_mpath() {
                    raw_increase.push(path);
                }
            }
            BonsaiSizeChange::Removed {
                path,
                size_change: _,
            } => {
                if let Some(path) = path.into_mpath() {
                    raw_decrease.push(path);
                }
            }
            BonsaiSizeChange::Changed {
                path,
                size_change: _,
            } => {
                if let Some(path) = path.into_mpath() {
                    changed.push(path);
                }
            }
        }
    }
    // profiles which need current total sizes
    let current_paths: Vec<_> = changed
        .iter()
        .cloned()
        .chain(raw_increase.iter().cloned())
        .collect();
    // profiles which need previous commit total sizes (need diff or been removed)
    let previous_paths: Vec<_> = changed
        .iter()
        .cloned()
        .chain(raw_decrease.iter().cloned())
        .collect();

    let (current_sizes, previous_sizes) = try_join!(
        get_profile_size(ctx, current, current_paths),
        get_profile_size(ctx, other, previous_paths)
    )?;
    let mut result = HashMap::new();
    for path in raw_increase {
        let size = current_sizes.get(&path.to_string()).ok_or_else(|| {
            anyhow!(
                "Size for the {} wasn't calculated for current cs: {}",
                path,
                current.id()
            )
        })?;
        result.insert(path.to_string(), ProfileSizeChange::Added(*size));
    }
    for path in raw_decrease {
        let size = previous_sizes.get(&path.to_string()).ok_or_else(|| {
            anyhow!(
                "Size for the {} wasn't calculated for previous cs: {}",
                path,
                other.id()
            )
        })?;
        result.insert(path.to_string(), ProfileSizeChange::Removed(*size));
    }
    for path in changed {
        let new_size = *current_sizes.get(&path.to_string()).ok_or_else(|| {
            anyhow!(
                "Size for the {} wasn't calculated for cs: {}",
                path,
                current.id()
            )
        })? as i64;
        let old_size = *previous_sizes.get(&path.to_string()).ok_or_else(|| {
            anyhow!(
                "Size for the {} wasn't calculated for cs: {}",
                path,
                other.id()
            )
        })? as i64;
        result.insert(
            path.to_string(),
            ProfileSizeChange::Changed(new_size - old_size),
        );
    }
    Ok(result)
}

#[derive(Debug)]
enum BonsaiSizeChange {
    Added {
        path: MononokePath,
        size_change: u64,
    },
    Removed {
        path: MononokePath,
        size_change: u64,
    },
    Changed {
        path: MononokePath,
        size_change: i64,
    },
}

impl BonsaiSizeChange {
    fn path(&self) -> &MononokePath {
        match self {
            BonsaiSizeChange::Added {
                path,
                size_change: _,
            }
            | BonsaiSizeChange::Removed {
                path,
                size_change: _,
            }
            | BonsaiSizeChange::Changed {
                path,
                size_change: _,
            } => path,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum ProfileSizeChange {
    Added(u64),
    Removed(u64),
    Changed(i64),
}
