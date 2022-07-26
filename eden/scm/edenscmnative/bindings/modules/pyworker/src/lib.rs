/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::cell::RefCell;
use std::mem;
use std::path::PathBuf;
use std::str;
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;

use anyhow::bail;
use anyhow::format_err;
use anyhow::Context;
use anyhow::Result;
use cpython::*;
use cpython_ext::ExtractInner;
use cpython_ext::PyNone;
use cpython_ext::PyPath;
use cpython_ext::PyPathBuf;
use cpython_ext::ResultPyErrExt;
use cpython_ext::Str;
use crossbeam::channel::bounded;
use crossbeam::channel::Receiver;
use crossbeam::channel::Sender;
use pyrevisionstore::contentstore;
use pyrevisionstore::filescmstore;
use revisionstore::datastore::RemoteDataStore;
use revisionstore::localstore::LocalStore;
use revisionstore::redact_if_needed;
use revisionstore::HgIdDataStore;
use revisionstore::LegacyStore;
use revisionstore::StoreKey;
use revisionstore::StoreResult;
use types::HgId;
use types::Key;
use types::RepoPathBuf;
use vfs::UpdateFlag;
use vfs::VFS;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "worker"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<writerworker>(py)?;
    m.add_class::<removerworker>(py)?;
    Ok(m)
}

/// The `Worker` will batch up to `WORKER_BATCH_SIZE` work items before sending them to the
/// individual threads.
const WORKER_BATCH_SIZE: usize = 100;

struct Worker<Ret, Work> {
    threads: Vec<JoinHandle<Ret>>,
    sender: Sender<Vec<Work>>,
    pending: Vec<Work>,
}

impl<Ret: Send + 'static, Work: Sync + Send + 'static> Worker<Ret, Work> {
    /// Create a new worker.
    ///
    /// The internal channel size is bounded in size which makes the `push_work`
    /// method blocking. This is needed to prevent work from simply accumulating
    /// into it, having a bound allows for pushes to be blocking which allows for
    /// some good progress reporting to the user. The size of the channel was
    /// chosen to be twice the number of workers as a way to always keep the
    /// workers busy. As long as we can push one unit of work faster than the
    /// workers can process, the overall performance won't be bound to the Python
    /// threads.
    fn new<State: Send + Clone + 'static>(
        num_workers: usize,
        state: State,
        work: impl Fn(State, Receiver<Vec<Work>>) -> Ret + Send + 'static + Copy,
    ) -> Self {
        let (sender, receiver) = bounded(num_workers * 2);

        let mut threads = Vec::with_capacity(num_workers);
        for _ in 1..=num_workers {
            let chan = receiver.clone();
            let worker_state = state.clone();

            threads.push(thread::spawn(move || work(worker_state, chan)));
        }

        let pending = Vec::with_capacity(WORKER_BATCH_SIZE);

        Self {
            threads,
            sender,
            pending,
        }
    }

    /// Push work to the workers. The pushed work will be picked up by an idle
    /// worker, if the amount of pending work is already too high, this method will
    /// release the Python GIL and block.
    ///
    /// We need to batch work to avoid many pitfalls:
    ///  1) Sending data over the channel will involve locking/unlocking it,
    ///     and getting work from it will also involve similar locking. In order
    ///     to keep the workers busy, we need to minimize the amount of time that
    ///     all threads spend waiting for this lock.
    ///  2) Similarly, since the queue is bounded in size, we need to release
    ///     the GIL when pushing to it so that the other Python threads can run,
    ///     acquiring/releasing the GIL too often will also lead to reduced
    ///     performance.
    ///  3) Since pushing work to the worker is single threaded, the batched work
    ///     has a high chance of being on the same files, increasing locality.
    /// Caveat: the batch size was chosen semi-arbitrarily, and should be
    /// tweaked, too high of a value, and the progress reporting won't be
    /// good, too small of a value and the performance will suffer.
    fn push_work(&mut self, py: Python, work: Work) -> Result<()> {
        self.pending.push(work);

        if self.pending.len() == WORKER_BATCH_SIZE {
            // Release the GIL so other Python threads (the progress bar for instance) have a
            // chance to run while we're blocked in `send`.
            py.allow_threads(move || -> Result<()> {
                let pending =
                    mem::replace(&mut self.pending, Vec::with_capacity(WORKER_BATCH_SIZE));
                // This may block if the channel is full, see the comment in the
                // constructor for details.
                self.sender.send(pending)?;
                Ok(())
            })?;
        }
        Ok(())
    }

    /// Wait until all the previously pushed work have completed. Return an
    /// iterator over all the threads results.
    fn wait(mut self) -> Result<impl Iterator<Item = Ret>> {
        let pending = mem::take(&mut self.pending);
        self.sender.send(pending)?;
        drop(self.sender);

        let threads = mem::take(&mut self.threads);

        Ok(threads.into_iter().map(|thread| thread.join().unwrap()))
    }
}

