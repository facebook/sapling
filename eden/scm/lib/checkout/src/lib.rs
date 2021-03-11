/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{bail, format_err, Result};
use futures::{stream, try_join, Stream, StreamExt};
use manifest::{DiffEntry, DiffType, FileMetadata, FileType, Manifest};
use pathmatcher::{Matcher, XorMatcher};
use revisionstore::{HgIdDataStore, RemoteDataStore, StoreKey, StoreResult};
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use types::{HgId, Key, RepoPath, RepoPathBuf};
use vfs::{AsyncVfsWriter, UpdateFlag, VFS};

const PREFETCH_CHUNK_SIZE: usize = 1000;
const VFS_BATCH_SIZE: usize = 100;

/// Contains lists of files to be removed / updated during checkout.
pub struct CheckoutPlan {
    /// Files to be removed.
    remove: Vec<RepoPathBuf>,
    /// Files that needs their content updated.
    update_content: Vec<UpdateContentAction>,
    /// Files that only need X flag updated.
    update_meta: Vec<UpdateMetaAction>,
}

/// Update content and (possibly) metadata on the file
#[derive(Debug)]
struct UpdateContentAction {
    /// Path to file.
    path: RepoPathBuf,
    /// If content has changed, HgId of new content.
    content_hgid: HgId,
    /// New file type.
    file_type: FileType,
}

