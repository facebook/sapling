/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::{
    cell::RefCell,
    collections::HashSet,
    fs::{create_dir_all, remove_dir, remove_dir_all, symlink_metadata, File},
    io::{self, ErrorKind, Write},
    mem,
    path::{Path, PathBuf},
    str,
    thread::{self, JoinHandle},
};
#[cfg(not(windows))]
use std::{
    fs::{set_permissions, Permissions},
    os::unix::fs::PermissionsExt,
};

use anyhow::{bail, ensure, format_err, Context, Result};
use bytes::Bytes;
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

/// Audit repositories path to make sure that it is safe to write/remove through them.
///
/// This uses caching internally to avoid the heavy cost of querying the OS for each directory in
/// the path of a file. For a multi-threaded writer/removed, the intention is to have one
/// `PathAuditor` per-thread, this will be more memory intensive than having a shared one, but it
/// avoids contention on the cache. A fine-grained concurrent `HashSet` could be used instead.
///
/// Due to the caching, the checks performed by the `PathAuditor` are inherently racy, and
/// concurrent writes to the working copy by the user may lead to unsafe operations.
#[derive(Clone)]
struct PathAuditor {
    root: PathBuf,
    audited: RefCell<HashSet<RepoPathBuf>>,
}

impl PathAuditor {
    pub fn new(root: impl AsRef<Path>) -> Self {
        let audited = RefCell::new(HashSet::new());
        let root = root.as_ref().to_owned();
        Self { root, audited }
    }

    /// Slow path, query the filesystem for unsupported path. Namely, writing through a symlink
    /// outside of the repo is not supported.
    /// XXX: more checks
    fn audit_fs(&self, path: &RepoPath) -> Result<()> {
        let full_path = self.root.join(path.as_str());

        // XXX: Maybe filter by specific errors?
        if let Ok(metadata) = symlink_metadata(&full_path) {
            ensure!(!metadata.file_type().is_symlink(), "{} is a symlink", path);
        }

        Ok(())
    }

    /// Make sure that it is safe to write/remove `path` from the repo.
    pub fn audit(&self, path: &RepoPath) -> Result<PathBuf> {
        for parent in path.parents() {
            if !self.audited.borrow().contains(parent) {
                self.audit_fs(parent)
                    .with_context(|| format!("Can't audit path \"{}\"", parent))?;
                self.audited.borrow_mut().insert(parent.to_owned());
            }
        }

        let mut filepath = self.root.to_owned();
        filepath.push(path.as_str());
        Ok(filepath)
    }
}

enum UpdateFlag {
    Symlink,
    Executable,
}

#[cfg(not(windows))]
fn supports_symlinks() -> bool {
    // XXX: placeholder
    true
}

/// On some OS/filesystems, symlinks aren't supported, we simply create a file where it's content
/// is the symlink destination for these.
fn plain_symlink_file(link_name: &Path, link_dest: &Path) -> Result<()> {
    let link_dest = match link_dest.to_str() {
        None => bail!("Not a valid UTF-8 path: {:?}", link_dest),
        Some(s) => s,
    };

    Ok(File::create(link_name)?.write_all(link_dest.as_bytes())?)
}

/// Add a symlink `link_name` pointing to `link_dest`. On platforms that do not support symlinks,
/// `link_name` will be a file containing the path to `link_dest`.
fn symlink(link_name: impl AsRef<Path>, link_dest: impl AsRef<Path>) -> Result<()> {
    let link_name = link_name.as_ref();
    let link_dest = link_dest.as_ref();

    #[cfg(windows)]
    let result = plain_symlink_file(link_name, link_dest);

    #[cfg(not(windows))]
    let result = if supports_symlinks() {
        Ok(std::os::unix::fs::symlink(link_dest, link_name)?)
    } else {
        plain_symlink_file(link_name, link_dest)
    };

    result.with_context(|| format!("Can't create symlink '{:?} -> {:?}'", link_name, link_dest))
}

/// The file `path` can't be written to, attempt to fixup the directories and files so the file can
/// be created.
///
/// This is a slow operation, and should not be called before attempting to create `path`.
fn clear_conflicts(filepath: &Path, root: &Path) -> Result<()> {
    let mut path = filepath;
    if let Ok(metadata) = symlink_metadata(path) {
        let file_type = metadata.file_type();
        if file_type.is_dir() {
            remove_dir_all(path).with_context(|| format!("Can't remove directory {:?}", path))?;
        }
    }

    loop {
        if path == root {
            break;
        }

        if let Ok(metadata) = symlink_metadata(path) {
            let file_type = metadata.file_type();
            if file_type.is_file() || file_type.is_symlink() {
                remove_file(path).with_context(|| format!("Can't remove file {:?}", path))?;
            }
        }

        // By virtue of the fact that we haven't reached the root, we are guaranteed to
        // have a parent directory.
        path = path.parent().unwrap();
    }

    let dir = filepath.parent().unwrap();
    create_dir_all(dir).with_context(|| format!("Can't create directory {:?}", dir))?;

    Ok(())
}

