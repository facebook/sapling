/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use cloned::cloned;
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
use vfs::UpdateFlag;
use vfs::VFS;

use crate::calc_contentid;

fn abs_path(root: &RepoPathBuf, rel_path: &RepoPathBuf) -> RepoPathBuf {
    let mut abs_path = root.clone();
    abs_path.push(rel_path);
    abs_path
}

struct OnDiskOptimizationResponse {
    /// Paths which don't have the correct content.
    incorrect_paths: Vec<(RepoPathBuf, FileType)>,
    /// Paths that have the correct content, but maybe not the correct permissions.
    correct_paths: Vec<(RepoPathBuf, FileType)>,
    /// If the content was found, it is returned.
    content: Option<Bytes>,
}

/// If the desired file is already on disk, usually, from a previous snapshot restore,
/// we can just read it from disk and filter the paths based on which are still outdated.
async fn on_disk_optimization(
    root: &RepoPathBuf,
    paths: Vec<(RepoPathBuf, FileType)>,
    token: &UploadToken,
    // Try not to fail if possible
    conservative: bool,
) -> Result<OnDiskOptimizationResponse> {
    let desired_cid = match token.data.id {
        AnyId::AnyFileContentId(AnyFileContentId::ContentId(cid)) => cid,
        _ => {
            if conservative {
                return Ok(OnDiskOptimizationResponse {
                    incorrect_paths: paths,
                    correct_paths: vec![],
                    content: None,
                });
            } else {
                bail!("Token not in ContentId format")
            }
        }
    };
    let (send_content, recv_content) = channel::unbounded();
    let (send_correct_paths, recv_correct_paths) = channel::unbounded();
    let incorrect_paths = stream::iter(paths)
        .map(Result::<_>::Ok)
        .try_filter_map(|(rel_path, file_type)| {
            let abs_path = abs_path(root, &rel_path);
            cloned!(send_content, send_correct_paths);
            async move {
                let future = {
                    cloned!(rel_path);
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
                            if send_content.is_empty() {
                                let _ = send_content.send(bytes);
                            }
                            let _ = send_correct_paths.send((rel_path, file_type));
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
    Ok(OnDiskOptimizationResponse {
        incorrect_paths,
        correct_paths: recv_correct_paths.try_iter().collect(),
        content: recv_content.try_recv().ok(),
    })
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

/// Return which files differ in content or symlinkness from the given upload tokens.
/// Note: does not return files that differ in executable permission.
pub async fn check_files(
    root: &RepoPathBuf,
    files: impl IntoIterator<Item = (RepoPathBuf, UploadToken, FileType)>,
) -> Result<Vec<RepoPathBuf>> {
    stream::iter(merge_tokens(files).map(|value| async move {
        let response = on_disk_optimization(root, value.paths, &value.token, false).await?;
        let paths = response.incorrect_paths.into_iter().map(|(path, _)| path);
        Result::<_>::Ok(stream::iter(paths).map(Result::<_>::Ok))
    }))
    .buffered(WORKERS)
    .try_flatten()
    .try_collect()
    .await
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
        let OnDiskOptimizationResponse {
            correct_paths,
            incorrect_paths,
            content,
        } = on_disk_optimization(root, value.paths, &value.token, true).await?;
        for (path, file_type) in correct_paths {
            match file_type {
                FileType::Regular => writer.set_executable(path, false).await?,
                FileType::Executable => writer.set_executable(path, true).await?,
                FileType::Symlink => {}
            }
        }
        let len = incorrect_paths.len();
        if len == 0 {
            return Ok(());
        }
        let content = match content {
            Some(bytes) => bytes,
            None => api.download_file(value.token).await?,
        };
        let write_paths = incorrect_paths
            .into_iter()
            // We're zipping and using repeat_n to avoid cloning the
            // whole content unecessarily. One file should be the most
            // common case.
            .zip(itertools::repeat_n(content, len))
            .map(|((path, file_type), content)| {
                (
                    path,
                    content,
                    match file_type {
                        FileType::Regular => UpdateFlag::Regular,
                        FileType::Symlink => UpdateFlag::Symlink,
                        FileType::Executable => UpdateFlag::Executable,
                    },
                )
            });
        writer.write_batch(write_paths).await?;

        Ok(())
    }))
    .buffered(WORKERS)
    .try_collect()
    .await
}
