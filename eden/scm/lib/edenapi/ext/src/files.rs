/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Result;
use crossbeam::channel;
use edenapi::api::EdenApi;
use edenapi_types::AnyFileContentId;
use edenapi_types::AnyId;
use edenapi_types::UploadToken;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use minibytes::Bytes;
use types::RepoPathBuf;
use vfs::AsyncVfsWriter;
use vfs::VFS;

use crate::calc_contentid;

/// If the desired file is already on disk, usually, from a previous snapshot restore,
/// we can just read it from disk and filter the paths based on which are still outdated.
async fn on_disk_optimization(
    root: &RepoPathBuf,
    paths: Vec<RepoPathBuf>,
    token: &UploadToken,
) -> (Vec<RepoPathBuf>, Option<Bytes>) {
    let desired_cid = match token.data.id {
        AnyId::AnyFileContentId(AnyFileContentId::ContentId(cid)) => cid,
        // Id is not in the desired format, skip optimisation
        _ => {
            return (paths, None);
        }
    };
    let (send, recv) = channel::unbounded();
    let filtered_paths = stream::iter(paths)
        .filter(|rel_path| {
            let mut abs_path = root.clone();
            abs_path.push(&rel_path);
            let send = send.clone();
            async move {
                let bytes = match tokio::fs::read(abs_path.as_repo_path().as_str()).await {
                    Ok(bytes) => bytes,
                    Err(_) => return true,
                };
                let content_id = calc_contentid(&bytes);
                if content_id == desired_cid {
                    if send.is_empty() {
                        let _ = send.send(Bytes::from_owner(bytes));
                    }
                    false
                } else {
                    true
                }
            }
        })
        .collect()
        .await;
    (filtered_paths, recv.try_recv().ok())
}

pub async fn download_files(
    api: &(impl EdenApi + ?Sized),
    repo: &String,
    root: &RepoPathBuf,
    files: impl IntoIterator<Item = (RepoPathBuf, UploadToken)>,
) -> Result<()> {
    struct Value {
        token: UploadToken,
        paths: Vec<RepoPathBuf>,
    }
    // Using a HashMap to merge all downloads of same content
    let to_download = files
        .into_iter()
        .fold(HashMap::new(), |mut map, (path, token)| {
            map.entry((token.data.id.clone(), token.data.bubble_id))
                .or_insert_with(|| Value {
                    token,
                    paths: vec![],
                })
                .paths
                .push(path);
            map
        });

    let vfs = VFS::new(std::path::PathBuf::from(root.as_str()))?;
    let workers = 10;
    let writer = AsyncVfsWriter::spawn_new(vfs, workers);
    let writer = &writer;

    stream::iter(to_download.into_iter().map(|(_, value)| async move {
        let (paths, content) = on_disk_optimization(root, value.paths, &value.token).await;
        let len = paths.len();
        if len == 0 {
            return Ok(());
        }
        let content = match content {
            Some(bytes) => bytes,
            None => api.download_file(repo.clone(), value.token).await?,
        };
        writer
            .write_batch(
                paths
                    .into_iter()
                    // We're zipping and using repeat_n to avoid cloning the
                    // whole content unecessarily. One file should be the most
                    // common case.
                    .zip(itertools::repeat_n(content, len))
                    .map(|(path, content)| (path, content, None)),
            )
            .await?;
        Ok(())
    }))
    .buffered(workers)
    .try_collect()
    .await
}
