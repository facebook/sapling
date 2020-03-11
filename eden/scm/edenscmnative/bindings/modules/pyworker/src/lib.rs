/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::{
    cell::RefCell,
    fs::{create_dir_all, remove_dir, remove_dir_all, symlink_metadata, File},
    io::{self, ErrorKind, Write},
    mem,
    path::{Path, PathBuf},
    thread::{self, JoinHandle},
};

use anyhow::{ensure, format_err, Context, Result};
use cpython::*;
use crossbeam::channel::{bounded, Receiver, Sender};

use cpython_ext::{PyNone, PyPath, ResultPyErrExt};
use pyrevisionstore::contentstore;
use revisionstore::ContentStore;
use types::{HgId, Key, RepoPath, RepoPathBuf};
use util::path::remove_file;

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
    threads: Vec<JoinHandle<Result<Ret>>>,
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
        work: impl Fn(State, Receiver<Vec<Work>>) -> Result<Ret> + Send + 'static + Copy,
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
    fn wait(mut self) -> Result<impl Iterator<Item = Result<Ret>>> {
        let pending = mem::take(&mut self.pending);
        self.sender.send(pending)?;
        drop(self.sender);

        let threads = mem::take(&mut self.threads);

        Ok(threads.into_iter().map(|thread| thread.join().unwrap()))
    }
}

/// Make sure that it is safe to write/remove `path` from the repo.
/// XXX: Doesn't do any validation for now.
fn audit_path(root: impl AsRef<Path>, path: &RepoPath) -> Result<PathBuf> {
    let mut filepath = root.as_ref().to_path_buf();
    filepath.push(path.as_str());
    Ok(filepath)
}

/// Fetch the content of the passed in `hgid` and write it to `path`.
fn update(state: &WriterState, path: &RepoPath, hgid: HgId) -> Result<usize> {
    let key = Key::new(path.to_owned(), hgid);

    let filepath = audit_path(&state.root, path)?;

    let content = state
        .store
        .get_file_content(&key)?
        .ok_or_else(|| format_err!("Can't find key: {}", key))?;
    let size = content.len();

    // Fast path: let's try to open the file directly, we'll handle the failure only if this fails.
    let mut f = match File::create(&filepath) {
        Ok(f) => f,
        Err(e) => {
            (|| -> Result<File> {
                // Slow path: let's go over the path and try to figure out what is conflicting to
                // fix it.
                let mut path = filepath.as_path();

                if let Ok(metadata) = symlink_metadata(path) {
                    let file_type = metadata.file_type();
                    if file_type.is_dir() {
                        remove_dir_all(&filepath)
                            .with_context(|| format!("Can't remove directory {:?}", filepath))?;
                    }
                }

                loop {
                    if path == state.root {
                        break;
                    }

                    if let Ok(metadata) = symlink_metadata(path) {
                        let file_type = metadata.file_type();
                        if file_type.is_file() || file_type.is_symlink() {
                            remove_file(path)
                                .with_context(|| format!("Can't remove file {:?}", path))?;
                        }
                    }

                    // By virtue of the fact that we haven't reached the root, we are guaranteed to
                    // have a parent directory.
                    path = path.parent().unwrap();
                }

                let dir = filepath.parent().unwrap();
                create_dir_all(dir).with_context(|| format!("Can't create directory {:?}", dir))?;

                Ok(File::create(&filepath)?)
            })()
            .with_context(|| {
                format!(
                    "Can't create file {:?}, after handling error \"{}\"",
                    filepath, e
                )
            })?
        }
    };
    f.write_all(&content)?;
    Ok(size)
}

fn threaded_writer(state: WriterState, chan: Receiver<Vec<(RepoPathBuf, HgId)>>) -> Result<usize> {
    let mut written = 0;
    while let Ok(vec) = chan.recv() {
        for (path, hgid) in vec.into_iter() {
            written += update(&state, path.as_repo_path(), hgid)
                .with_context(|| format!("Can't update {} at {}", path, hgid))?;
        }
    }

    Ok(written)
}

#[derive(Clone)]
struct WriterState {
    root: PathBuf,
    store: ContentStore,
}

