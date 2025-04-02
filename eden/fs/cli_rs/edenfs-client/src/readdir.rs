/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use async_recursion::async_recursion;
use edenfs_error::impl_eden_data_into_result;
use edenfs_error::EdenDataIntoResult;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_error::ResultExt;
use edenfs_error::ThriftRequestError;
use edenfs_utils::bytes_from_path;
use edenfs_utils::path_from_bytes_lossy;

use crate::attributes::FileAttributeDataOrErrorV2;
use crate::attributes::FileAttributeDataV2;
use crate::attributes::SourceControlType;
use crate::attributes::SourceControlTypeOrError;
use crate::client::EdenFsClient;
use crate::types::attributes_as_bitmask;
use crate::types::FileAttributes;
use crate::types::SyncBehavior;

pub type DirListAttributeEntry = HashMap<PathBuf, FileAttributeDataOrErrorV2>;
pub type ReaddirEntry = (PathBuf, FileAttributeDataV2);
pub type ListDirResult = Result<Vec<ReaddirEntry>>;

#[derive(Debug)]
enum DirListAttributeDataOrError {
    DirListAttributeData(DirListAttributeEntry),
    Error(EdenFsError),
    UnknownField(i32),
}

impl From<thrift_types::edenfs::DirListAttributeDataOrError> for DirListAttributeDataOrError {
    fn from(from: thrift_types::edenfs::DirListAttributeDataOrError) -> Self {
        match from {
            thrift_types::edenfs::DirListAttributeDataOrError::dirListAttributeData(data) => {
                DirListAttributeDataOrError::DirListAttributeData(
                    data.into_iter()
                        .map(|e| (path_from_bytes_lossy(&e.0), e.1.into()))
                        .collect(),
                )
            }
            thrift_types::edenfs::DirListAttributeDataOrError::error(error) => {
                Self::Error(EdenFsError::ThriftRequestError(error.into()))
            }
            thrift_types::edenfs::DirListAttributeDataOrError::UnknownField(unknown) => {
                Self::UnknownField(unknown)
            }
        }
    }
}

impl_eden_data_into_result!(
    DirListAttributeDataOrError,
    DirListAttributeEntry,
    DirListAttributeData
);

#[derive(Debug)]
struct ReaddirResult {
    #[allow(dead_code)]
    pub dir_lists: Vec<DirListAttributeDataOrError>,
}

impl From<thrift_types::edenfs::ReaddirResult> for ReaddirResult {
    fn from(from: thrift_types::edenfs::ReaddirResult) -> Self {
        Self {
            dir_lists: from.dirLists.into_iter().map(Into::into).collect(),
        }
    }
}

impl EdenFsClient {
    async fn readdir<P, R>(
        &self,
        mount_path: &P,
        directory_paths: &[R],
        attributes: i64,
        sync: SyncBehavior,
    ) -> Result<ReaddirResult>
    where
        P: AsRef<Path>,
        R: AsRef<Path>,
    {
        let directory_paths: Result<Vec<Vec<u8>>> = directory_paths
            .iter()
            .map(|p| bytes_from_path(p.as_ref().to_path_buf()))
            .collect();
        let params = thrift_types::edenfs::ReaddirParams {
            mountPoint: bytes_from_path(mount_path.as_ref().to_path_buf())?,
            directoryPaths: directory_paths?,
            requestedAttributes: attributes,
            sync: sync.into(),
            ..Default::default()
        };
        tracing::trace!(
            "Issuing readdir request with the following params: {:?}",
            &params
        );
        self.with_thrift(|t| t.readdir(&params))
            .await
            .map_err(|e| EdenFsError::Other(anyhow!("failed to get readdir result: {:?}", e)))
            .map(Into::into)
    }

    #[allow(dead_code)]
    async fn readdir_with_attributes_vec<P, R>(
        &self,
        mount_path: &P,
        directory_paths: &[R],
        attributes: &[FileAttributes],
        sync: SyncBehavior,
    ) -> Result<ReaddirResult>
    where
        P: AsRef<Path>,
        R: AsRef<Path>,
    {
        let attributes = attributes_as_bitmask(attributes);
        self.readdir(mount_path, directory_paths, attributes, sync)
            .await
    }

