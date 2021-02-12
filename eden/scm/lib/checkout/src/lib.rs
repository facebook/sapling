/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{bail, format_err, Result};
use futures::{stream, try_join, Stream, StreamExt};
use manifest::{DiffEntry, DiffType, FileMetadata, FileType};
use revisionstore::{HgIdDataStore, RemoteDataStore, StoreKey, StoreResult};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
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

#[derive(Default)]
pub struct CheckoutStats {
    removed: AtomicUsize,
    updated: AtomicUsize,
    meta_updated: AtomicUsize,
    written_bytes: AtomicUsize,
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
                DiffType::RightOnly(meta) => {
                    update_content.push(UpdateContentAction::new(item, meta))
                }
                DiffType::Changed(old, new) => {
                    match (old.hgid == new.hgid, old.file_type, new.file_type) {
                        (true, FileType::Executable, FileType::Regular) => {
                            update_meta.push(UpdateMetaAction {
                                path: item.path,
                                set_x_flag: false,
                            });
                        }
                        (true, FileType::Regular, FileType::Executable) => {
                            update_meta.push(UpdateMetaAction {
                                path: item.path,
                                set_x_flag: true,
                            });
                        }
                        _ => {
                            update_content.push(UpdateContentAction::new(item, new));
                        }
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
    pub async fn apply_stream<
        S: Stream<Item = Result<StoreResult<Vec<u8>>>> + Unpin,
        F: FnOnce(Vec<Key>) -> Result<S>,
    >(
        self,
        vfs: &VFS,
        f: F,
    ) -> Result<CheckoutStats> {
        let stats_arc = Arc::new(CheckoutStats::default());
        let stats = &stats_arc;
        const PARALLEL_CHECKOUT: usize = 16;

        let remove_files =
            stream::iter(self.remove).map(|path| Self::remove_file(vfs, stats, path));
        let remove_files = remove_files.buffer_unordered(PARALLEL_CHECKOUT);

        Self::process_work_stream(remove_files).await?;

        let keys: Vec<_> = self
            .update_content
            .iter()
            .map(|u| Key::new(u.path.clone(), u.content_hgid))
            .collect();

        let data_stream = f(keys)?;

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
                let flag = type_to_flag(&action.file_type);

                Self::write_file(vfs, stats, path, data, flag).await
            });

        let update_content = update_content.buffer_unordered(PARALLEL_CHECKOUT);

        let update_meta = stream::iter(self.update_meta)
            .map(|action| Self::set_exec_on_file(vfs, stats, action.path, action.set_x_flag));
        let update_meta = update_meta.buffer_unordered(PARALLEL_CHECKOUT);

        let update_content = Self::process_work_stream(update_content);
        let update_meta = Self::process_work_stream(update_meta);

        try_join!(update_content, update_meta)?;

        Ok(Arc::try_unwrap(stats_arc)
            .ok()
            .expect("Failed to unwrap stats - lingering workers?"))
    }

    pub async fn apply_data_store<DS: HgIdDataStore>(
        self,
        vfs: &VFS,
        store: &DS,
    ) -> Result<CheckoutStats> {
        self.apply_stream(vfs, |keys| {
            Ok(stream::iter(
                keys.into_iter().map(|key| store.get(StoreKey::HgId(key))),
            ))
        })
        .await
    }

