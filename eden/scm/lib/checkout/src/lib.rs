/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{bail, format_err, Result};
use futures::{stream, try_join, Stream, StreamExt};
use manifest::{DiffEntry, DiffType, FileType};
use revisionstore::{HgIdDataStore, StoreKey, StoreResult};
use types::{HgId, Key, RepoPathBuf};
use vfs::{UpdateFlag, VFS};

/// Contains lists of files to be removed / updated during checkout.
#[allow(dead_code)]
pub struct CheckoutPlan {
    /// Files to be removed.
    remove: Vec<RepoPathBuf>,
    /// Files that needs their content updated.
    update_content: Vec<UpdateContentAction>,
    /// Files that only need X flag updated.
    update_meta: Vec<UpdateMetaAction>,
}

/// Update content and (possibly) metadata on the file
#[allow(dead_code)]
struct UpdateContentAction {
    /// Path to file.
    path: RepoPathBuf,
    /// If content has changed, HgId of new content.
    content_hgid: HgId,
    /// New file type.
    file_type: FileType,
}

/// Only update metadata on the file, do not update content
#[allow(dead_code)]
struct UpdateMetaAction {
    /// Path to file.
    path: RepoPathBuf,
    /// true if need to set executable flag, false if need to remove it.
    set_x_flag: bool,
}

impl CheckoutPlan {
    /// Processes diff into checkout plan.
    /// Left in the diff is a current commit.
    /// Right is a commit to be checked out.
    pub fn from_diff<D: Iterator<Item = Result<DiffEntry>>>(iter: D) -> Result<Self> {
        let mut remove = vec![];
        let mut update_content = vec![];
        let mut update_meta = vec![];
        for item in iter {
            let item: DiffEntry = item?;
            match item.diff_type {
                DiffType::LeftOnly(_) => remove.push(item.path),
                DiffType::RightOnly(meta) => update_content.push(UpdateContentAction {
                    path: item.path,
                    content_hgid: meta.hgid,
                    file_type: meta.file_type,
                }),
                DiffType::Changed(old, new) => {
                    if old.hgid == new.hgid {
                        let set_x_flag = match (old.file_type, new.file_type) {
                            (FileType::Executable, FileType::Regular) => false,
                            (FileType::Regular, FileType::Executable) => true,
                            // todo - address this case
                            // Since this is rare case we are going to handle it by submitting
                            // delete and then create operation to avoid complexity
                            (o, n) => bail!(
                                "Can not update {}: hg id has not changed and file type changed {:?}->{:?}",
                                item.path,
                                o,
                                n
                            ),
                        };
                        update_meta.push(UpdateMetaAction {
                            path: item.path,
                            set_x_flag,
                        });
                    } else {
                        update_content.push(UpdateContentAction {
                            path: item.path,
                            content_hgid: new.hgid,
                            file_type: new.file_type,
                        })
                    }
                }
            };
        }
        Ok(Self {
            remove,
            update_content,
            update_meta,
        })
    }

    // todo - tests
    // todo (VFS) - when writing simple file verify that destination is not a symlink
    // todo (VFS) - when writing symlink instead of regular file, remove it first
    /// Applies plan to the root using store to fetch data.
    /// This async function offloads file system operation to tokio blocking thread pool.
    /// It limits number of concurrent fs operations to PARALLEL_CHECKOUT.
    ///
    /// This function also designed to leverage async storage API(which we do not yet have).
    /// When updating content of the file/symlink, this function first creates list of HgId
    /// it needs to fetch. This list is then converted to stream and fed into storage for fetching
    ///
    /// As storage starts returning blobs of data, we start to kick off fs write operations in
    /// the tokio async worker pool. If more then PARALLEL_CHECKOUT fs operations are pending, we
    /// stop polling storage stream, until one of pending fs operations complete
    ///
    /// This function fails fast and returns error when first checkout operation fails.
    /// Pending storage futures are dropped when error is returned
    pub async fn apply<DS: HgIdDataStore>(self, vfs: &VFS, store: &DS) -> Result<()> {
        const PARALLEL_CHECKOUT: usize = 16;

        let remove_files = stream::iter(self.remove).map(|path| Self::remove_file(vfs, path));
        let remove_files = remove_files.buffer_unordered(PARALLEL_CHECKOUT);

        Self::process_work_stream(remove_files).await?;


        let keys: Vec<_> = self
            .update_content
            .iter()
            .map(|u| Key::new(u.path.clone(), u.content_hgid))
            .collect();

        // todo - replace store with async store when we have it
        // This does not call prefetch intentionally, since it will go away with async storage api
        let data_stream = stream::iter(keys.into_iter().map(|key| store.get(StoreKey::HgId(key))));

        let update_content = data_stream
            .zip(stream::iter(self.update_content.into_iter()))
            .map(|(data, action)| async move {
                let data = data
                    .map_err(|err| format_err!("Failed to fetch {:?}: {:?}", action.path, err))?;
                let data = match data {
                    StoreResult::Found(data) => data,
                    StoreResult::NotFound(key) => bail!("Key {:?} not found in data store", key),
                };
                let path = action.path;
                let flag = match action.file_type {
                    FileType::Regular => None,
                    FileType::Executable => Some(UpdateFlag::Executable),
                    FileType::Symlink => Some(UpdateFlag::Symlink),
                };

                Self::write_file(vfs, path, data, flag).await
            });

        let update_content = update_content.buffer_unordered(PARALLEL_CHECKOUT);

        let update_meta = stream::iter(self.update_meta)
            .map(|action| Self::set_exec_on_file(vfs, action.path, action.set_x_flag));
        let update_meta = update_meta.buffer_unordered(PARALLEL_CHECKOUT);

        let update_content = Self::process_work_stream(update_content);
        let update_meta = Self::process_work_stream(update_meta);

        try_join!(update_content, update_meta)?;

        Ok(())
    }

    /// Drains stream returning error if one of futures fail
    async fn process_work_stream<S: Stream<Item = Result<()>> + Unpin>(
        mut stream: S,
    ) -> Result<()> {
        while let Some(result) = stream.next().await {
            result?;
        }
        Ok(())
    }

    // Functions below use blocking fs operations in spawn_blocking proc.
    // As of today tokio::fs operations do the same.
    // Since we do multiple fs calls inside, it is beneficial to 'pack'
    // all of them into single spawn_blocking.

    // todo - create directories if needed
    async fn write_file(
        vfs: &VFS,
        path: RepoPathBuf,
        data: Vec<u8>,
        flag: Option<UpdateFlag>,
    ) -> Result<()> {
        let vfs = vfs.clone(); // vfs auditor cache is shared
        tokio::runtime::Handle::current()
            .spawn_blocking(move || vfs.write(path.as_repo_path(), &data.into(), flag))
            .await??;
        Ok(())
    }

    async fn remove_file(vfs: &VFS, path: RepoPathBuf) -> Result<()> {
        let vfs = vfs.clone(); // vfs auditor cache is shared
        tokio::runtime::Handle::current()
            .spawn_blocking(move || vfs.remove(path.as_repo_path()))
            .await??;
        Ok(())
    }

    async fn set_exec_on_file(vfs: &VFS, path: RepoPathBuf, flag: bool) -> Result<()> {
        let vfs = vfs.clone(); // vfs auditor cache is shared
        tokio::runtime::Handle::current()
            .spawn_blocking(move || vfs.set_executable(path.as_repo_path(), flag))
            .await??;
        Ok(())
    }
}