/// Only update metadata on the file, do not update content
#[derive(Debug)]
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
                    update_content.push(UpdateContentAction::new(item.path, meta))
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
                            update_content.push(UpdateContentAction::new(item.path, new));
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

    /// Updates current plan to account for sparse profile change
    pub fn with_sparse_profile_change(
        mut self,
        old_matcher: &impl Matcher,
        new_matcher: &impl Matcher,
        new_manifest: &impl Manifest,
    ) -> Result<Self> {
        // First - remove all the files that were scheduled for update, but actually aren't in new sparse profile
        retain_paths(&mut self.update_content, new_matcher)?;
        retain_paths(&mut self.update_meta, new_matcher)?;

        // Second - handle files in a new manifest, that were affected by sparse profile change
        let xor_matcher = XorMatcher::new(old_matcher, new_matcher);
        for file in new_manifest.files(&xor_matcher) {
            let file = file?;
            if new_matcher.matches_file(&file.path)? {
                self.update_content
                    .push(UpdateContentAction::new(file.path, file.meta));
            } else {
                // by definition of xor matcher this means old_matcher.matches_file==true
                self.remove.push(file.path);
            }
        }

        Ok(self)
    }

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
        F: FnOnce(Vec<Key>) -> S,
    >(
        &self,
        vfs: &VFS,
        f: F,
    ) -> Result<CheckoutStats> {
        let async_vfs = &AsyncVfsWriter::spawn_new(vfs.clone(), 16);
        let stats = CheckoutStats::default();
        let stats_ref = &stats;
        const PARALLEL_CHECKOUT: usize = 16;

        let remove_files = stream::iter(self.remove.clone().into_iter())
            .chunks(VFS_BATCH_SIZE)
            .map(|paths| Self::remove_files(async_vfs, stats_ref, paths));
        let remove_files = remove_files.buffer_unordered(PARALLEL_CHECKOUT);

        Self::process_work_stream(remove_files).await?;

        let keys: Vec<_> = self
            .update_content
            .iter()
            .map(|u| Key::new(u.path.clone(), u.content_hgid))
            .collect();

        let data_stream = f(keys);

        let update_content = data_stream
            .zip(stream::iter(self.update_content.iter()))
            .map(|(data, action)| {
                let data = data
                    .map_err(|err| format_err!("Failed to fetch {:?}: {:?}", action.path, err))?;
                let data = match data {
                    StoreResult::Found(data) => data,
                    StoreResult::NotFound(key) => bail!("Key {:?} not found in data store", key),
                };
                let path = action.path.clone();
                let flag = type_to_flag(&action.file_type);
                Ok((path, data, flag))
            });

        let update_content = update_content
            .chunks(VFS_BATCH_SIZE)
            .map(|actions| async move {
                let actions: Result<Vec<_>, _> = actions.into_iter().collect();
                Self::write_files(async_vfs, stats_ref, actions?).await
            });

        let update_content = update_content.buffer_unordered(PARALLEL_CHECKOUT);

        let update_meta = stream::iter(self.update_meta.iter()).map(|action| {
            Self::set_exec_on_file(async_vfs, stats_ref, &action.path, action.set_x_flag)
        });
        let update_meta = update_meta.buffer_unordered(PARALLEL_CHECKOUT);

        let update_content = Self::process_work_stream(update_content);
        let update_meta = Self::process_work_stream(update_meta);

        try_join!(update_content, update_meta)?;

        Ok(stats)
    }

    pub async fn apply_data_store<DS: HgIdDataStore>(
        &self,
        vfs: &VFS,
        store: &DS,
    ) -> Result<CheckoutStats> {
        self.apply_stream(vfs, |keys| {
            stream::iter(keys.into_iter().map(|key| store.get(StoreKey::HgId(key))))
        })
        .await
    }

    pub async fn apply_remote_data_store<DS: RemoteDataStore + Clone + 'static>(
        &self,
        vfs: &VFS,
        store: &DS,
    ) -> Result<CheckoutStats> {
        use futures::channel::mpsc;
        self.apply_stream(vfs, |keys| {
            let (tx, rx) = mpsc::unbounded();
            let store = store.clone();
            tokio::runtime::Handle::current().spawn_blocking(move || {
                let keys: Vec<_> = keys.into_iter().map(StoreKey::HgId).collect();
                for chunk in keys.chunks(PREFETCH_CHUNK_SIZE) {
                    match store.prefetch(chunk) {
                        Err(e) => {
                            if tx.unbounded_send(Err(e)).is_err() {
                                return;
                            }
                        }
                        Ok(_) => {
                            for key in chunk {
                                if tx.unbounded_send(store.get(key.clone())).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                }
            });
            rx
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
    async fn write_files(
        async_vfs: &AsyncVfsWriter,
        stats: &CheckoutStats,
        actions: Vec<(RepoPathBuf, Vec<u8>, Option<UpdateFlag>)>,
    ) -> Result<()> {
        let count = actions.len();
        let w = async_vfs.write_batch(actions).await?;
        stats.updated.fetch_add(count, Ordering::Relaxed);
        stats.written_bytes.fetch_add(w, Ordering::Relaxed);
        Ok(())
    }

    async fn remove_files(
        async_vfs: &AsyncVfsWriter,
        stats: &CheckoutStats,
        paths: Vec<RepoPathBuf>,
    ) -> Result<()> {
        let count = paths.len();
        async_vfs.remove_batch(paths).await?;
        stats.removed.fetch_add(count, Ordering::Relaxed);
        Ok(())
    }

    async fn set_exec_on_file(
        async_vfs: &AsyncVfsWriter,
        stats: &CheckoutStats,
        path: &RepoPath,
        flag: bool,
    ) -> Result<()> {
        async_vfs.set_executable(path.to_owned(), flag).await?;
        stats.meta_updated.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn removed_files(&self) -> impl Iterator<Item = &RepoPathBuf> {
        self.remove.iter()
    }

    pub fn updated_content_files(&self) -> impl Iterator<Item = &RepoPathBuf> {
        self.update_content.iter().map(|u| &u.path)
    }

    pub fn updated_meta_files(&self) -> impl Iterator<Item = &RepoPathBuf> {
        self.update_meta.iter().map(|u| &u.path)
    }

    /// Returns (updated, removed)
    pub fn stats(&self) -> (usize, usize) {
        (
            self.update_meta.len() + self.update_content.len(),
            self.remove.len(),
        )
    }

    #[cfg(test)]
    pub fn empty() -> Self {
        Self {
            remove: vec![],
            update_content: vec![],
            update_meta: vec![],
        }
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
    pub fn new(path: RepoPathBuf, meta: FileMetadata) -> Self {
        Self {
            path,
            content_hgid: meta.hgid,
            file_type: meta.file_type,
        }
    }
}

fn retain_paths<T: AsRef<RepoPath>>(v: &mut Vec<T>, matcher: impl Matcher) -> Result<()> {
    let mut result = Ok(());
    v.retain(|p| {
        if result.is_err() {
            return true;
        }
        match matcher.matches_file(p.as_ref()) {
            Ok(v) => v,
            Err(err) => {
                result = Err(err);
                true
            }
        }
    });
    result
}

impl AsRef<RepoPath> for UpdateContentAction {
    fn as_ref(&self) -> &RepoPath {
        &self.path
    }
}

impl AsRef<RepoPath> for UpdateMetaAction {
    fn as_ref(&self) -> &RepoPath {
        &self.path
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
    use pathmatcher::{AlwaysMatcher, TreeMatcher};
    use quickcheck::{Arbitrary, StdGen};
    use rand::SeedableRng;
    use rand_chacha::ChaChaRng;
    use std::collections::HashMap;
    use std::path::Path;
    use tempfile::TempDir;
    use types::testutil::generate_repo_paths;
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
        // regular<->symlink
        assert_checkout_symmetrical(&[a.clone()], &[a_s.clone()]).await?;
        // dir <-> file with the same name
        assert_checkout_symmetrical(&[ab.clone()], &[a.clone()]).await?;
        // create / rm dir
        assert_checkout_symmetrical(&[ab.clone()], &[b.clone()]).await?;
        // mv file between dirs
        assert_checkout(&[ab.clone()], &[cd.clone()]).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_checkout_generated() -> Result<()> {
        let trees = generate_trees(6, 50);
        for a in trees.iter() {
            for b in trees.iter() {
                if a == b {
                    continue;
                }
                assert_checkout(a, b).await?;
            }
        }
        Ok(())
    }

    #[test]
    fn test_with_sparse_profile_change() -> Result<()> {
        let a = (rp("a"), FileMetadata::regular(hgid(1)));
        let b = (rp("b"), FileMetadata::regular(hgid(2)));
        let c = (rp("c"), FileMetadata::regular(hgid(3)));
        let ab_profile = TreeMatcher::from_rules(["a/**", "b/**"].iter())?;
        let ac_profile = TreeMatcher::from_rules(["a/**", "c/**"].iter())?;
        let manifest = make_tree_manifest_from_meta(vec![a, b, c]);

        let plan = CheckoutPlan::empty().with_sparse_profile_change(
            &ab_profile,
            &ab_profile,
            &manifest,
        )?;
        assert_eq!("", &plan.to_string());

        let plan = CheckoutPlan::empty().with_sparse_profile_change(
            &ab_profile,
            &ac_profile,
            &manifest,
        )?;
        assert_eq!(
            "rm b\nup c=>0300000000000000000000000000000000000000\n",
            &plan.to_string()
        );

        let mut plan = CheckoutPlan::empty();
        plan.update_content.push(UpdateContentAction::new(
            rp("b"),
            FileMetadata::regular(hgid(10)),
        ));
        plan.update_meta.push(UpdateMetaAction {
            path: rp("b"),
            set_x_flag: true,
        });
        let plan = plan.with_sparse_profile_change(&ab_profile, &ac_profile, &manifest)?;
        assert_eq!(
            "rm b\nup c=>0300000000000000000000000000000000000000\n",
            &plan.to_string()
        );

        Ok(())
    }

    fn generate_trees(tree_size: usize, count: usize) -> Vec<Vec<(RepoPathBuf, FileMetadata)>> {
        let mut result = vec![];
        let rng = ChaChaRng::from_seed([0u8; 32]);
        let mut gen = StdGen::new(rng, 5);
        let paths = generate_repo_paths(tree_size * count, &mut gen);

        for i in 0..count {
            let mut tree = vec![];
            for idx in 0..tree_size {
                let meta = FileMetadata::arbitrary(&mut gen);
                let path = paths.get(i * tree_size / 2 + idx).unwrap().clone();
                tree.push((path, meta));
            }
            result.push(tree)
        }
        result
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
            vfs.write(path.as_repo_path(), &data, flag)?;
        }
        Ok(())
    }

    fn assert_fs(root: &Path, expected: &[(RepoPathBuf, FileMetadata)]) -> Result<()> {
        let mut expected: HashMap<_, _> = expected.iter().cloned().collect();
        for dir in WalkDir::new(root).into_iter() {
            let dir = dir?;
            if dir.file_type().is_dir() {
                assert_not_empty_dir(&dir)?;
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

    fn assert_not_empty_dir(dir: &DirEntry) -> Result<()> {
        let mut rd = std::fs::read_dir(dir.path())?;
        if rd.next().is_none() {
            bail!("Unexpected empty dir: {}", dir.path().display())
        }
        Ok(())
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

    fn dummy_fs(v: Vec<Key>) -> impl Stream<Item = Result<StoreResult<Vec<u8>>>> {
        stream::iter(v).map(|key| Ok(StoreResult::Found(hgid_file(&key.hgid))))
    }

    fn hgid_file(hgid: &HgId) -> Vec<u8> {
        hgid.to_string().into_bytes()
    }
}

impl fmt::Display for CheckoutPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for r in &self.remove {
            writeln!(f, "rm {}", r)?;
        }
        for u in &self.update_content {
            let ft = match u.file_type {
                FileType::Executable => "(x)",
                FileType::Symlink => "(s)",
                FileType::Regular => "",
            };
            writeln!(f, "up {}=>{}{}", u.path, u.content_hgid, ft)?;
        }
        for u in &self.update_meta {
            let ch = if u.set_x_flag { "+x" } else { "-x" };
            writeln!(f, "{} {}", ch, u.path)?;
        }
        Ok(())
    }
}
