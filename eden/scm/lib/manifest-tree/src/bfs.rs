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
use anyhow::bail;
use edenapi_types::SaplingRemoteApiServerErrorKind;
use edenapi_types::errors::find_permission_denied;
use edenapi_types::errors::is_permission_denied;
use flume::Receiver;
use flume::Sender;
use flume::WeakSender;
use manifest::FileMetadata;
use pathmatcher::DirectoryMatch;
use pathmatcher::Matcher;
use storemodel::TreeEntry;
use types::FetchContext;
use types::FetchSyncMode;
use types::HgId;
use types::Key;
use types::PathComponentBuf;
use types::RepoPath;
use types::RepoPathBuf;

use crate::acl_metrics;
use crate::link::DurableEntry;
use crate::link::Link;
use crate::link::MaybeLinks;
use crate::store;
use crate::store::InnerStore;

fn num_workers() -> usize {
    num_cpus::get().min(20)
}

pub(crate) struct BfsWork<W, Ctx> {
    pub work: Vec<W>,
    pub ctx: Ctx,
}

pub(crate) trait Cancelable {
    fn canceled(&self) -> bool;
}

/// Returns false if the walk has been canceled.
pub(crate) fn try_send<W: Send + Sync + 'static, Ctx: Cancelable + Send + Sync + 'static>(
    work_send: &WeakSender<BfsWork<W, Ctx>>,
    work: BfsWork<W, Ctx>,
) -> Result<bool> {
    if work.ctx.canceled() {
        return Ok(false);
    }

    if work.work.is_empty() {
        return Ok(true);
    }

    match work_send.upgrade() {
        Some(send) => send.send(work)?,
        None => bail!("work channel disconnected (sender)"),
    }

    Ok(true)
}

/// Spawn BFS workers as dedicated threads with weak-sender shutdown.
/// Workers shut down when the returned strong `Sender` is dropped.
pub(crate) fn spawn_workers<W, Ctx, F>(worker_fn: F) -> Sender<BfsWork<W, Ctx>>
where
    W: Send + 'static,
    Ctx: Send + 'static,
    F: Fn(Receiver<BfsWork<W, Ctx>>, WeakSender<BfsWork<W, Ctx>>) -> Result<()>
        + Send
        + Clone
        + 'static,
{
    let (work_send, work_recv) = flume::unbounded();

    for _ in 0..num_workers() {
        let work_recv = work_recv.clone();
        let work_send_weak = work_send.downgrade();
        let worker_fn = worker_fn.clone();
        std::thread::spawn(move || {
            let res = worker_fn(work_recv, work_send_weak);
            tracing::debug!(?res, "bfs worker exited");
        });
    }

    work_send
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

/// Batch-fetch tree content and populate DurableEntry links.
pub(crate) fn prefetch_trees<'a>(
    store: &InnerStore,
    entries: impl IntoIterator<Item = PrefetchTree<'a>>,
    matcher: &dyn Matcher,
) -> Result<()> {
    let mut by_hgid: HashMap<HgId, Vec<PrefetchTree<'a>>> = HashMap::new();
    let mut keys = Vec::new();
    for entry in entries {
        if !entry.entry.links_initialized() {
            let v = by_hgid.entry(entry.entry.hgid).or_default();
            if v.is_empty() {
                keys.push(Key::new(RepoPathBuf::new(), entry.entry.hgid));
            }
            v.push(entry);
        }
    }

    if keys.is_empty() {
        return Ok(());
    }

    let span = tracing::debug_span!(
        "tree::store::prefetch",
        ids = keys
            .iter()
            .map(|k| k.hgid.to_hex())
            .collect::<Vec<String>>()
            .join(" ")
    );
    let _entered = span.enter();

    let fctx = FetchContext::default().with_sync_mode(FetchSyncMode::Sync);
    for res in store.get_tree_iter(fctx, keys)? {
        match res {
            Ok((key, tree_entry)) => {
                let mut denied_hgids = HashMap::new();
                let children_with_acl = match by_hgid.get(&key.hgid) {
                    Some(entries) => filter_acl_children(tree_entry.as_ref(), entries, matcher),
                    None => Ok(Vec::new()),
                };
                match children_with_acl.and_then(|children_with_acl| {
                    tree_entry.filter_permission_denied(children_with_acl)
                }) {
                    Ok(iter) => {
                        for item in iter {
                            match item {
                                Ok((_component, hgid, reason)) => {
                                    tracing::debug!(%hgid, reason, "marking child tree as permission denied");
                                    acl_metrics::ACL_AVOIDED.increment();
                                    denied_hgids.insert(hgid, reason);
                                }
                                Err(err) => {
                                    tracing::debug!(
                                        ?err,
                                        "error reading permission_denied_children"
                                    );
                                }
                            }
                        }
                    }
                    Err(err) => {
                        tracing::debug!(?err, "error calling permission_denied_children");
                    }
                }

                let links = tree_entry_to_links(&key.path, tree_entry, &denied_hgids)?;
                if let Some(entries) = by_hgid.get(&key.hgid) {
                    for entry in entries {
                        entry
                            .entry
                            .links
                            .get_or_init(|| MaybeLinks::Links(links.clone()));
                    }
                }
            }
            Err(ref err) if is_permission_denied(err) => {
                if let Some(SaplingRemoteApiServerErrorKind::PermissionDenied {
                    tree_id,
                    request_acl,
                }) = find_permission_denied(err).map(|e| &e.err)
                {
                    acl_metrics::ACL_DENIED.increment();
                    if let Some(entries) = by_hgid.get(tree_id) {
                        let perm_err = types::errors::PermissionDenied {
                            path: types::RepoPathBuf::new(),
                            hgid: *tree_id,
                            request_acl: request_acl.clone(),
                        };
                        for entry in entries {
                            entry
                                .entry
                                .links
                                .get_or_init(|| MaybeLinks::PermissionDenied(perm_err.clone()));
                        }
                    }
                }
            }
            Err(err) => return Err(err),
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