/// Write a plain file with `content` at `filepath`.
fn write_regular(filepath: &Path, content: Bytes, root: &Path) -> Result<usize> {
    // Fast path: let's try to open the file directly, we'll handle the failure only if this fails.
    let mut f = match File::create(&filepath) {
        Ok(f) => f,
        Err(e) => {
            // Slow path: let's go over the path and try to figure out what is conflicting to
            // fix it.
            clear_conflicts(filepath, root)?;
            File::create(&filepath).with_context(|| {
                format!(
                    "Can't create file {:?}, after handling error \"{}\"",
                    filepath, e
                )
            })?
        }
    };
    f.write_all(&content)?;
    Ok(content.len())
}

/// Write an executable file with `content` as `filepath`.
fn write_executable(filepath: &Path, content: Bytes, root: &Path) -> Result<usize> {
    let size = write_regular(filepath, content, root)?;

    #[cfg(windows)]
    return Ok(size);

    #[cfg(not(windows))]
    {
        let perms = Permissions::from_mode(0o755);
        set_permissions(filepath, perms)?;
        Ok(size)
    }
}

/// Write a symlink file at `filepath`. The destination is represented by `content`.
fn write_symlink(filepath: &Path, content: Bytes, root: &Path) -> Result<usize> {
    let link_dest = Path::new(str::from_utf8(content.as_ref())?);

    // Fast path: let's try to symlink the file directly
    if let Err(e) = symlink(filepath, link_dest) {
        // Slow path: that didn't work out, let's try to fix it up.
        clear_conflicts(filepath, root)?;
        symlink(filepath, link_dest)
            .with_context(|| format!("Can't create symlink after handling error \"{}\"", e))?;
    };
    Ok(filepath.as_os_str().len())
}

fn write_file(
    filepath: &Path,
    content: Bytes,
    root: &Path,
    flag: Option<UpdateFlag>,
) -> Result<usize> {
    match flag {
        None => write_regular(filepath, content, root),
        Some(UpdateFlag::Executable) => write_executable(filepath, content, root),
        Some(UpdateFlag::Symlink) => write_symlink(filepath, content, root),
    }
}

/// Fetch the content of the passed in `hgid` and write it to `path`.
fn update(
    state: &WriterState,
    path: &RepoPath,
    hgid: HgId,
    flag: Option<UpdateFlag>,
) -> Result<usize> {
    let key = Key::new(path.to_owned(), hgid);

    let filepath = state.auditor.audit(path)?;

    let content = state
        .store
        .get_file_content(&key)?
        .ok_or_else(|| format_err!("Can't find key: {}", key))?;

    write_file(&filepath, content, state.root.as_path(), flag)
}

fn threaded_writer(
    state: WriterState,
    chan: Receiver<Vec<(RepoPathBuf, HgId, Option<UpdateFlag>)>>,
) -> Result<usize> {
    let mut written = 0;
    while let Ok(vec) = chan.recv() {
        for (path, hgid, flag) in vec.into_iter() {
            written += update(&state, path.as_repo_path(), hgid, flag)
                .with_context(|| format!("Can't update {} at {}", path, hgid))?;
        }
    }

    Ok(written)
}

#[derive(Clone)]
struct WriterState {
    root: PathBuf,
    store: ContentStore,
    auditor: PathAuditor,
}

impl WriterState {
    pub fn new(root: PathBuf, store: ContentStore) -> Self {
        let auditor = PathAuditor::new(&root);
        Self {
            root,
            store,
            auditor,
        }
    }
}