py_class!(class writerworker |py| {
    data inner: RefCell<Option<Worker<usize, (RepoPathBuf, HgId)>>>;

    def __new__(_cls, contentstore: contentstore, root: &PyPath, numthreads: usize) -> PyResult<writerworker> {
        let store = contentstore.to_inner(py);
        let root = root.to_path_buf();

        let inner = Worker::new(numthreads, WriterState { store, root }, |state, receiver| threaded_writer(state, receiver));

        writerworker::create_instance(py, RefCell::new(Some(inner)))
    }

    /// Issue an asynchronous write call. The request will be picked up by a
    /// background thread which will write the content corresponding to `node` to
    /// the file `name`. May block and release the GIL when too much work is
    /// pending.
    def write(&self, name: &PyPath, node: &PyBytes) -> PyResult<PyNone> {
        let path = name.to_repo_path_buf().map_pyerr(py)?;
        let node = HgId::from_slice(node.data(py)).map_pyerr(py)?;

        self.inner(py).borrow_mut().as_mut().unwrap().push_work(py, (path, node)).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// Wait for all the pending `write` calls to complete.
    def wait(&self) -> PyResult<usize> {
        let inner = self.inner(py).borrow_mut().take().unwrap();

        let written = py.allow_threads(move || -> Result<usize> {
            Ok(inner.wait()?.collect::<Result<Vec<usize>>>()?.into_iter().sum())
        }).map_pyerr(py)?;

        Ok(written)
    }
});

fn remove(root: impl AsRef<Path>, path: &RepoPath) -> Result<()> {
    let root = root.as_ref();
    let mut filepath = audit_path(&root, &path)?;

    if let Ok(metadata) = symlink_metadata(&filepath) {
        let file_type = metadata.file_type();
        if file_type.is_file() || file_type.is_symlink() {
            if let Err(e) =
                remove_file(&filepath).with_context(|| format!("Can't remove file {:?}", filepath))
            {
                if let Some(io_error) = e.downcast_ref::<io::Error>() {
                    ensure!(io_error.kind() == ErrorKind::NotFound, e);
                } else {
                    return Err(e);
                };
            }
        }
    }

    // Mercurial doesn't track empty directories, remove them
    // recursively.
    loop {
        if !filepath.pop() || filepath == root {
            break;
        }

        if remove_dir(&filepath).is_err() {
            break;
        }
    }
    Ok(())
}

fn threaded_remover(root: PathBuf, chan: Receiver<Vec<RepoPathBuf>>) -> Result<()> {
    while let Ok(vec) = chan.recv() {
        for path in vec.into_iter() {
            remove(&root, &path)?;
        }
    }

    Ok(())
}

py_class!(class removerworker |py| {
    data inner: RefCell<Option<Worker<(), RepoPathBuf>>>;

    def __new__(_cls, root: &PyPath, numthreads: usize) -> PyResult<removerworker> {
        let root = root.to_path_buf();
        let inner = Worker::new(numthreads, root, |root, chan| threaded_remover(root, chan));

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
    def wait(&self) -> PyResult<PyNone> {
        let inner = self.inner(py).borrow_mut().take().unwrap();

        py.allow_threads(move || -> Result<()> {
            inner.wait()?.collect::<Result<()>>()
        }).map_pyerr(py)?;

        Ok(PyNone)
    }
});

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs::{metadata, read_dir, read_to_string};

    use bytes::Bytes;
    use quickcheck::{quickcheck, TestResult};
    use tempfile::TempDir;

    use revisionstore::{
        datastore::{Delta, MutableDeltaStore},
        testutil::make_config,
    };
    use types::testutil::key;

    #[test]
    fn test_update_basic() -> Result<()> {
        let workingdir = TempDir::new()?;
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);
        let store = ContentStore::new(&localdir, &config)?;

        let k = key("a", "2");
        let delta = Delta {
            data: Bytes::from("CONTENT"),
            base: None,
            key: k.clone(),
        };
        store.add(&delta, &Default::default())?;

        let root = workingdir.as_ref().to_path_buf();
        let state = WriterState { root, store };
        let written = update(&state, &k.path, k.hgid.clone())?;

        assert_eq!(written, 7);

        Ok(())
    }

    #[test]
    fn test_update_nested() -> Result<()> {
        let workingdir = TempDir::new()?;
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);
        let store = ContentStore::new(&localdir, &config)?;

        let k = key("a/b/c/d/e", "2");
        let delta = Delta {
            data: Bytes::from("CONTENT"),
            base: None,
            key: k.clone(),
        };
        store.add(&delta, &Default::default())?;

        let root = workingdir.as_ref().to_path_buf();
        let state = WriterState { root, store };
        let written = update(&state, &k.path, k.hgid.clone())?;
        assert_eq!(written, 7);

        Ok(())
    }

    #[test]
    fn test_update_replace_file_with_dir() -> Result<()> {
        let workingdir = TempDir::new()?;
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);
        let store = ContentStore::new(&localdir, &config)?;

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
        let state = WriterState { root, store };
        let written = update(&state, &k.path, k.hgid.clone())?;
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
        let store = ContentStore::new(&localdir, &config)?;

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
        let state = WriterState { root, store };
        let written = update(&state, &k.path, k.hgid.clone())?;
        assert_eq!(written, 7);

        let mut path = workingdir.as_ref().to_path_buf();
        path.extend(&["a", "b", "c", "d", "e"]);
        assert!(metadata(path)?.is_file());

        Ok(())
    }

    #[test]
    fn test_update_replace_dir_with_file() -> Result<()> {
        let workingdir = TempDir::new()?;
        let cachedir = TempDir::new()?;
        let localdir = TempDir::new()?;
        let config = make_config(&cachedir);
        let store = ContentStore::new(&localdir, &config)?;

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
        let state = WriterState { root, store };
        let written = update(&state, &k.path, k.hgid.clone())?;
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

        remove(&root, RepoPath::from_str("TEST")?)?;

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

        remove(&root, RepoPath::from_str("THESE/ARE/DIRECTORIES/FILE")?)?;
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

        remove(&root, RepoPath::from_str("THESE/ARE/DIRECTORIES/FILE")?)?;
        assert_eq!(read_dir(&workingdir)?.count(), 1);

        Ok(())
    }

    quickcheck! {
        fn update_only(keys: Vec<Key>) -> Result<TestResult> {
            let workingdir = TempDir::new()?;
            let cachedir = TempDir::new()?;
            let localdir = TempDir::new()?;
            let config = make_config(&cachedir);
            let store = ContentStore::new(&localdir, &config)?;

            // Keys for an empty path are meaningless
            for key in keys.iter() {
                if key.path.as_str().trim() == "" {
                    return Ok(TestResult::discard());
                }
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
            let state = WriterState { root, store };

            let mut written_size = 0;
            for key in keys.iter() {
                written_size += update(&state, &key.path, key.hgid.clone())?;
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

            for path in paths.iter() {
                if path.as_str().trim() == "" {
                    return Ok(TestResult::discard());
                }
            }

            for path in paths.iter() {
                let mut fullpath = workingdir.as_ref().to_path_buf();
                fullpath.push(path.as_str());

                create_dir_all(fullpath.parent().unwrap())?;
                // Discard failures, this can happen if quickhcheck passes the following:
                // `vec!['a/b', 'a']`, the second file is already a directory, hence File::create
                // failing. This is harmless so let's ignore.
                if let Err(_) = File::create(&fullpath) {
                    return Ok(TestResult::discard());
                }
            }

            let root = workingdir.as_ref().to_path_buf();
            for path in paths.iter() {
                remove(&root, &path)?;
            }

            Ok(TestResult::from_bool(read_dir(&workingdir)?.count() == 0))
        }

        fn update_then_remove(keys: Vec<Key>) -> Result<TestResult> {
            let workingdir = TempDir::new()?;
            let cachedir = TempDir::new()?;
            let localdir = TempDir::new()?;
            let config = make_config(&cachedir);
            let store = ContentStore::new(&localdir, &config)?;

            // Keys for an empty path are meaningless
            for key in keys.iter() {
                if key.path.as_str().trim() == "" {
                    return Ok(TestResult::discard());
                }
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
            let state = WriterState { root, store };

            for key in keys.iter() {
                update(&state, &key.path, key.hgid.clone())?;
            }

            let root = workingdir.as_ref().to_path_buf();
            for key in keys.iter() {
                remove(&root, &key.path)?;
            }

            Ok(TestResult::from_bool(read_dir(&workingdir)?.count() == 0))
        }
    }
}