/// Fetch the content of the passed in `key` and write it to it's path.
fn update(state: &WriterState, key: Key, flag: UpdateFlag) -> Result<usize> {
    let content = state
        .store
        .get_file_content(&key)?
        .ok_or_else(|| format_err!("Can't find key: {}", key))?;

    let meta = match state.store.get_meta(StoreKey::hgid(key.clone()))? {
        StoreResult::NotFound(key) => {
            return Err(format_err!("Can't find metadata for key: {:?}", key));
        }
        StoreResult::Found(meta) => meta,
    };

    if meta.is_lfs() {
        bail!("LFS pointers cannot be deserialized properly yet");
    }

    let content = redact_if_needed(content);

    state.working_copy.write(&key.path, &content, flag)
}

fn threaded_writer(
    state: WriterState,
    chan: Receiver<Vec<(Key, UpdateFlag)>>,
) -> (usize, Vec<(RepoPathBuf, UpdateFlag)>) {
    let mut failures = Vec::new();

    let mut written = 0;
    while let Ok(vec) = chan.recv() {
        let store_keys: Vec<_> = vec.iter().map(|(k, _)| StoreKey::hgid(k.clone())).collect();
        let missing = match state.store.get_missing(&store_keys) {
            Ok(missing) => missing,
            Err(e) => {
                tracing::warn!("{:?}", e);
                let failed_inputs: Vec<_> = vec.into_iter().map(|(k, f)| (k.path, f)).collect();
                failures.extend_from_slice(&failed_inputs);
                continue;
            }
        };
        if !missing.is_empty() {
            // Any errors will get reported below.
            let _ = state.store.prefetch(&missing);
        }

        for (key, flag) in vec.into_iter() {
            let res = update(&state, key.clone(), flag)
                .with_context(|| format!("Can't update {} at {}", key.path, key.hgid));

            match res {
                Ok(count) => written += count,
                Err(e) => {
                    tracing::warn!("{:?}", e);
                    failures.push((key.path, flag));
                }
            };
        }
    }

    (written, failures)
}

#[derive(Clone)]
struct WriterState {
    store: Arc<dyn LegacyStore>,
    working_copy: VFS,
}

impl WriterState {
    pub fn new(root: PathBuf, store: Arc<dyn LegacyStore>) -> Result<Self> {
        let working_copy = VFS::new(root)?;
        Ok(Self {
            store,
            working_copy,
        })
    }
}