    pub async fn apply_remote_data_store<DS: RemoteDataStore>(
        self,
        vfs: &VFS,
        store: &DS,
    ) -> Result<CheckoutStats> {
        self.apply_stream(vfs, |keys| {
            let store_keys: Vec<_> = keys.into_iter().map(StoreKey::HgId).collect();
            store.prefetch(&store_keys)?;
            Ok(stream::iter(
                store_keys.into_iter().map(|key| store.get(key)),
            ))
        })
        .await
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
    async fn write_file(
        vfs: &VFS,
        stats: &Arc<CheckoutStats>,
        path: RepoPathBuf,
        data: Vec<u8>,
        flag: Option<UpdateFlag>,
    ) -> Result<()> {
        let vfs = vfs.clone(); // vfs auditor cache is shared
        let stats = Arc::clone(stats);
        tokio::runtime::Handle::current()
            .spawn_blocking(move || -> Result<()> {
                let repo_path = path.as_repo_path();
                let w = vfs.write(repo_path, &data.into(), flag)?;
                stats.updated.fetch_add(1, Ordering::Relaxed);
                stats.written_bytes.fetch_add(w, Ordering::Relaxed);
                Ok(())
            })
            .await??;
        Ok(())
    }

    async fn remove_file(vfs: &VFS, stats: &Arc<CheckoutStats>, path: RepoPathBuf) -> Result<()> {
        let vfs = vfs.clone(); // vfs auditor cache is shared
        let stats = Arc::clone(stats);
        tokio::runtime::Handle::current()
            .spawn_blocking(move || -> Result<()> {
                vfs.remove(path.as_repo_path())?;
                stats.removed.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })
            .await??;
        Ok(())
    }

    async fn set_exec_on_file(
        vfs: &VFS,
        stats: &Arc<CheckoutStats>,
        path: RepoPathBuf,
        flag: bool,
    ) -> Result<()> {
        let vfs = vfs.clone(); // vfs auditor cache is shared
        let stats = Arc::clone(stats);
        tokio::runtime::Handle::current()
            .spawn_blocking(move || -> Result<()> {
                vfs.set_executable(path.as_repo_path(), flag)?;
                stats.meta_updated.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })
            .await??;
        Ok(())
    }
}

// todo: possibly migrate VFS api to use FileType?
fn type_to_flag(ft: &FileType) -> Option<UpdateFlag> {
    match ft {
        FileType::Regular => None,
        FileType::Executable => Some(UpdateFlag::Executable),
        FileType::Symlink => Some(UpdateFlag::Symlink),
    }
}

impl UpdateContentAction {
    pub fn new(item: DiffEntry, meta: FileMetadata) -> Self {
        Self {
            path: item.path,
            content_hgid: meta.hgid,
            file_type: meta.file_type,
        }
    }
}

#[cfg(test)]
// todo - consider moving some of this code to vfs / separate test create
// todo parallel execution for the test
mod test {
    use super::*;
    use anyhow::ensure;
    use anyhow::Context;
    use manifest_tree::testutil::make_tree_manifest_from_meta;
    use manifest_tree::Diff;
    use pathmatcher::AlwaysMatcher;
    use std::collections::HashMap;
    use std::path::Path;
    use tempfile::TempDir;
    use walkdir::{DirEntry, WalkDir};

    #[tokio::test]
    async fn test_basic_checkout() -> Result<()> {
        // Pattern - lowercase_path_[hgid!=1]_[flags!=normal]
        let a = (rp("A"), FileMetadata::regular(hgid(1)));
        let a_2 = (rp("A"), FileMetadata::regular(hgid(2)));
        let a_e = (rp("A"), FileMetadata::executable(hgid(1)));
        let a_s = (rp("A"), FileMetadata::symlink(hgid(1)));
        let b = (rp("B"), FileMetadata::regular(hgid(1)));
        let ab = (rp("A/B"), FileMetadata::regular(hgid(1)));
        let cd = (rp("C/D"), FileMetadata::regular(hgid(1)));

        // update file
        assert_checkout(&[a.clone()], &[a_2.clone()]).await?;
        // mv file
        assert_checkout(&[a.clone()], &[b.clone()]).await?;
        // add / rm file
        assert_checkout_symmetrical(&[a.clone()], &[a.clone(), b.clone()]).await?;
        // regular<->exec
        assert_checkout_symmetrical(&[a.clone()], &[a_e.clone()]).await?;
        // regular->symlink
        assert_checkout(&[a.clone()], &[a_s.clone()]).await?;
        // symlink->regular - todo - does not currently work
        // assert_checkout(vec![a_s.clone()], vec![a.clone()]).await?;
        // dir <-> file with the same name
        assert_checkout_symmetrical(&[ab.clone()], &[a.clone()]).await?;
        // mv file between dirs
        assert_checkout(&[ab.clone()], &[cd.clone()]).await?;

        Ok(())
    }

    fn rp(p: &str) -> RepoPathBuf {
        RepoPathBuf::from_string(p.to_string()).unwrap()
    }

    fn hgid(p: u8) -> HgId {
        let mut r = HgId::default().into_byte_array();
        r[0] = p;
        HgId::from_byte_array(r)
    }

    async fn assert_checkout_symmetrical(
        a: &[(RepoPathBuf, FileMetadata)],
        b: &[(RepoPathBuf, FileMetadata)],
    ) -> Result<()> {
        assert_checkout(a, b).await?;
        assert_checkout(b, a).await
    }

    async fn assert_checkout(
        from: &[(RepoPathBuf, FileMetadata)],
        to: &[(RepoPathBuf, FileMetadata)],
    ) -> Result<()> {
        let tempdir = tempfile::tempdir()?;
        if let Err(e) = assert_checkout_impl(from, to, &tempdir).await {
            eprintln!("===");
            eprintln!("Failed transitioning from tree");
            print_tree(&from);
            eprintln!("To tree");
            print_tree(&to);
            eprintln!("===");
            eprintln!(
                "Working directory: {} (not deleted)",
                tempdir.into_path().display()
            );
            return Err(e);
        }
        Ok(())
    }

