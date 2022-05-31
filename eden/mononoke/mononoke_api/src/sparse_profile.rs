/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::errors::MononokeError;
use crate::ChangesetContext;
use crate::ChangesetFileOrdering;
use crate::MononokePath;
use anyhow::{anyhow, Context, Result};
use blobstore::Loadable;
use cloned::cloned;
use context::CoreContext;
use futures::{stream, FutureExt, StreamExt, TryStreamExt};
use mononoke_types::{fsnode::FsnodeEntry, MPath};
use pathmatcher::{DirectoryMatch, Matcher};
use types::RepoPath;

use std::collections::HashMap;
use std::sync::Arc;

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
        .context(format!("Couldn't fetch content of {path}"))
        .map(|b| Some(b.to_vec()))
}

pub async fn get_profile_size(
    ctx: &CoreContext,
    changeset: &ChangesetContext,
    paths: Vec<MPath>,
) -> Result<HashMap<String, u64>, MononokeError> {
    let matchers: HashMap<_, _> = stream::iter(paths)
        .map(|path| async move {
            let content = fetch(path.to_string(), changeset)
                .await
                .context(format!("While fetching {path}"))?
                .ok_or_else(|| anyhow!("Content is empty"))?;
            let profile = sparse::Root::from_bytes(content, path.to_string())
                .context(format!("while constructing Profile for source {path}"))?;
            let matcher = profile
                .matcher(|path| fetch(path, changeset))
                .await
                .context(format!("While constructing matcher for source {path}"))?;
            anyhow::Ok((
                path.to_string(),
                Arc::new(matcher) as Arc<dyn Matcher + Send + Sync>,
            ))
        })
        .buffer_unordered(100)
        .try_collect()
        .await?;
    calculate_size(ctx, changeset, matchers).await
}

type Out = HashMap<String, u64>;

async fn calculate_size<'a>(
    ctx: &'a CoreContext,
    changeset: &'a ChangesetContext,
    matchers: HashMap<String, Arc<dyn Matcher + Send + Sync>>,
) -> Result<HashMap<String, u64>, MononokeError> {
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

fn non_sparse_profile(path: &str) -> bool {
    path.starts_with("validate_sparse_profiles") || path == "README.md"
}

pub async fn get_all_profiles(changeset: &ChangesetContext) -> Result<Vec<MPath>, MononokeError> {
    // TODO: read profile location from config
    let prefixes = vec![MononokePath::try_from("tools/scm/sparse")?];
    let files = changeset
        .find_files(Some(prefixes), None, ChangesetFileOrdering::Unordered)
        .await?;
    Ok(files
        .filter_map(|path| async move {
            path.ok()?.into_mpath().filter(|path| {
                match std::str::from_utf8(path.basename().as_ref()) {
                    Err(_) => false,
                    Ok(path) => !non_sparse_profile(path),
                }
            })
        })
        .collect()
        .await)
}
