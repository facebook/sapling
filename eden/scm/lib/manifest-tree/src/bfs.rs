/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use edenapi_types::errors::find_permission_denied;
use manifest::FileMetadata;
use pathmatcher::DirectoryMatch;
use pathmatcher::Matcher;
use slex::Batch;
use slex::Items;
use slex::Work;
use slex::WorkOptions;
use storemodel::TreeEntry;
use types::FetchContext;
use types::HgId;
use types::Key;
use types::PathComponentBuf;
use types::RepoPath;
use types::RepoPathBuf;
use types::fetch_mode::FetchMode;

use crate::acl_metrics;
use crate::link::DurableEntry;
use crate::link::Link;
use crate::link::MaybeLinks;
use crate::store;
use crate::store::InnerStore;

pub(crate) fn num_workers() -> usize {
    num_cpus::get().min(20)
}

fn tree_entry_to_links(
    parent_path: &types::RepoPath,
    entry: Arc<dyn TreeEntry>,
    denied_hgids: &HashMap<HgId, String>,
) -> Result<BTreeMap<PathComponentBuf, Link>> {
    let mut links = BTreeMap::new();
    for item in entry.iter_owned()? {
        let (component, hgid, flag) = item?;
        let link = match flag {
            store::Flag::File(file_type) => Link::leaf(FileMetadata::new(hgid, file_type)),
            store::Flag::Directory => {
                if let Some(request_acl) = denied_hgids.get(&hgid) {
                    let mut path = parent_path.to_owned();
                    path.push(component.as_path_component());
                    Link::durable_permission_denied(types::errors::PermissionDenied {
                        path,
                        hgid,
                        request_acl: request_acl.clone(),
                    })
                } else {
                    Link::durable(hgid)
                }
            }
        };
        links.insert(component, link);
    }
    Ok(links)
}

pub(crate) struct PrefetchTree<'a> {
    pub path: &'a RepoPath,
    pub entry: &'a Arc<DurableEntry>,
    pub subtree_matches_everything: bool,
}

#[derive(Clone)]
struct PrefetchWork {
    path: RepoPathBuf,
    entry: Arc<DurableEntry>,
    subtree_matches_everything: bool,
}

impl PrefetchWork {
    fn as_prefetch_tree(&self) -> PrefetchTree<'_> {
        PrefetchTree {
            path: self.path.as_repo_path(),
            entry: &self.entry,
            subtree_matches_everything: self.subtree_matches_everything,
        }
    }
}

impl<'a> From<PrefetchTree<'a>> for PrefetchWork {
    fn from(entry: PrefetchTree<'a>) -> Self {
        Self {
            path: entry.path.to_owned(),
            entry: Arc::clone(entry.entry),
            subtree_matches_everything: entry.subtree_matches_everything,
        }
    }
}

