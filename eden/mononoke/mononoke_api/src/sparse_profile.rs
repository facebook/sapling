/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::ops::Not;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use blobstore::Loadable;
use cloned::cloned;
use context::CoreContext;
use futures::stream;
use futures::try_join;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryStreamExt;
use maplit::btreeset;
use metaconfig_types::SparseProfilesConfig;
use mononoke_types::fsnode::FsnodeEntry;
use mononoke_types::MPath;
use pathmatcher::DirectoryMatch;
use pathmatcher::Matcher;
use repo_sparse_profiles::RepoSparseProfiles;
use slog::debug;
use types::RepoPath;

use crate::errors::MononokeError;
use crate::ChangesetContext;
use crate::ChangesetDiffItem;
use crate::ChangesetFileOrdering;
use crate::ChangesetPathContentContext;
use crate::ChangesetPathDiffContext;
use crate::MononokePath;
use crate::PathEntry;

// This struct contains matchers which will be consulted in various scenarious
// Clients can request analysis either for ALL profiles or Set of interested
//  1. If client requests exact list of profiles, then `exact_profiles_matcher`
//     will be used to check for the config changed and profiles will be
//     located using `changeset::find_files()`
//  2. If client asks to analyse all profiles - we check the configuration:
//     a. If list of monitored_profiles is set, then we use
//        `monitoring_profiles_only_matcher`
//     b. If monitored_profiles is None, then profiles would be checked
//        in configured location with respect of optional excludes.
pub struct SparseProfileMonitoring {
    sql_sparse_profiles: Arc<RepoSparseProfiles>,
    sparse_config: SparseProfilesConfig,
    exact_profiles_matcher: pathmatcher::TreeMatcher,
    profiles_location_with_excludes_matcher: pathmatcher::TreeMatcher,
    monitoring_profiles_only_matcher: Option<pathmatcher::TreeMatcher>,
    monitoring_profiles: MonitoringProfiles,
}

impl SparseProfileMonitoring {
    pub fn new(
        repo_name: &str,
        sql_sparse_profiles: Arc<RepoSparseProfiles>,
        maybe_sparse_config: Option<SparseProfilesConfig>,
        monitoring_profiles: MonitoringProfiles,
    ) -> Result<Self, MononokeError> {
        let sparse_config = maybe_sparse_config.ok_or_else(|| {
            MononokeError::from(anyhow!(
                "There isn't sparse profiles monitoring config in repo {}",
                repo_name
            ))
        })?;
        let rules: Vec<_> = vec![format!("{}/**", sparse_config.sparse_profiles_location)]
            .into_iter()
            .chain(
                sparse_config
                    .excluded_paths
                    .iter()
                    .map(|s| format!("!{}/{}", sparse_config.sparse_profiles_location, s)),
            )
            .collect();
        let profiles_location_with_excludes_matcher =
            pathmatcher::TreeMatcher::from_rules(rules.iter(), true).context(format!(
                "Couldn't create profiles config matcher for repo {} from rules {:?}",
                repo_name, rules,
            ))?;
        let exact_profiles_matcher = match monitoring_profiles {
            MonitoringProfiles::Exact { ref profiles } => {
                pathmatcher::TreeMatcher::from_rules(profiles.iter(), true).context(format!(
                    "Couldn't create exact profiles matcher for repo {} from rules {:?}",
                    repo_name, rules,
                ))?
            }
            // In that case exact_proifles_matcher will not be used, however making it Option
            // brings in some unnecessary complexity, so I just cloned existing matcher.
            MonitoringProfiles::All => profiles_location_with_excludes_matcher.clone(),
        };
        let monitoring_profiles_only_matcher = if sparse_config.monitored_profiles.is_empty() {
            None
        } else {
            Some(
                pathmatcher::TreeMatcher::from_rules(
                    sparse_config
                        .monitored_profiles
                        .iter()
                        .map(|p| format!("{}/{}", sparse_config.sparse_profiles_location, p)),
                    true,
                )
                .context(format!(
                    "Couldn't create monitored profiles matcher for repo {} from rules {:?}",
                    repo_name, sparse_config.monitored_profiles,
                ))?,
            )
        };
        Ok(Self {
            sql_sparse_profiles,
            sparse_config,
            exact_profiles_matcher,
            profiles_location_with_excludes_matcher,
            monitoring_profiles_only_matcher,
            monitoring_profiles,
        })
    }