    async fn assert_checkout_impl(
        from: &[(RepoPathBuf, FileMetadata)],
        to: &[(RepoPathBuf, FileMetadata)],
        tempdir: &TempDir,
    ) -> Result<()> {
        let vfs = VFS::new(tempdir.path().to_path_buf())?;
        roll_out_fs(&vfs, from)?;

        let matcher = AlwaysMatcher::new();
        let left_tree = make_tree_manifest_from_meta(from.iter().cloned());
        let right_tree = make_tree_manifest_from_meta(to.iter().cloned());
        let diff = Diff::new(&left_tree, &right_tree, &matcher);
        let plan = CheckoutPlan::from_diff(diff).context("Plan construction failed")?;

        // Use clean vfs for test
        let vfs = VFS::new(tempdir.path().to_path_buf())?;
        plan.apply_stream(&vfs, dummy_fs)
            .await
            .context("Plan execution failed")?;

        assert_fs(tempdir.path(), to)
    }

    fn print_tree(t: &[(RepoPathBuf, FileMetadata)]) {
        for (path, meta) in t {
            eprintln!("{} [{:?}]", path, meta);
        }
    }

    fn roll_out_fs(vfs: &VFS, files: &[(RepoPathBuf, FileMetadata)]) -> Result<()> {
        for (path, meta) in files {
            let flag = type_to_flag(&meta.file_type);
            let data = hgid_file(&meta.hgid);
            vfs.write(path.as_repo_path(), &data.into(), flag)?;
        }
        Ok(())
    }

    fn assert_fs(root: &Path, expected: &[(RepoPathBuf, FileMetadata)]) -> Result<()> {
        let mut expected: HashMap<_, _> = expected.iter().cloned().collect();
        for dir in WalkDir::new(root).into_iter() {
            let dir = dir?;
            if dir.file_type().is_dir() {
                // todo check is not empty
                continue;
            }
            let rel_path = dir.path().strip_prefix(root)?;
            let rel_path = into_repo_path(rel_path.to_string_lossy().into_owned());
            let rel_path = RepoPathBuf::from_string(rel_path)?;
            let expected_meta = if let Some(m) = expected.remove(&rel_path) {
                m
            } else {
                bail!("Checkout created unexpected file {}", rel_path);
            };
            assert_metadata(&expected_meta, &dir)?;
        }
        if !expected.is_empty() {
            bail!(
                "Some files are not present after checkout: {:?}",
                expected.keys().collect::<Vec<_>>()
            );
        }
        Ok(())
    }

    #[cfg(not(windows))]
    fn into_repo_path(path: String) -> String {
        path
    }

    #[cfg(windows)]
    fn into_repo_path(path: String) -> String {
        path.replace("\\", "/")
    }

    fn assert_metadata(expected: &FileMetadata, actual: &DirEntry) -> Result<()> {
        match expected.file_type {
            FileType::Regular => assert_regular(actual),
            FileType::Executable => assert_exec(actual),
            FileType::Symlink => assert_symlink(actual),
        }
    }

    // When compiling on unknown platform will get function not defined compile error and will need to address it

    #[cfg(unix)] // This is where PermissionsExt is defined
    fn assert_regular(actual: &DirEntry) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let meta = actual.metadata()?;
        ensure!(
            meta.permissions().mode() & 0o111 == 0,
            "Expected {} to be a regular file, actual mode {:#o}",
            actual.path().display(),
            meta.permissions().mode()
        );
        Ok(())
    }

    #[cfg(unix)]
    fn assert_exec(actual: &DirEntry) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let meta = actual.metadata()?;
        ensure!(
            meta.permissions().mode() & 0o111 != 0,
            "Expected {} to be a executable file, actual mode {:#o}",
            actual.path().display(),
            meta.permissions().mode()
        );
        Ok(())
    }

    #[cfg(unix)]
    fn assert_symlink(actual: &DirEntry) -> Result<()> {
        ensure!(
            actual.path_is_symlink(),
            "Expected {} to be a symlink",
            actual.path().display()
        );
        Ok(())
    }

    #[cfg(windows)]
    fn assert_regular(_actual: &DirEntry) -> Result<()> {
        Ok(())
    }

    #[cfg(windows)]
    fn assert_exec(_actual: &DirEntry) -> Result<()> {
        Ok(())
    }

    #[cfg(windows)]
    fn assert_symlink(_actual: &DirEntry) -> Result<()> {
        Ok(())
    }

    fn dummy_fs(v: Vec<Key>) -> Result<impl Stream<Item = Result<StoreResult<Vec<u8>>>>> {
        Ok(stream::iter(v).map(|key| Ok(StoreResult::Found(hgid_file(&key.hgid)))))
    }

    fn hgid_file(hgid: &HgId) -> Vec<u8> {
        hgid.to_string().into_bytes()
    }
}
