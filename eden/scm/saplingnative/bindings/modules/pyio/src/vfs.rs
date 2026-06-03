/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::vfs::AtomicReplaceFile;
use ::vfs::OpenFlags;
use ::vfs::RemoveOptions;
use ::vfs::UpdateFlag;
use ::vfs::VFS as RustVfs;
use cpython::*;
use cpython_ext::PyNone;
use cpython_ext::PyPath;
use cpython_ext::PyPathBuf;
use cpython_ext::ResultPyErrExt;

use crate::metadata::metadata;
use crate::wrap_file_like;

py_class!(pub class vfs |py| {
    data inner: RustVfs;

    def __new__(_cls, root: PyPathBuf, destructive: bool = false) -> PyResult<Self> {
        let root = root.to_path_buf();
        let inner = if destructive {
            RustVfs::new_destructive(root)
        } else {
            RustVfs::new(root)
        }
        .map_pyerr(py)?;
        Self::create_instance(py, inner)
    }

    /// root() -> str
    /// Return the filesystem root path.
    def root(&self) -> PyResult<PyPathBuf> {
        self.inner(py).root().try_into().map_pyerr(py)
    }

    /// join(path: str) -> str
    /// Return the absolute filesystem path for a repo-relative path.
    def join(&self, path: &PyPath) -> PyResult<PyPathBuf> {
        let path = path.to_repo_path().map_pyerr(py)?;
        self.inner(py).join(path).try_into().map_pyerr(py)
    }

    /// open_vfs(path: str) -> vfs
    /// Open an existing directory below this root as a new VFS root.
    def open_vfs(&self, path: &PyPath) -> PyResult<Self> {
        let inner = self.inner(py).clone();
        let path = path.to_repo_path_buf().map_pyerr(py)?;
        let inner = py.allow_threads(move || inner.open_vfs(path.as_repo_path())).map_pyerr(py)?;
        Self::create_instance(py, inner)
    }

    /// mkdir(path: str, mode: Optional[int] = None) -> None
    /// Create a directory without following symlinks.
    def mkdir(&self, path: &PyPath, mode: Option<u32> = None) -> PyResult<PyNone> {
        let inner = self.inner(py).clone();
        let path = path.to_repo_path_buf().map_pyerr(py)?;
        py.allow_threads(move || inner.create_dir(path.as_repo_path(), mode)).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// makedirs(path: str, mode: Optional[int] = None) -> None
    /// Create a directory and missing parents without following symlinks.
    def makedirs(&self, path: &PyPath, mode: Option<u32> = None) -> PyResult<PyNone> {
        let inner = self.inner(py).clone();
        let path = path.to_repo_path_buf().map_pyerr(py)?;
        py.allow_threads(move || inner.create_dir_all(path.as_repo_path(), mode)).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// listdir(path: Optional[str] = None) -> List[str]
    /// List directory entries without following symlinks.
    def listdir(&self, path: Option<&PyPath> = None) -> PyResult<Vec<PyPathBuf>> {
        let inner = self.inner(py).clone();
        let path = match path {
            Some(path) => path.to_repo_path_buf().map_pyerr(py)?,
            None => types::RepoPathBuf::new(),
        };
        let names = py
            .allow_threads(move || {
                inner
                    .list_dir(path.as_repo_path())?
                    .into_iter()
                    .collect::<anyhow::Result<Vec<_>>>()
            })
            .map_pyerr(py)?;
        Ok(names.into_iter().map(PyPathBuf::from).collect())
    }

    /// case_sensitive() -> bool
    /// Return whether the root filesystem is case-sensitive.
    def case_sensitive(&self) -> PyResult<bool> {
        Ok(self.inner(py).case_sensitive())
    }

    /// supports_executables() -> bool
    /// Return whether the root filesystem supports executable bits.
    def supports_executables(&self) -> PyResult<bool> {
        Ok(self.inner(py).supports_executables())
    }

    /// supports_symlinks() -> bool
    /// Return whether this VFS is currently allowed to write symlinks.
    def supports_symlinks(&self) -> PyResult<bool> {
        Ok(self.inner(py).supports_symlinks())
    }

    /// set_supports_symlinks(value: bool) -> None
    /// Override whether this VFS is currently allowed to write symlinks.
    def set_supports_symlinks(&self, value: bool) -> PyResult<PyNone> {
        self.inner(py).set_supports_symlinks(value);
        Ok(PyNone)
    }

    /// metadata(path: str) -> metadata
    /// Return no-follow lstat-style metadata for a repo-relative path.
    def metadata(&self, path: &PyPath) -> PyResult<metadata> {
        let inner = self.inner(py).clone();
        let path = path.to_repo_path_buf().map_pyerr(py)?;
        let meta = py.allow_threads(move || inner.metadata(path.as_repo_path())).map_pyerr(py)?;
        metadata::create_instance(py, meta)
    }

    /// exists(path: str) -> bool
    /// Return whether a repo-relative path exists without following ancestor symlinks.
    def exists(&self, path: &PyPath) -> PyResult<bool> {
        let inner = self.inner(py).clone();
        let path = path.to_repo_path_buf().map_pyerr(py)?;
        py.allow_threads(move || inner.exists(path.as_repo_path())).map_pyerr(py)
    }

    /// is_file(path: str) -> bool
    /// Return whether a repo-relative path is a regular file.
    def is_file(&self, path: &PyPath) -> PyResult<bool> {
        let inner = self.inner(py).clone();
        let path = path.to_repo_path_buf().map_pyerr(py)?;
        py.allow_threads(move || inner.is_file(path.as_repo_path())).map_pyerr(py)
    }

    /// open(path: str, mode: str = "r", perm: int = 0o666, atomicreplace: bool = False) -> file
    /// Open a regular file without following symlinks. When atomicreplace is
    /// true, mode is ignored and perm is used as the replacement file mode.
    def open(
        &self,
        path: &PyPath,
        mode: &str = "r",
        perm: u32 = 0o666,
        atomicreplace: bool = false,
    ) -> PyResult<PyObject> {
        let inner = self.inner(py).clone();
        let path = path.to_repo_path_buf().map_pyerr(py)?;
        if atomicreplace {
            let file = py
                .allow_threads(move || inner.open_with_atomic_replace(path.as_repo_path(), perm))
                .map_pyerr(py)?;
            return Ok(wrap_file_like(
                py,
                file,
                atomic_replace_file_mut,
                |file| file.persist(),
            )?
            .into_object());
        }

        let flags = mode.parse::<OpenFlags>().map_pyerr(py)?;
        let file = py.allow_threads(move || inner.open(path.as_repo_path(), flags, perm)).map_pyerr(py)?;
        Ok(wrap_file_like(py, file, |file| file, |_| Ok(()))?.into_object())
    }

    /// read(path: str) -> bytes
    /// Read a file or symlink without following ancestor symlinks.
    def read(&self, path: &PyPath) -> PyResult<PyBytes> {
        let inner = self.inner(py).clone();
        let path = path.to_repo_path_buf().map_pyerr(py)?;
        let data = py
            .allow_threads(move || inner.read(path.as_repo_path()).map(|data| data.into_vec()))
            .map_pyerr(py)?;
        Ok(PyBytes::new(py, &data))
    }

    /// read_with_metadata(path: str) -> Tuple[bytes, metadata]
    /// Read content and metadata without following ancestor symlinks.
    def read_with_metadata(&self, path: &PyPath) -> PyResult<(PyBytes, metadata)> {
        let inner = self.inner(py).clone();
        let path = path.to_repo_path_buf().map_pyerr(py)?;
        let (data, meta) = py
            .allow_threads(move || {
                inner
                    .read_with_metadata(path.as_repo_path())
                    .map(|(data, meta)| (data.into_vec(), meta))
            })
            .map_pyerr(py)?;
        Ok((PyBytes::new(py, &data), metadata::create_instance(py, meta)?))
    }

    /// write(path: str, data: bytes, flags: str = "") -> int
    /// Write data using flags "", "l", or "x"; returns bytes written.
    def write(&self, path: &PyPath, data: PyBytes, flags: &str = "") -> PyResult<usize> {
        let inner = self.inner(py).clone();
        let path = path.to_repo_path_buf().map_pyerr(py)?;
        let data = data.data(py).to_vec();
        let flag = parse_update_flag(flags).map_pyerr(py)?;
        py.allow_threads(move || inner.write(path.as_repo_path(), blob::Blob::from(data), flag)).map_pyerr(py)
    }

    /// unlink(path: str) -> None
    /// Remove a file or symlink without pruning empty parent directories.
    def unlink(&self, path: &PyPath) -> PyResult<PyNone> {
        let inner = self.inner(py).clone();
        let path = path.to_repo_path_buf().map_pyerr(py)?;
        py.allow_threads(move || inner.remove(path.as_repo_path(), RemoveOptions::empty()))
            .map_pyerr(py)?;
        Ok(PyNone)
    }

    /// tryunlink(path: str) -> None
    /// Attempt to remove a file or symlink, ignoring missing files.
    def tryunlink(&self, path: &PyPath) -> PyResult<PyNone> {
        let inner = self.inner(py).clone();
        let path = path.to_repo_path_buf().map_pyerr(py)?;
        py.allow_threads(move || {
            inner.remove(path.as_repo_path(), RemoveOptions::IGNORE_MISSING_PATH)
        })
        .map_pyerr(py)?;
        Ok(PyNone)
    }

    /// unlinkpath(path: str, ignoremissing: bool = False) -> None
    /// Remove a file or symlink and prune empty parent directories.
    def unlinkpath(&self, path: &PyPath, ignoremissing: bool = false) -> PyResult<PyNone> {
        let inner = self.inner(py).clone();
        let path = path.to_repo_path_buf().map_pyerr(py)?;
        let mut options =
            RemoveOptions::IGNORE_NON_FILE_OR_SYMLINK | RemoveOptions::PRUNE_EMPTY_PARENTS;
        if ignoremissing {
            options |= RemoveOptions::IGNORE_MISSING_PATH;
        }
        py.allow_threads(move || inner.remove(path.as_repo_path(), options)).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// rmtree(path: str) -> None
    /// Remove a directory tree recursively without following ancestor symlinks.
    def rmtree(&self, path: &PyPath) -> PyResult<PyNone> {
        let inner = self.inner(py).clone();
        let path = path.to_repo_path_buf().map_pyerr(py)?;
        py.allow_threads(move || inner.remove_dir_all(path.as_repo_path())).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// rmdir(path: str) -> None
    /// Remove an empty directory without following ancestor symlinks.
    def rmdir(&self, path: &PyPath) -> PyResult<PyNone> {
        let inner = self.inner(py).clone();
        let path = path.to_repo_path_buf().map_pyerr(py)?;
        py.allow_threads(move || inner.remove_dir(path.as_repo_path())).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// set_executable(path: str, executable: bool) -> None
    /// Set or clear executable bits on a regular file.
    def set_executable(&self, path: &PyPath, executable: bool) -> PyResult<PyNone> {
        let inner = self.inner(py).clone();
        let path = path.to_repo_path_buf().map_pyerr(py)?;
        py.allow_threads(move || inner.set_executable(path.as_repo_path(), executable)).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// set_permissions(path: str, mode: int) -> None
    /// Set file permissions without following ancestor symlinks.
    def set_permissions(&self, path: &PyPath, mode: u32) -> PyResult<PyNone> {
        let inner = self.inner(py).clone();
        let path = path.to_repo_path_buf().map_pyerr(py)?;
        py.allow_threads(move || inner.set_permissions(path.as_repo_path(), mode)).map_pyerr(py)?;
        Ok(PyNone)
    }

    /// reconcile_symlinks(paths: List[str]) -> None
    /// On Windows, rewrite file symlinks that should be directory symlinks.
    def reconcile_symlinks(&self, paths: Vec<String>) -> PyResult<PyNone> {
        let inner = self.inner(py).clone();
        let paths = paths
            .into_iter()
            .map(types::RepoPathBuf::from_string)
            .collect::<Result<Vec<_>, _>>()
            .map_pyerr(py)?;
        py.allow_threads(move || reconcile_symlinks(inner, paths)).map_pyerr(py)?;
        Ok(PyNone)
    }
});

#[cfg(windows)]
fn reconcile_symlinks(inner: RustVfs, paths: Vec<types::RepoPathBuf>) -> anyhow::Result<()> {
    let paths = paths
        .iter()
        .map(|path| path.as_repo_path())
        .collect::<Vec<_>>();
    inner.reconcile_symlinks(&paths)
}

#[cfg(not(windows))]
fn reconcile_symlinks(_inner: RustVfs, _paths: Vec<types::RepoPathBuf>) -> anyhow::Result<()> {
    Ok(())
}

fn parse_update_flag(flags: &str) -> anyhow::Result<UpdateFlag> {
    match flags {
        "" => Ok(UpdateFlag::Regular),
        "l" => Ok(UpdateFlag::Symlink),
        "x" => Ok(UpdateFlag::Executable),
        _ => Err(anyhow::format_err!("unknown vfs update flags: {flags:?}")),
    }
}

fn atomic_replace_file_mut(file: &mut AtomicReplaceFile) -> &mut std::fs::File {
    file
}