    pub async fn get_monitoring_profiles(
        &self,
        changeset: &ChangesetContext,
    ) -> Result<Vec<MPath>, MononokeError> {
        match &self.monitoring_profiles {
            MonitoringProfiles::All => {
                let prefixes = vec![MononokePath::try_from(
                    &self.sparse_config.sparse_profiles_location,
                )?];
                let files = changeset
                    .find_files(Some(prefixes), None, None, ChangesetFileOrdering::Unordered)
                    .await?;
                files
                    .try_filter_map(|path| async move {
                        let matcher = self
                            .monitoring_profiles_only_matcher
                            .as_ref()
                            .map_or_else(|| &self.profiles_location_with_excludes_matcher, |m| m);
                        Ok(match matcher.matches(path.to_string()) {
                            // Since None in MononokePath is a root repo directory
                            // and we are returning list of profiles
                            // we can safely filter that out.
                            true => path.into_mpath(),
                            false => None,
                        })
                    })
                    .try_collect()
                    .await
            }
            MonitoringProfiles::Exact { profiles } => {
                let prefixes = profiles
                    .iter()
                    .map(MononokePath::try_from)
                    .collect::<Result<Vec<_>, MononokeError>>()?;
                changeset
                    .find_files(Some(prefixes), None, None, ChangesetFileOrdering::Unordered)
                    .await?
                    .map(|p| {
                        p.and_then(|path| {
                            path.into_mpath().ok_or_else(|| {
                                MononokeError::from(anyhow!(
                                    "Provided root diretory as monitored profile."
                                ))
                            })
                        })
                    })
                    .try_collect()
                    .await
            }
        }
    }

    fn is_profile_config_change(&self, path: &MononokePath) -> bool {
        let matcher = match self.monitoring_profiles {
            MonitoringProfiles::Exact { .. } => &self.exact_profiles_matcher,
            MonitoringProfiles::All => &self.profiles_location_with_excludes_matcher,
        };
        matcher.matches(path.to_string())
    }

    pub async fn get_profile_size(
        &self,
        ctx: &CoreContext,
        changeset: &ChangesetContext,
        paths: Vec<MPath>,
    ) -> Result<Out, MononokeError> {
        let cs_id = changeset.id();
        let maybe_sizes = self
            .sql_sparse_profiles
            .get_profiles_sizes(cs_id, paths.iter().map(MPath::to_string).collect())
            .await?;
        let (paths_to_calculate, mut sizes) = match maybe_sizes {
            None => (paths, HashMap::new()),
            Some(sizes) => {
                let processed_paths = sizes
                    .iter()
                    .map(|(path, _)| MPath::try_from(<String as AsRef<str>>::as_ref(path)))
                    .collect::<Result<HashSet<_>>>()?;
                (
                    paths
                        .into_iter()
                        .filter(|path| processed_paths.contains(path).not())
                        .collect(),
                    sizes.into_iter().collect(),
                )
            }
        };
        if paths_to_calculate.is_empty().not() {
            let matchers = create_matchers(changeset, paths_to_calculate).await?;
            let other_sizes = calculate_size(ctx, changeset, matchers).await?;
            let res = self
                .sql_sparse_profiles
                .insert_profiles_sizes(cs_id, other_sizes.clone())
                .await?;
            if let Some(false) = res {
                debug!(
                    ctx.logger(),
                    "Failed to insert sizes into DB for cs_id {}", cs_id
                );
            }
            sizes.extend(other_sizes);
        }
        Ok(sizes)
    }
}

pub(crate) async fn fetch(path: String, changeset: &ChangesetContext) -> Result<Option<Vec<u8>>> {
    let path: &str = &path;
    let path = MPath::try_from(path)?;
    let path_with_content = changeset.path_with_content(path.clone()).await?;
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
    monitor: &SparseProfileMonitoring,
    current: &ChangesetContext,
    other: &ChangesetContext,
    paths: Vec<MPath>,
) -> Result<HashMap<String, ProfileSizeChange>, MononokeError> {
    let matchers = create_matchers(current, paths).await?;
    calculate_delta_size(ctx, monitor, current, other, matchers).await
}

pub async fn calculate_delta_size<'a>(
    ctx: &'a CoreContext,
    monitor: &'a SparseProfileMonitoring,
    current: &'a ChangesetContext,
    other: &'a ChangesetContext,
    matchers: HashMap<String, Arc<dyn Matcher + Send + Sync>>,
) -> Result<HashMap<String, ProfileSizeChange>, MononokeError> {
    let diff_change = get_bonsai_size_change(current, other).await?;
    let (sparse_config_change, other_changes): (Vec<_>, Vec<_>) = diff_change
        .into_iter()
        .partition(|entry| monitor.is_profile_config_change(entry.path()));
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
        calculate_profile_config_change(ctx, monitor, current, other, sparse_config_change).await?;
    sizes.extend(profile_configs_change.into_iter());
    Ok(sizes
        .into_iter()
        .filter(|(_, size)| {
            !(*size == ProfileSizeChange::Added(0)
                || *size == ProfileSizeChange::Removed(0)
                || *size == ProfileSizeChange::Changed(0))
        })
        .collect())
}

async fn calculate_profile_config_change<'a>(
    ctx: &'a CoreContext,
    monitor: &'a SparseProfileMonitoring,
    current: &'a ChangesetContext,
    other: &'a ChangesetContext,
    // This should be changes to the sparse profile configs only
    changes: Vec<BonsaiSizeChange>,
) -> Result<HashMap<String, ProfileSizeChange>, MononokeError> {
    let mut result = HashMap::new();
    if changes.is_empty() {
        return Ok(result);
    }
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
        monitor.get_profile_size(ctx, current, current_paths),
        monitor.get_profile_size(ctx, other, previous_paths)
    )?;
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

pub enum MonitoringProfiles {
    All,
    Exact { profiles: Vec<String> },
}
