// Copyright Facebook, Inc. 2017
//! Python bindings for treedirstate.

use cpython::*;
use cpython::exc;
use dirstate::Dirstate;
use filestate::FileState;
use std::cell::RefCell;
use std::path::PathBuf;
use store::BlockId;

py_module_initializer!(
    rusttreedirstate,
    initrusttreedirstate,
    PyInit_rusttreedirstate,
    |py, m| {
        m.add_class::<RustDirstateMap>(py)?;
        Ok(())
    }
);

py_class!(class RustDirstateMap |py| {
    data repodir: PathBuf;
    data dirstate: RefCell<Dirstate<FileState>>;

    def __new__(
        _cls,
        _ui: &PyObject,
        opener: &PyObject
    ) -> PyResult<RustDirstateMap> {
        let repodir = opener.getattr(py, "base")?.extract::<String>(py)?;
        let dirstate = Dirstate::new();
        RustDirstateMap::create_instance(
            py,
            repodir.into(),
            RefCell::new(dirstate))
    }

    def clear(&self) -> PyResult<PyObject> {
        let mut dirstate = self.dirstate(py).borrow_mut();
        dirstate.clear();
        Ok(py.None())
    }

    // Read a dirstate file.
    def read(&self, name: &str, root_id: u64) -> PyResult<Option<PyObject>> {
        let mut dirstate = self.dirstate(py).borrow_mut();
        dirstate
            .open(self.repodir(py).join(name), BlockId(root_id))
            .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?;
        Ok(None)
    }

    // Import another map of dirstate tuples into a treedirstate file.
    def importmap(&self, old_map: PyObject) -> PyResult<Option<PyObject>> {
        let mut dirstate = self.dirstate(py).borrow_mut();
        let iter = PyIterator::from_object(
            py,
            old_map.call_method(py, "iteritems", NoArgs, None)?)?;
        for item in iter {
            let item_tuple = item?.extract::<PyTuple>(py)?;
            let filename = item_tuple.get_item(py, 0).extract::<PyBytes>(py)?;
            let data = item_tuple.get_item(py, 1).extract::<PySequence>(py)?;
            let state = *data.get_item(py, 0)?.extract::<PyBytes>(py)?.data(py).first().unwrap();
            let mode = data.get_item(py, 1)?.extract::<u32>(py)?;
            let size = data.get_item(py, 2)?.extract::<i32>(py)?;
            let mtime = data.get_item(py, 3)?.extract::<i32>(py)?;
            if state == b'r' {
                dirstate
                    .remove_file(filename.data(py), &FileState::new(b'r', 0, size, 0))
                    .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?;
            } else {
                dirstate
                    .add_file(filename.data(py),
                              &FileState::new(state, mode, size, mtime))
                    .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?;
            }

        }
        Ok(None)
    }

    def write(&self, name: &str) -> PyResult<Option<PyObject>> {
        let mut dirstate = self.dirstate(py).borrow_mut();
        dirstate
            .write_full(self.repodir(py).join(name))
            .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?;
        Ok(None)
    }

    def writedelta(&self) -> PyResult<Option<PyObject>> {
        let mut dirstate = self.dirstate(py).borrow_mut();
        dirstate.write_delta().map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?;
        Ok(None)
    }

    def filecount(&self) -> PyResult<usize> {
        let dirstate = self.dirstate(py).borrow();
        Ok((dirstate.tracked_count() + dirstate.removed_count()) as usize)
    }

    def hastrackedfile(&self, filename: PyBytes) -> PyResult<bool> {
        let mut dirstate = self.dirstate(py).borrow_mut();
        let value = dirstate
                .get_tracked(filename.data(py))
                .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?;

        Ok(value.is_some())
    }

    def hasremovedfile(&self, filename: PyBytes) -> PyResult<bool> {
        let mut dirstate = self.dirstate(py).borrow_mut();
        let value = dirstate
                .get_removed(filename.data(py))
                .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?;
        Ok(value.is_some())
    }

    def gettracked(&self, filename: PyObject, default: PyObject) -> PyResult<PyObject> {
        let mut dirstate = self.dirstate(py).borrow_mut();
        let res = if let Ok(filename) = filename.extract::<PyBytes>(py) {
            let value = dirstate
                    .get_tracked(filename.data(py))
                    .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?;
            match value {
                Some(ref file) =>
                    PyTuple::new(py, &[
                           PyBytes::new(py, &[file.state; 1]).to_py_object(py).into_object(),
                           file.mode.to_py_object(py).into_object(),
                           file.size.to_py_object(py).into_object(),
                           file.mtime.to_py_object(py).into_object()]).into_object(),
                None => default,
            }
        } else {
            default
        };
        Ok(res)
    }

    def getremoved(&self, filename: PyObject, default: PyObject) -> PyResult<PyObject> {
        let mut dirstate = self.dirstate(py).borrow_mut();
        let res = if let Ok(filename) = filename.extract::<PyBytes>(py) {
            let value = dirstate
                    .get_removed(filename.data(py))
                    .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?;
            match value {
                Some(ref file) =>
                    PyTuple::new(py, &[
                           PyBytes::new(py, &[file.state; 1]).to_py_object(py).into_object(),
                           file.mode.to_py_object(py).into_object(),
                           file.size.to_py_object(py).into_object(),
                           file.mtime.to_py_object(py).into_object()]).into_object(),
                None => default,
            }
        } else {
            default
        };
        Ok(res)
    }

    def hastrackeddir(&self, dirname: PyBytes) -> PyResult<bool> {
        let mut dirstate = self.dirstate(py).borrow_mut();
        dirstate
            .has_tracked_dir(dirname.data(py))
            .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))
    }

    def hasremoveddir(&self, dirname: PyBytes) -> PyResult<bool> {
        let mut dirstate = self.dirstate(py).borrow_mut();
        dirstate
            .has_removed_dir(dirname.data(py))
            .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))
    }

    // Get the next dirstate object after the provided filename.  If the filename is None,
    // returns the first file in the tree.  If the provided filename is the last file, returns
    // None.
    def getnext(&self, filename: Option<PyBytes>, removed: bool) -> PyResult<PyObject> {
        let mut dirstate = self.dirstate(py).borrow_mut();
        let next = if removed {
            match filename {
                Some(filename) => {
                    dirstate
                        .get_next_removed(filename.data(py))
                        .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?
                }
                None => {
                    dirstate
                        .get_first_removed()
                        .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?
                }
            }
        } else {
            match filename {
                Some(filename) => {
                    dirstate
                        .get_next_tracked(filename.data(py))
                        .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?
                }
                None => {
                    dirstate.get_first_tracked()
                        .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?
                }
            }
        };
        let res = match next {
            Some((ref f, ref s)) =>
                PyTuple::new(py, &[
                    PyBytes::new(py, &f).into_object(),
                    PyTuple::new(py, &[
                        PyBytes::new(py, &[s.state; 1]).to_py_object(py).into_object(),
                        s.mode.to_py_object(py).into_object(),
                        s.size.to_py_object(py).into_object(),
                        s.mtime.to_py_object(py).into_object()]).into_object()
                    ]).into_object(),
            None => py.None(),
        };
        Ok(res)
    }

    def addfile(
        &self,
        filename: PyBytes,
        _old_state: PyBytes,
        state: PyBytes,
        mode: u32,
        size: i32,
        mtime: i32
    ) -> PyResult<PyObject> {
        let mut dirstate = self.dirstate(py).borrow_mut();
        let state = *state.data(py).first().unwrap_or(&b'?');
        dirstate
            .add_file(filename.data(py), &FileState::new(state, mode, size, mtime))
            .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?;
        Ok(py.None())
    }

    def removefile(&self, filename: PyBytes, _old_state: PyBytes, size: i32) -> PyResult<PyObject> {
        let mut dirstate = self.dirstate(py).borrow_mut();
        dirstate
            .remove_file(filename.data(py), &FileState::new(b'r', 0, size, 0))
            .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?;
        Ok(py.None())
    }

    def dropfile(&self, filename: PyBytes) -> PyResult<bool> {
        let mut dirstate = self.dirstate(py).borrow_mut();
        dirstate
            .drop_file(filename.data(py))
            .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))
    }

    // Returns the ID of the root node.
    def rootid(&self) -> PyResult<Option<u64>> {
        Ok(self.dirstate(py).borrow().root_id().map(|id| id.0))
    }

});