py_class!(class writerworker |py| {
    data inner: RefCell<Option<Worker<(usize, Vec<(RepoPathBuf, UpdateFlag)>), (Key, UpdateFlag)>>>;

    def __new__(_cls, store: PyObject, root: &PyPath, numthreads: usize) -> PyResult<writerworker> {
        let store = contentstore::downcast_from(py, store.clone_ref(py)).map(|s| s.extract_inner(py) as Arc<dyn LegacyStore>)
            .or_else(|_| filescmstore::downcast_from(py, store).map(|s|  s.extract_inner(py) as Arc<dyn LegacyStore>))?;

        let root = root.to_path_buf();
        let writer_state = WriterState::new(root, store).map_pyerr(py)?;

        let inner = Worker::new(numthreads, writer_state, threaded_writer);

        writerworker::create_instance(py, RefCell::new(Some(inner)))
    }

    /// Issue an asynchronous write call. The request will be picked up by a
    /// background thread which will write the content corresponding to `node` to
    /// the file `name`. May block and release the GIL when too much work is
    /// pending.
    def write(&self, name: &PyPath, node: &PyBytes, flags: &str) -> PyResult<PyNone> {
        let path = name.to_repo_path_buf().map_pyerr(py)?;
        let node = HgId::from_slice(node.data(py)).map_pyerr(py)?;

        let flags = if flags == "l" {
            UpdateFlag::Symlink
        } else if flags == "x" {
            UpdateFlag::Executable
        } else if flags == "" {
            UpdateFlag::Regular
        } else {
            return Err(format_err!("Unknown flags: {}", flags)).map_pyerr(py);
        };

        self.inner(py).borrow_mut().as_mut().unwrap().push_work(py, (Key::new(path, node), flags)).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Wait for all the pending `write` calls to complete.
    def wait(&self) -> PyResult<(usize, Vec<(PyPathBuf, Str)>)> {
        let inner = self.inner(py).borrow_mut().take().unwrap();

        py.allow_threads(move || -> Result<(usize, Vec<(PyPathBuf, Str)>)> {
            let mut written = 0;
            let mut failures = Vec::new();

            for (count, fail) in inner.wait()? {
                written += count;
                failures.extend(fail.into_iter().map(|(path, flag)| {
                    let path = PyPathBuf::from(path);

                    let flags = match flag {
                        UpdateFlag::Regular => Str::from("".to_string()),
                        UpdateFlag::Symlink => Str::from("l".to_string()),
                        UpdateFlag::Executable => Str::from("x".to_string()),
                    };

                    (path, flags)
                }));
            }

            Ok((written, failures))
        }).map_pyerr(py)
    }
});

fn threaded_remover(state: RemoverState, chan: Receiver<Vec<RepoPathBuf>>) -> Vec<RepoPathBuf> {
    let mut failures = Vec::new();

    while let Ok(vec) = chan.recv() {
        for path in vec.into_iter() {
            if let Err(e) = state.working_copy.remove(&path) {
                tracing::warn!("{:?}", e);
                failures.push(path);
            }
        }
    }

    failures
}

#[derive(Clone)]
struct RemoverState {
    working_copy: VFS,
}

impl RemoverState {
    pub fn new(root: PathBuf) -> Result<Self> {
        let working_copy = VFS::new(root)?;
        Ok(Self { working_copy })
    }
}

py_class!(class removerworker |py| {
    data inner: RefCell<Option<Worker<Vec<RepoPathBuf>, RepoPathBuf>>>;

    def __new__(_cls, root: &PyPath, numthreads: usize) -> PyResult<removerworker> {
        let root = root.to_path_buf();
        let remover_state = RemoverState::new(root).map_pyerr(py)?;

        let inner = Worker::new(numthreads, remover_state, threaded_remover);

        removerworker::create_instance(py, RefCell::new(Some(inner)))
    }

    /// Issue an asynchronous remove call. The request will be processed by a
    /// background thread. May block and release the GIL when too much work is pending.
    def remove(&self, name: &PyPath) -> PyResult<PyNone> {
        let path = name.to_repo_path_buf().map_pyerr(py)?;
        self.inner(py).borrow_mut().as_mut().unwrap().push_work(py, path).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Wait for all the pending `remove` calls to complete
    def wait(&self) -> PyResult<Vec<PyPathBuf>> {
        let inner = self.inner(py).borrow_mut().take().unwrap();

        let failures = py.allow_threads(move || -> Result<Vec<_>> {
            Ok(inner.wait()?.flatten().map(|path| PyPathBuf::from(path)).collect::<Vec<PyPathBuf>>())
        }).map_pyerr(py)?;

        Ok(failures)
    }
});

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs::create_dir_all;
    use std::fs::metadata;
    use std::fs::read_dir;
    use std::fs::read_to_string;
    use std::fs::symlink_metadata;
    use std::fs::File;
    #[cfg(windows)]
    use std::fs::OpenOptions;
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(windows)]
    use std::os::windows::fs::OpenOptionsExt;

    use anyhow::ensure;
    use memmap::MmapOptions;
    use minibytes::Bytes;
    use quickcheck::quickcheck;
    use quickcheck::TestResult;
    use revisionstore::datastore::Delta;
    use revisionstore::datastore::HgIdMutableDeltaStore;
    use revisionstore::testutil::make_config;
    use revisionstore::ContentStore;
    use tempfile::TempDir;
    use types::testutil::key;
    use types::RepoPath;

    use super::*;

    #[test]
    fn test_update_basic() -> Result<()> {
        let workingdir = TempDir::new()?;
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);
        let store = Arc::new(ContentStore::new(&localdir, &config)?);

        let k = key("a", "2");
        let delta = Delta {
            data: Bytes::from("CONTENT"),
            base: None,
            key: k.clone(),
        };
        store.add(&delta, &Default::default())?;

        let root = workingdir.as_ref().to_path_buf();
        let state = WriterState::new(root, store)?;
        let written = update(&state, k, UpdateFlag::Regular)?;

        assert_eq!(written, 7);

        Ok(())
    }

    #[test]
    fn test_executable() -> Result<()> {
        let workingdir = TempDir::new()?;
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);
        let store = Arc::new(ContentStore::new(&localdir, &config)?);

        let k = key("a", "2");
        let delta = Delta {
            data: Bytes::from("CONTENT"),
            base: None,
            key: k.clone(),
        };
        store.add(&delta, &Default::default())?;

        let root = workingdir.as_ref().to_path_buf();
        let state = WriterState::new(root, store)?;
        update(&state, k, UpdateFlag::Executable)?;

        let mut file = workingdir.as_ref().to_path_buf();
        file.push("a");

        let perms = metadata(&file)?.permissions();

        assert!(!perms.readonly());

        #[cfg(not(windows))]
        assert_eq!(perms.mode() & 0o755, 0o755);

        Ok(())
    }

    #[test]
    fn test_symlink() -> Result<()> {
        let workingdir = TempDir::new()?;
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);
        let store = Arc::new(ContentStore::new(&localdir, &config)?);

        let k = key("a", "2");
        let delta = Delta {
            data: Bytes::from("b"),
            base: None,
            key: k.clone(),
        };
        store.add(&delta, &Default::default())?;

        let root = workingdir.as_ref().to_path_buf();
        let state = WriterState::new(root, store)?;
        update(&state, k, UpdateFlag::Symlink)?;

        let mut file = workingdir.as_ref().to_path_buf();
        file.push("a");

        let file_type = symlink_metadata(&file)?.file_type();

        if cfg!(windows) {
            assert!(file_type.is_file());
        } else {
            assert!(file_type.is_symlink());
        }

        Ok(())
    }

    #[test]
    fn test_update_nested() -> Result<()> {
        let workingdir = TempDir::new()?;
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);
        let store = Arc::new(ContentStore::new(&localdir, &config)?);

        let k = key("a/b/c/d/e", "2");
        let delta = Delta {
            data: Bytes::from("CONTENT"),
            base: None,
            key: k.clone(),
        };
        store.add(&delta, &Default::default())?;

        let root = workingdir.as_ref().to_path_buf();
        let state = WriterState::new(root, store)?;
        let written = update(&state, k, UpdateFlag::Regular)?;
        assert_eq!(written, 7);

        Ok(())
    }

    #[test]
    fn test_update_replace_file_with_dir() -> Result<()> {
        let workingdir = TempDir::new()?;
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);
        let store = Arc::new(ContentStore::new(&localdir, &config)?);

        let k = key("a/b/c/d/e", "2");
        let delta = Delta {
            data: Bytes::from("CONTENT"),
            base: None,
            key: k.clone(),
        };
        store.add(&delta, &Default::default())?;

        let mut path = workingdir.as_ref().to_path_buf();
        path.extend(&["a", "b"]);
        create_dir_all(&path)?;
        path.push("c");
        File::create(&path)?;

        let root = workingdir.as_ref().to_path_buf();
        let state = WriterState::new(root, store)?;
        let written = update(&state, k, UpdateFlag::Regular)?;
        assert_eq!(written, 7);

        let mut path = workingdir.as_ref().to_path_buf();
        path.extend(&["a", "b", "c", "d", "e"]);
        assert!(metadata(path)?.is_file());

        Ok(())
    }

    #[cfg(not(windows))]
    #[test]
    fn test_update_replace_symlink_with_dir() -> Result<()> {
        let workingdir = TempDir::new()?;
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);
        let store = Arc::new(ContentStore::new(&localdir, &config)?);

        let k = key("a/b/c/d/e", "2");
        let delta = Delta {
            data: Bytes::from("CONTENT"),
            base: None,
            key: k.clone(),
        };
        store.add(&delta, &Default::default())?;

        let mut path = workingdir.as_ref().to_path_buf();
        path.extend(&["a", "b"]);
        create_dir_all(&path)?;
        path.push("c");

        std::os::unix::fs::symlink("foo", &path)?;

        let root = workingdir.as_ref().to_path_buf();
        let state = WriterState::new(root, store)?;
        update(&state, k, UpdateFlag::Regular)?;

        path.extend(&["d", "e"]);
        assert!(metadata(path)?.is_file());

        Ok(())
    }

    #[test]
    fn test_update_replace_dir_with_file() -> Result<()> {
        let workingdir = TempDir::new()?;
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);
        let store = Arc::new(ContentStore::new(&localdir, &config)?);

        let k = key("a/b/c/d/e", "2");
        let delta = Delta {
            data: Bytes::from("CONTENT"),
            base: None,
            key: k.clone(),
        };
        store.add(&delta, &Default::default())?;

        let mut path = workingdir.as_ref().to_path_buf();
        path.extend(&["a", "b", "c", "d", "e", "f"]);
        create_dir_all(&path)?;

        let root = workingdir.as_ref().to_path_buf();
        let state = WriterState::new(root, store)?;
        let written = update(&state, k, UpdateFlag::Regular)?;
        assert_eq!(written, 7);

        let mut path = workingdir.as_ref().to_path_buf();
        path.extend(&["a", "b", "c", "d", "e"]);
        assert!(metadata(path)?.is_file());

        Ok(())
    }

    #[test]
    fn test_remove_basic() -> Result<()> {
        let workingdir = TempDir::new()?;

        let root = workingdir.as_ref().to_path_buf();
        let mut path = root.clone();
        path.push("TEST");

        File::create(&path)?;

        let state = RemoverState::new(root)?;
        state.working_copy.remove(RepoPath::from_str("TEST")?)?;

        assert_eq!(read_dir(&workingdir)?.count(), 0);

        Ok(())
    }

    #[test]
    fn test_remove_nested() -> Result<()> {
        let workingdir = TempDir::new()?;

        let root = workingdir.as_ref().to_path_buf();
        let mut path = root.clone();
        path.extend(&["THESE", "ARE", "DIRECTORIES"]);

        create_dir_all(&path)?;
        path.push("FILE");
        File::create(&path)?;

        let state = RemoverState::new(root)?;
        state
            .working_copy
            .remove(RepoPath::from_str("THESE/ARE/DIRECTORIES/FILE")?)?;
        assert_eq!(read_dir(&workingdir)?.count(), 0);

        Ok(())
    }

    #[test]
    fn test_remove_nested_partial() -> Result<()> {
        let workingdir = TempDir::new()?;

        let root = workingdir.as_ref().to_path_buf();
        let mut path = root.clone();
        path.extend(&["THESE", "ARE", "DIRECTORIES"]);

        create_dir_all(&path)?;
        path.push("FILE");
        File::create(&path)?;

        let mut path = root.clone();
        path.extend(&["OTHER"]);

        create_dir_all(&path)?;
        path.push("FILE");
        File::create(&path)?;

        let state = RemoverState::new(root)?;
        state
            .working_copy
            .remove(RepoPath::from_str("THESE/ARE/DIRECTORIES/FILE")?)?;
        assert_eq!(read_dir(&workingdir)?.count(), 1);

        Ok(())
    }

    #[test]
    fn test_remove_while_open() -> Result<()> {
        let workingdir = TempDir::new()?;

        let root = workingdir.as_ref().to_path_buf();
        let path = root.join("TEST");

        let f = File::create(&path)?;

        let state = RemoverState::new(root)?;
        state.working_copy.remove(RepoPath::from_str("TEST")?)?;

        drop(f);

        assert_eq!(read_dir(&workingdir)?.count(), 0);

        Ok(())
    }

    #[test]
    fn test_remove_while_mapped() -> Result<()> {
        let workingdir = TempDir::new()?;

        let root = workingdir.as_ref().to_path_buf();
        let path = root.join("TEST");

        File::create(&path)?.write_all(b"CONTENT")?;
        let f = File::open(&path)?;
        let map = unsafe { MmapOptions::new().map(&f)? };

        let state = RemoverState::new(root)?;
        state.working_copy.remove(RepoPath::from_str("TEST")?)?;

        drop(map);

        if cfg!(windows) {
            assert_eq!(read_dir(&workingdir)?.count(), 1);

            // The file must have been removed
            state.working_copy.remove(RepoPath::from_str("TEST")?)?;
            assert_eq!(read_dir(&workingdir)?.count(), 1);
        } else {
            assert_eq!(read_dir(&workingdir)?.count(), 0);
        }

        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn test_remove_while_open_with_no_sharing() -> Result<()> {
        let workingdir = TempDir::new()?;

        let root = workingdir.as_ref().to_path_buf();
        let path = root.join("TEST");

        File::create(&path)?.write_all(b"CONTENT")?;

        // No sharing for this file. remove should still succeed.
        let f = OpenOptions::new().read(true).share_mode(0).open(&path)?;

        let state = RemoverState::new(root)?;
        // This will fail silently.
        state.working_copy.remove(RepoPath::from_str("TEST")?)?;
        drop(f);

        assert_eq!(read_dir(&workingdir)?.count(), 1);

        // The file is still there.
        let f = File::open(&path)?;
        drop(f);

        // Now there is no longer a file handle to it, remove it.
        state.working_copy.remove(RepoPath::from_str("TEST")?)?;

        assert_eq!(read_dir(&workingdir)?.count(), 0);

        Ok(())
    }

    fn validate_paths<'a>(paths: impl Iterator<Item = &'a RepoPath>) -> bool {
        let mut files = HashSet::new();
        let mut directories = HashSet::new();

        for path in paths {
            // Keys for an empty path are meaningless.
            if path.as_str().trim() == "" {
                return false;
            }

            // We cannot have a file also be a directory.
            if directories.contains(path) {
                return false;
            }

            // Files have to be unique
            if files.contains(path) {
                return false;
            }

            // Make sure we do no have paths with directories and files with the same name.
            for parent in path.parents().skip(1) {
                if files.contains(parent) {
                    return false;
                }

                directories.insert(parent);
            }

            files.insert(path);
        }

        true
    }

    quickcheck! {
        fn update_only(keys: Vec<Key>) -> Result<TestResult> {
            let workingdir = TempDir::new()?;
            let cachedir = TempDir::new()?;
            let localdir = TempDir::new()?;
            let config = make_config(&cachedir);
            let store = Arc::new(ContentStore::new(&localdir, &config)?);

            if !validate_paths(keys.iter().map(|k| k.path.as_ref())) {
                return Ok(TestResult::discard());
            }

            let mut expected_size = 0;
            for key in keys.iter() {
                let data = Bytes::from(format!("{}", key));
                expected_size += data.len();
                let delta = Delta {
                    data,
                    base: None,
                    key: key.clone(),
                };

                store.add(&delta, &Default::default())?;
            }

            let root = workingdir.as_ref().to_path_buf();
            let state = WriterState::new(root, store)?;

            let mut written_size = 0;
            for key in keys.iter() {
                written_size += update(&state, key.clone(), UpdateFlag::Regular)?;
            }

            for key in keys.iter() {
                let mut fullpath = workingdir.as_ref().to_path_buf();
                fullpath.push(key.path.as_str());

                let ondisk = read_to_string(&fullpath)?;
                let expected = format!("{}", key);
                ensure!(ondisk == expected, format!("Got: {}, expected: {}", ondisk, expected));
            }

            Ok(TestResult::from_bool(expected_size == written_size))
        }

        fn remove_only(paths: Vec<RepoPathBuf>) -> Result<TestResult> {
            let workingdir = TempDir::new()?;

            if !validate_paths(paths.iter().map(|path| path.as_ref())) {
                return Ok(TestResult::discard());
            }

            for path in paths.iter() {
                let mut fullpath = workingdir.as_ref().to_path_buf();
                fullpath.push(path.as_str());

                create_dir_all(fullpath.parent().unwrap())?;
                File::create(&fullpath)?;
            }

            let root = workingdir.as_ref().to_path_buf();
            let state = RemoverState::new(root)?;
            for path in paths.iter() {
                state.working_copy.remove(&path)?;
            }

            Ok(TestResult::from_bool(read_dir(&workingdir)?.count() == 0))
        }

        fn update_then_remove(keys: Vec<Key>) -> Result<TestResult> {
            let workingdir = TempDir::new()?;
            let cachedir = TempDir::new()?;
            let localdir = TempDir::new()?;
            let config = make_config(&cachedir);
            let store = Arc::new(ContentStore::new(&localdir, &config)?);

            if !validate_paths(keys.iter().map(|k| k.path.as_ref())) {
                return Ok(TestResult::discard());
            }

            for key in keys.iter() {
                let delta = Delta {
                    data: Bytes::from(format!("{}", key)),
                    base: None,
                    key: key.clone(),
                };

                store.add(&delta, &Default::default())?;
            }

            let root = workingdir.as_ref().to_path_buf();
            let state = WriterState::new(root, store)?;

            for key in keys.iter() {
                update(&state, key.clone(), UpdateFlag::Regular)?;
            }

            let root = workingdir.as_ref().to_path_buf();
            let state = RemoverState::new(root)?;
            for key in keys.iter() {
                state.working_copy.remove(&key.path)?;
            }

            Ok(TestResult::from_bool(read_dir(&workingdir)?.count() == 0))
        }
    }
}