    pub async fn recursive_readdir_with_attributes_vec<P, R>(
        self: Arc<Self>,
        mount_path: &P,
        root: &R,
        attributes: &[FileAttributes],
        parallelism: usize,
    ) -> ListDirResult
    where
        P: AsRef<Path>,
        R: AsRef<Path>,
    {
        // We depend on the SourceControlType to determine how to recurse through the directories.
        // Always include it in the attributes bitmask.
        let attributes_bitmask =
            attributes_as_bitmask(attributes) | (FileAttributes::SourceControlType as i64);
        self.recursive_readdir(mount_path, root, attributes_bitmask, parallelism)
            .await
    }

    pub async fn recursive_readdir<P, R>(
        self: Arc<Self>,
        mount_path: &P,
        root: &R,
        attributes: i64,
        parallelism: usize,
    ) -> ListDirResult
    where
        P: AsRef<Path>,
        R: AsRef<Path>,
    {
        _recursive_readdir(
            mount_path.as_ref().to_path_buf(),
            self.clone(),
            root.as_ref().to_path_buf(),
            vec!["".into()],
            attributes,
            parallelism,
        )
        .await
    }
}

#[async_recursion]
async fn _recursive_readdir(
    mount_path: PathBuf,
    client: Arc<EdenFsClient>,
    root: PathBuf,
    directory_list: Vec<PathBuf>,
    attributes: i64,
    parallelism: usize,
) -> ListDirResult {
    let mut files: Vec<ReaddirEntry> = Vec::new();
    let client = client.clone();
    let directory_list: std::vec::Vec<PathBuf> = directory_list
        .iter()
        .map(|dir| match dir.as_os_str().len() {
            0 => root.clone(),
            _ => root.join(dir),
        })
        .collect();
    let directory_listings = match client
        .readdir(
            &mount_path,
            &directory_list,
            attributes,
            SyncBehavior::no_sync(),
        )
        .await
    {
        Ok(lists) => lists,
        Err(e) => {
            return Err(anyhow!("readdir failed root={}: {e:?}", root.display()).into());
        }
    };

    let mut child_directories = Vec::new();
    for (data_or_error, directory) in directory_listings
        .dir_lists
        .into_iter()
        .zip(directory_list)
        .filter(|(data_or_error, dir)| match data_or_error {
            DirListAttributeDataOrError::Error(EdenFsError::ThriftRequestError(
                ThriftRequestError {
                    message: _,
                    error_code: Some(errno),
                    error_type: _,
                },
            )) if *errno == libc::ENOENT => {
                tracing::warn!("warning: {} does not exist.", dir.display());
                false
            }
            _ => true,
        })
    {
        for (filename, entry_data) in data_or_error
            .into_result()
            .with_context(|| directory.display().to_string())?
        {
            let entry_data = entry_data
                .into_result()
                .with_context(|| anyhow!("missing entry data for {}", filename.display()))?;
            let scm_type = entry_data.scm_type.as_ref().map_or_else(
                || Err(EdenFsError::Other(anyhow!("missing scm_type"))),
                |t| match t {
                    SourceControlTypeOrError::SourceControlType(t) => Ok(t),
                    _ => Err(EdenFsError::Other(anyhow!("missing scm_type"))),
                },
            );
            match scm_type {
                Err(e) => {
                    tracing::warn!(
                        "warning: failed to get scm_type for {}: {e:?}",
                        filename.display()
                    );
                }
                Ok(SourceControlType::Tree) => {
                    if !filename.starts_with(".") {
                        let relpath = directory.join(filename.clone());
                        child_directories.push(filename);
                        files.push((relpath, entry_data));
                    }
                }
                Ok(SourceControlType::RegularFile) | Ok(SourceControlType::ExecutableFile) => {
                    let relpath = directory.join(filename);
                    files.push((relpath, entry_data));
                }
                Ok(SourceControlType::Symlink) => {
                    tracing::debug!("symlink: {}", directory.display());
                }
                bad => return Err(anyhow!("unexpected SourceControlType: {:?}", bad).into()),
            }
        }
    }

    let subdir_files =
        futures::future::try_join_all(child_directories.chunks(parallelism).map(|directories| {
            let root = root.clone();
            let directories = directories.to_vec();
            let mount_path = mount_path.clone();
            let client = client.clone();
            tokio::spawn(async move {
                _recursive_readdir(
                    mount_path,
                    client,
                    root,
                    directories,
                    attributes,
                    parallelism,
                )
                .await
            })
        }))
        .await
        .from_err()?;
    for subfiles in subdir_files {
        files.extend(subfiles?);
    }
    Ok(files)
}
