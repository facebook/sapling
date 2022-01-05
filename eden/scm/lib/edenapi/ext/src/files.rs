/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::path::Path;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use crossbeam::channel;
use edenapi::api::EdenApi;
use edenapi_types::AnyFileContentId;
use edenapi_types::AnyId;
use edenapi_types::FileType;
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
    paths: Vec<(RepoPathBuf, FileType)>,
    token: &UploadToken,
    // Try not to fail if possible
    conservative: bool,
) -> Result<(Vec<(RepoPathBuf, FileType)>, Option<Bytes>)> {
    let desired_cid = match token.data.id {
        AnyId::AnyFileContentId(AnyFileContentId::ContentId(cid)) => cid,
        // Id is not in the desired format, skip optimisation
        _ => {
            if conservative {
                return Ok((paths, None));
            } else {
                bail!("Token not in ContentId format")
            }
        }
    };
    let (send, recv) = channel::unbounded();
    let filtered_paths = stream::iter(paths)
        .map(Result::<_>::Ok)
        .try_filter_map(|(rel_path, file_type)| {
            let mut abs_path = root.clone();
            abs_path.push(&rel_path);
            let send = send.clone();
            async move {
                let future = {
                    let rel_path = rel_path.clone();
                    async move {
                        let abs_path = abs_path.as_repo_path().as_str();
                        let is_symlink = tokio::fs::symlink_metadata(abs_path)
                            .await?
                            .file_type()
                            .is_symlink();
                        let bytes = match file_type {
                            FileType::Executable | FileType::Regular => {
                                if is_symlink {
                                    bail!("File '{}' is a symlink", rel_path)
                                }
                                Bytes::from_owner(tokio::fs::read(abs_path).await?)
                            }
                            FileType::Symlink => {
                                if !is_symlink {
                                    bail!("File '{}' is not a symlink", rel_path)
                                }
                                let link = tokio::fs::read_link(abs_path).await?;
                                let to = link.to_str().context("invalid path")?.as_bytes();
                                Bytes::copy_from_slice(to)
                            }
                        };
                        let content_id = calc_contentid(&bytes);
                        if content_id == desired_cid {
                            if send.is_empty() {
                                let _ = send.send(bytes);
                            }
                            Ok(None)
                        } else {
                            Ok(Some((rel_path, file_type)))
                        }
                    }
                };
                match future.await {
                    Ok(r) => Ok(r),
                    Err(_) if conservative => return Ok(Some((rel_path, file_type))),
                    Err(err) => Err(err.into()),
                }
            }
        })
        .try_collect()
        .await?;
    Ok((filtered_paths, recv.try_recv().ok()))
}

struct MergedTokens {
    token: UploadToken,
    paths: Vec<(RepoPathBuf, FileType)>,
}

fn merge_tokens(
    files: impl IntoIterator<Item = (RepoPathBuf, UploadToken, FileType)>,
) -> impl Iterator<Item = MergedTokens> {
    let to_download =
        files
            .into_iter()
            .fold(HashMap::new(), |mut map, (path, token, file_type)| {
                map.entry((token.data.id.clone(), token.data.bubble_id))
                    .or_insert_with(|| MergedTokens {
                        token,
                        paths: vec![],
                    })
                    .paths
                    .push((path, file_type));
                map
            });

    to_download.into_iter().map(|(_, value)| value)
}

const WORKERS: usize = 10;

pub async fn check_files(
    root: &RepoPathBuf,
    files: impl IntoIterator<Item = (RepoPathBuf, UploadToken, FileType)>,
) -> Result<Vec<RepoPathBuf>> {
    stream::iter(merge_tokens(files).map(|value| async move {
        let (paths, _) = on_disk_optimization(root, value.paths, &value.token, false).await?;
        let paths = paths.into_iter().map(|(path, _)| path);
        Result::<_>::Ok(stream::iter(paths).map(Result::<_>::Ok))
    }))
    .buffered(WORKERS)
    .try_flatten()
    .try_collect()
    .await
}

async fn symlink(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> Result<()> {
    #[cfg(unix)]
    {
        tokio::fs::symlink(src, dst).await?;
    }
    #[cfg(windows)]
    {
        let metadata = tokio::fs::metadata(src.as_ref()).await?;
        if metadata.file_type().is_dir() {
            tokio::fs::symlink_dir(src, dst).await?;
        } else {
            tokio::fs::symlink_file(src, dst).await?;
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        unimplemented!()
    }
    Ok(())
}

pub async fn download_files(
    api: &(impl EdenApi + ?Sized),
    root: &RepoPathBuf,
    files: impl IntoIterator<Item = (RepoPathBuf, UploadToken, FileType)>,
) -> Result<()> {
    let vfs = VFS::new(std::path::PathBuf::from(root.as_str()))?;
    let writer = AsyncVfsWriter::spawn_new(vfs, WORKERS);
    let writer = &writer;

    stream::iter(merge_tokens(files).map(|value| async move {
        let (paths, content) = on_disk_optimization(root, value.paths, &value.token, true).await?;
        let len = paths.len();
        if len == 0 {
            return Ok(());
        }
        let content = match content {
            Some(bytes) => bytes,
            None => api.download_file(value.token).await?,
        };
        let (write_paths, symlink_paths): (Vec<_>, Vec<_>) = paths
            .into_iter()
            // We're zipping and using repeat_n to avoid cloning the
            // whole content unecessarily. One file should be the most
            // common case.
            .zip(itertools::repeat_n(content, len))
            .partition(|content| content.0.1 != FileType::Symlink);
        writer
            .write_batch(
                write_paths
                    .into_iter()
                    .map(|((path, _), content)| (path, content, None)),
            )
            .await?;

        for ((path, _), content) in symlink_paths {
            symlink(
                String::from_utf8(content.to_vec())?,
                AsRef::<str>::as_ref(&path),
            )
            .await?;
        }


        Ok(())
    }))
    .buffered(WORKERS)
    .try_collect()
    .await
}