py_class!(class writerworker |py| {
    data inner: RefCell<Option<Worker<usize, (RepoPathBuf, HgId, Option<UpdateFlag>)>>>;

    def __new__(_cls, contentstore: contentstore, root: &PyPath, numthreads: usize) -> PyResult<writerworker> {
        let store = contentstore.to_inner(py);
        let root = root.to_path_buf();
        let writer_state = WriterState::new(root, store);

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
            Some(UpdateFlag::Symlink)
        } else if flags == "x" {
            Some(UpdateFlag::Executable)
        } else {
            return Err(format_err!("Unknown flags: {}", flags)).map_pyerr(py);
        };

        self.inner(py).borrow_mut().as_mut().unwrap().push_work(py, (path, node, flags)).map_pyerr(py)?;
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

fn remove(state: &RemoverState, path: &RepoPath) -> Result<()> {
    let mut filepath = state.auditor.audit(&path)?;

    if let Ok(metadata) = symlink_metadata(&filepath) {
        let file_type = metadata.file_type();
        if file_type.is_file() || file_type.is_symlink() {
            let result =
                remove_file(&filepath).with_context(|| format!("Can't remove file {:?}", filepath));
            if cfg!(windows) {
                // Windows is... an interesting beast. Some applications may
                // open files in the working copy and completely disallowing
                // sharing of the file[0] with others. On example of such
                // application is the Windows Defender[1], so if for some reason
                // it is scanning the working copy, Mercurial will be unable to
                // remove that file, and there is nothing that we could do about it.
                //
                // We could think of various strategies to mitigate this. One
                // being that we simply retry a bit later, but there is still no
                // guarantee that it would work. For now, let's just ignore all failures
                // on Windows.
                //
                // [0]: https://docs.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilea?redirectedfrom=MSDN
                // [1]: https://en.wikipedia.org/wiki/Windows_Defender
                let _ = result;
            } else {
                if let Err(e) = result {
                    if let Some(io_error) = e.downcast_ref::<io::Error>() {
                        ensure!(io_error.kind() == ErrorKind::NotFound, e);
                    } else {
                        return Err(e);
                    };
                }
            }
        }
    }

    // Mercurial doesn't track empty directories, remove them
    // recursively.
    let root = state.root.as_path();
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

fn threaded_remover(state: RemoverState, chan: Receiver<Vec<RepoPathBuf>>) -> Result<()> {
    while let Ok(vec) = chan.recv() {
        for path in vec.into_iter() {
            remove(&state, &path)?;
        }
    }

    Ok(())
}

#[derive(Clone)]
struct RemoverState {
    root: PathBuf,
    auditor: PathAuditor,
}

impl RemoverState {
    pub fn new(root: PathBuf) -> Self {
        let auditor = PathAuditor::new(&root);
        Self { root, auditor }
    }
}

py_class!(class removerworker |py| {
    data inner: RefCell<Option<Worker<(), RepoPathBuf>>>;

    def __new__(_cls, root: &PyPath, numthreads: usize) -> PyResult<removerworker> {
        let root = root.to_path_buf();
        let remover_state = RemoverState::new(root);

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

    use std::fs::{metadata, read_dir, read_link, read_to_string};
    #[cfg(windows)]
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};

    use bytes::Bytes;
    use memmap::MmapOptions;
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
        let state = WriterState::new(root, store);
        let written = update(&state, &k.path, k.hgid.clone(), None)?;

        assert_eq!(written, 7);

        Ok(())
    }

    #[test]
    fn test_executable() -> Result<()> {
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
        let state = WriterState::new(root, store);
        update(
            &state,
            &k.path,
            k.hgid.clone(),
            Some(UpdateFlag::Executable),
        )?;

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
        let store = ContentStore::new(&localdir, &config)?;

        let k = key("a", "2");
        let delta = Delta {
            data: Bytes::from("b"),
            base: None,
            key: k.clone(),
        };
        store.add(&delta, &Default::default())?;

        let root = workingdir.as_ref().to_path_buf();
        let state = WriterState::new(root, store);
        update(&state, &k.path, k.hgid.clone(), Some(UpdateFlag::Symlink))?;

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
        let store = ContentStore::new(&localdir, &config)?;

        let k = key("a/b/c/d/e", "2");
        let delta = Delta {
            data: Bytes::from("CONTENT"),
            base: None,
            key: k.clone(),
        };
        store.add(&delta, &Default::default())?;

        let root = workingdir.as_ref().to_path_buf();
        let state = WriterState::new(root, store);
        let written = update(&state, &k.path, k.hgid.clone(), None)?;
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
        let state = WriterState::new(root, store);
        let written = update(&state, &k.path, k.hgid.clone(), None)?;
        assert_eq!(written, 7);

        let mut path = workingdir.as_ref().to_path_buf();
        path.extend(&["a", "b", "c", "d", "e"]);
        assert!(metadata(path)?.is_file());

        Ok(())
    }

    #[cfg(not(windows))]
    #[test]
    fn test_update_try_replace_symlink_with_dir() -> Result<()> {
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
        let state = WriterState::new(root, store);
        assert!(update(&state, &k.path, k.hgid.clone(), None).is_err());

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
        let state = WriterState::new(root, store);
        let written = update(&state, &k.path, k.hgid.clone(), None)?;
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

        let state = RemoverState::new(root);
        remove(&state, RepoPath::from_str("TEST")?)?;

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

        let state = RemoverState::new(root);
        remove(&state, RepoPath::from_str("THESE/ARE/DIRECTORIES/FILE")?)?;
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

        let state = RemoverState::new(root);
        remove(&state, RepoPath::from_str("THESE/ARE/DIRECTORIES/FILE")?)?;
        assert_eq!(read_dir(&workingdir)?.count(), 1);

        Ok(())
    }

    #[test]
    fn test_remove_while_open() -> Result<()> {
        let workingdir = TempDir::new()?;

        let root = workingdir.as_ref().to_path_buf();
        let path = root.join("TEST");

        let f = File::create(&path)?;

        let state = RemoverState::new(root);
        remove(&state, RepoPath::from_str("TEST")?)?;

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

        let state = RemoverState::new(root);
        remove(&state, RepoPath::from_str("TEST")?)?;

        drop(map);

        if cfg!(windows) {
            assert_eq!(read_dir(&workingdir)?.count(), 1);

            // The file must have been removed
            remove(&state, RepoPath::from_str("TEST")?)?;
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

        let state = RemoverState::new(root);
        // This will fail silently.
        remove(&state, RepoPath::from_str("TEST")?)?;
        drop(f);

        assert_eq!(read_dir(&workingdir)?.count(), 1);

        // The file is still there.
        let f = File::open(&path)?;
        drop(f);

        // Now there is no longer a file handle to it, remove it.
        remove(&state, RepoPath::from_str("TEST")?)?;

        assert_eq!(read_dir(&workingdir)?.count(), 0);

        Ok(())
    }

    #[test]
    fn test_audit_valid() -> Result<()> {
        let root = TempDir::new()?;

        let auditor = PathAuditor::new(&root);

        let repo_path = RepoPath::from_str("a/b")?;
        assert_eq!(
            auditor.audit(repo_path)?,
            root.as_ref().join(repo_path.as_str())
        );

        Ok(())
    }

    #[cfg(not(windows))]
    #[test]
    fn test_audit_invalid_symlink() -> Result<()> {
        let root = TempDir::new()?;
        let other = TempDir::new()?;

        let auditor = PathAuditor::new(&root);

        let link = root.as_ref().join("a");
        std::os::unix::fs::symlink(&other, &link)?;
        assert_eq!(read_link(&link)?.canonicalize()?, other.as_ref());

        let repo_path = RepoPath::from_str("a/b")?;
        assert!(auditor.audit(repo_path).is_err());

        Ok(())
    }

    #[cfg(not(windows))]
    #[test]
    fn test_audit_caching() -> Result<()> {
        let root = TempDir::new()?;
        let other = TempDir::new()?;

        let path = root.as_ref().join("a");
        create_dir_all(&path)?;

        let auditor = PathAuditor::new(&root);

        // Populate the auditor cache.
        let repo_path = RepoPath::from_str("a/b")?;
        auditor.audit(&repo_path)?;

        remove_dir_all(&path)?;

        let link = root.as_ref().join("a");
        std::os::unix::fs::symlink(&other, &link)?;
        assert_eq!(read_link(&link)?.canonicalize()?, other.as_ref());

        // Even though "a" is now a symlink to outside the repo, the audit will succeed due to the
        // one performed just above.
        let repo_path = RepoPath::from_str("a/b")?;
        auditor.audit(repo_path)?;

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
            let state = WriterState::new(root, store);

            let mut written_size = 0;
            for key in keys.iter() {
                written_size += update(&state, &key.path, key.hgid.clone(), None)?;
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
            let state = RemoverState::new(root);
            for path in paths.iter() {
                remove(&state, &path)?;
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
            let state = WriterState::new(root, store);

            for key in keys.iter() {
                update(&state, &key.path, key.hgid.clone(), None)?;
            }

            let root = workingdir.as_ref().to_path_buf();
            let state = RemoverState::new(root);
            for key in keys.iter() {
                remove(&state, &key.path)?;
            }

            Ok(TestResult::from_bool(read_dir(&workingdir)?.count() == 0))
        }
    }
}
