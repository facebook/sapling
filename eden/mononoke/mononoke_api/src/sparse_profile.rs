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
use anyhow::{anyhow, Context, Error, Result};
use blobstore::Loadable;
use cloned::cloned;
use context::CoreContext;
use futures::{FutureExt, Stream, StreamExt, TryStreamExt};
use mononoke_types::{fsnode::FsnodeEntry, MPath};
use pathmatcher::{DirectoryMatch, Matcher};
use types::RepoPath;

use sparse::Profile;

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
    path: &MPath,
) -> Result<Option<u64>, MononokeError> {
    let content = fetch(path.to_string(), changeset)
        .await
        .context(format!("While fetching {path}"))?
        .ok_or_else(|| anyhow!("Content is empty"))?;
    let profile = Profile::from_bytes(content, path.to_string())
        .context(format!("while constructing Profile for source {path}"))?;
    let matcher = profile
        .matcher(|path| fetch(path, changeset))
        .await
        .context("While constructing matcher")?;
    Ok(Some(
        calculate_size(ctx, changeset, Arc::new(matcher)).await?,
    ))
}

async fn calculate_size<'a>(
    ctx: &'a CoreContext,
    changeset: &'a ChangesetContext,
    matcher: Arc<dyn pathmatcher::Matcher + Send + Sync>,
) -> Result<u64, MononokeError> {
    let root_fsnode_id = changeset.root_fsnode_id().await?;
    let root: Option<MPath> = None;
    let sizes = bounded_traversal::bounded_traversal_stream(
        256,
        vec![(root, *root_fsnode_id.fsnode_id())],
        |(path, fsnode_id)| {
            cloned!(ctx, matcher);
            let blobstore = changeset.repo().blob_repo().blobstore();
            async move {
                let mut size = 0;
                let mut next = vec![];
                let fsnode = fsnode_id.load(&ctx, blobstore).await?;
                for (base_name, entry) in fsnode.list() {
                    let path = MPath::join_opt_element(path.as_ref(), base_name);
                    let path_vec = path.to_vec();
                    let repo_path = RepoPath::from_utf8(&path_vec)?;
                    match entry {
                        FsnodeEntry::File(leaf) => {
                            if matcher.matches_file(repo_path)? {
                                size += leaf.size();
                            }
                        }
                        FsnodeEntry::Directory(tree) => {
                            match matcher.matches_directory(repo_path)? {
                                DirectoryMatch::Everything => {
                                    size += tree.summary().descendant_files_total_size;
                                }
                                DirectoryMatch::Nothing => {}
                                DirectoryMatch::ShouldTraverse => {
                                    next.push((Some(path), *tree.id()));
                                }
                            }
                        }
                    }
                }

                Result::<_, Error>::Ok((size, next))
            }
            .boxed()
        },
    )
    .try_collect::<Vec<_>>()
    .await?;
    Ok(sizes.iter().sum())
}

fn non_sparse_profile(path: &str) -> bool {
    path.starts_with("validate_sparse_profiles") || path == "README.md"
}

pub async fn get_all_profiles(
    changeset: &ChangesetContext,
) -> Result<impl Stream<Item = MPath>, MononokeError> {
    // TODO: read profile location from config
    let prefixes = vec![MononokePath::try_from("tools/scm/sparse")?];
    let files = changeset
        .find_files(Some(prefixes), None, ChangesetFileOrdering::Unordered)
        .await?;
    Ok(files.filter_map(|path| async move {
        path.ok()?
            .into_mpath()
            .filter(|path| match std::str::from_utf8(path.basename().as_ref()) {
                Err(_) => false,
                Ok(path) => !non_sparse_profile(path),
            })
    }))
}