fn build_links(
    parent_path: &types::RepoPath,
    tree_entry: Arc<dyn TreeEntry>,
    entries: &[PrefetchTree<'_>],
    matcher: &dyn Matcher,
) -> Result<MaybeLinks> {
    let mut denied_hgids = HashMap::new();
    match filter_acl_children(tree_entry.as_ref(), entries, matcher)
        .and_then(|children_with_acl| tree_entry.filter_permission_denied(children_with_acl))
    {
        Ok(iter) => {
            for item in iter {
                match item {
                    Ok((_component, hgid, reason)) => {
                        tracing::debug!(%hgid, reason, "marking child tree as permission denied");
                        acl_metrics::ACL_AVOIDED.increment();
                        denied_hgids.insert(hgid, reason);
                    }
                    Err(err) => {
                        tracing::debug!(?err, "error reading permission_denied_children");
                    }
                }
            }
        }
        Err(err) => {
            tracing::debug!(?err, "error calling permission_denied_children");
        }
    }

    let links = tree_entry_to_links(parent_path, tree_entry, &denied_hgids)?;
    Ok(MaybeLinks::Links(links))
}

enum LocalPrefetch {
    Hit {
        work: PrefetchWork,
        tree_entry: Arc<dyn TreeEntry>,
    },
    Miss(PrefetchWork),
}

/// Batch-fetch tree content and populate DurableEntry links.
pub(crate) fn prefetch_trees<'a>(
    store: &InnerStore,
    entries: impl IntoIterator<Item = PrefetchTree<'a>>,
    matcher: &dyn Matcher,
) -> Result<()> {
    let entries = entries
        .into_iter()
        .filter(|entry| !entry.entry.links_initialized())
        .map(PrefetchWork::from)
        .collect::<Vec<_>>();
    if entries.is_empty() {
        return Ok(());
    }

    let span = tracing::debug_span!(
        "tree::store::prefetch",
        ids = entries
            .iter()
            .map(|entry| entry.entry.hgid.to_hex())
            .collect::<Vec<_>>()
            .join(" ")
    );
    let _entered = span.enter();

    let local_store = store.clone();
    let local_results = Work::try_map(
        WorkOptions::new(),
        Items::ready(entries),
        move |work: PrefetchWork| -> Result<LocalPrefetch> {
            match local_store.get_local_tree(RepoPath::empty(), work.entry.hgid)? {
                Some(tree_entry) => Ok(LocalPrefetch::Hit { work, tree_entry }),
                None => Ok(LocalPrefetch::Miss(work)),
            }
        },
    );

    let mut remote_keys = Vec::new();
    let mut remote_work_by_hgid: HashMap<HgId, Batch<PrefetchWork>> = HashMap::new();
    for result in local_results {
        match result? {
            LocalPrefetch::Hit { work, tree_entry } => {
                let prefetch = work.as_prefetch_tree();
                let links =
                    build_links(work.path.as_repo_path(), tree_entry, &[prefetch], matcher)?;
                work.entry.links.get_or_init(|| links);
            }
            LocalPrefetch::Miss(work) => {
                let key = Key::new(RepoPathBuf::new(), work.entry.hgid);
                remote_work_by_hgid
                    .entry(work.entry.hgid)
                    .or_default()
                    .push(work);
                remote_keys.push(key);
            }
        }
    }

    if !remote_keys.is_empty() {
        let fctx = FetchContext::new(FetchMode::RemoteOnly);
        for res in store.get_tree_iter(fctx, remote_keys)? {
            match res {
                Ok((key, tree_entry)) => {
                    let Some(work) = remote_work_by_hgid
                        .get_mut(&key.hgid)
                        .and_then(|work| work.pop())
                    else {
                        continue;
                    };
                    let prefetch = work.as_prefetch_tree();
                    let links =
                        build_links(work.path.as_repo_path(), tree_entry, &[prefetch], matcher)?;
                    work.entry.links.get_or_init(|| links);
                }
                Err(err) => {
                    let (hgid, request_acl) = match find_permission_denied(&err) {
                        Some(permission_denied) => permission_denied,
                        None => return Err(err),
                    };

                    let work = match remote_work_by_hgid
                        .get_mut(&hgid)
                        .and_then(|work| work.pop())
                    {
                        Some(work) => work,
                        None => {
                            tracing::warn!(
                                %hgid,
                                ?err,
                                "remote tree permission-denied error did not match pending prefetch work"
                            );
                            continue;
                        }
                    };
                    acl_metrics::ACL_DENIED.increment();
                    let perm_err = types::errors::PermissionDenied {
                        path: work.path,
                        hgid,
                        request_acl: request_acl.unwrap_or_default(),
                    };
                    work.entry
                        .links
                        .get_or_init(|| MaybeLinks::PermissionDenied(perm_err));
                }
            }
        }
    }
    Ok(())
}

fn filter_acl_children(
    tree_entry: &dyn TreeEntry,
    entries: &[PrefetchTree<'_>],
    matcher: &dyn Matcher,
) -> Result<Vec<(PathComponentBuf, HgId)>> {
    let children_with_acls = tree_entry.children_with_acls()?;
    let mut should_check = vec![false; children_with_acls.len()];

    for entry in entries {
        if should_check.iter().all(|should_check| *should_check) {
            break;
        }

        if entry.subtree_matches_everything {
            should_check.fill(true);
            break;
        }

        let mut child_path = None;
        for (should_check, (component, _hgid)) in
            should_check.iter_mut().zip(children_with_acls.iter())
        {
            if *should_check {
                continue;
            }

            let child_path = child_path.get_or_insert_with(|| entry.path.to_owned());
            child_path.push(component.as_path_component());
            if matcher.matches_directory(child_path)? != DirectoryMatch::Nothing {
                *should_check = true;
            }
            child_path.pop();
        }
    }

    Ok(children_with_acls
        .into_iter()
        .zip(should_check)
        .filter_map(|(child, should_check)| should_check.then_some(child))
        .collect())
}
