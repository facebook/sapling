/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Result;
use edenapi::api::EdenApi;
use edenapi_types::UploadToken;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use types::RepoPathBuf;
use vfs::AsyncVfsWriter;
use vfs::VFS;

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
        let content = api.download_file(repo.clone(), value.token).await?;
        let len = value.paths.len();
        writer
            .write_batch(
                value
                    .paths
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
