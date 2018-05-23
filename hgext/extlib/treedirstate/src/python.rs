// Copyright Facebook, Inc. 2017
//! Python bindings for treedirstate.

use cpython::*;
use std::cell::RefCell;
use std::path::PathBuf;
use treestate::errors::{self, ErrorKind};
use treestate::filestate::FileState;
use treestate::store::BlockId;
use treestate::tree::{Key, KeyRef};
use treestate::treedirstate::TreeDirstate;

py_module_initializer!(
    treedirstate,
    inittreedirstate,
    PyInit_treedirstate,
    |py, m| {
        m.add_class::<treedirstatemap>(py)?;
        Ok(())
    }
);

fn callback_error(py: Python, mut e: PyErr) -> ErrorKind {
    let s = e.instance(py)
        .extract::<String>(py)
        .unwrap_or_else(|_e| "unknown exception".to_string());
    ErrorKind::CallbackError(s)
}

py_class!(class treedirstatemap |py| {
    data repodir: PathBuf;
    data dirstate: RefCell<TreeDirstate>;

    def __new__(
        _cls,
        _ui: &PyObject,
        opener: &PyObject
    ) -> PyResult<treedirstatemap> {
        let repodir = opener.getattr(py, "base")?.extract::<String>(py)?;
        let dirstate = TreeDirstate::new();
        treedirstatemap::create_instance(
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

    def write(&self, name: &str, fsnow: i32, nonnorm: PyObject) -> PyResult<Option<PyObject>> {
        let mut dirstate = self.dirstate(py).borrow_mut();
        // Mark files with an mtime of `fsnow` as being out of date.  See
        // mercurial/pure/parsers.py:pack_dirstate in core Mercurial for why this is done.
        let mut filter = |filepath: &Vec<KeyRef>, state: &mut FileState| {
            if state.state == b'n' && state.mtime == fsnow {
                state.mtime = -1;
                let filename = PyBytes::new(py, &filepath.concat()).into_object();
                nonnorm.call(py, (filename,), None).map_err(|e| callback_error(py, e))?;
            }
            Ok(())
        };
        dirstate
            .visit_tracked(&mut filter)
            .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?;
        dirstate
            .write_full(self.repodir(py).join(name))
            .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?;
        Ok(None)
    }

    def writedelta(&self, fsnow: i32, nonnorm: PyObject) -> PyResult<Option<PyObject>> {
        let mut dirstate = self.dirstate(py).borrow_mut();
        // Mark files with an mtime of `fsnow` as being out of date.  See
        // mercurial/pure/parsers.py:pack_dirstate in core Mercurial for why this is done.
        let mut filter = |filepath: &Vec<KeyRef>, state: &mut FileState| {
            if state.state == b'n' && state.mtime == fsnow {
                state.mtime = -1;
                let filename = PyBytes::new(py, &filepath.concat()).into_object();
                nonnorm.call(py, (filename,), None).map_err(|e| callback_error(py, e))?;
            }
            Ok(())
        };
        dirstate
            .visit_changed_tracked(&mut filter)
            .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?;
        dirstate
            .write_delta()
            .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?;
        Ok(None)
    }

    def storeoffset(&self) -> PyResult<u64> {
        let dirstate = self.dirstate(py).borrow();
        let offset = dirstate.store_offset();
        Ok(offset.unwrap_or(0))
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

    def visittrackedfiles(&self, target: PyObject) -> PyResult<PyObject> {
        let mut dirstate = self.dirstate(py).borrow_mut();
        let mut visitor = |filepath: &Vec<KeyRef>, _state: &mut FileState| {
            let filename = PyBytes::new(py, &filepath.concat()).into_object();
            target.call(py, (filename,), None).map_err(|e| callback_error(py, e))?;
            Ok(())
        };
        dirstate
            .visit_tracked(&mut visitor)
            .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?;
        Ok(py.None())
    }

    def visitremovedfiles(&self, target: PyObject) -> PyResult<PyObject> {
        let mut dirstate = self.dirstate(py).borrow_mut();
        let mut visitor = |filepath: &Vec<KeyRef>, _state: &mut FileState| {
            let filename = PyBytes::new(py, &filepath.concat()).into_object();
            target.call(py, (filename,), None).map_err(|e| callback_error(py, e))?;
            Ok(())
        };
        dirstate
            .visit_removed(&mut visitor)
            .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?;
        Ok(py.None())
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

    def computenonnormals(&self, nonnormal: PyObject, otherparent: PyObject) -> PyResult<PyObject> {
        let mut dirstate = self.dirstate(py).borrow_mut();
        let mut tracked_visitor = |filepath: &Vec<KeyRef>, state: &mut FileState| {
            if state.state != b'n' || state.mtime == -1 {
                let filename = PyBytes::new(py, &filepath.concat()).into_object();
                nonnormal.call(py, (filename,), None).map_err(|e| callback_error(py, e))?;
            }
            if state.state == b'n' && state.mtime == -2 {
                let filename = PyBytes::new(py, &filepath.concat()).into_object();
                otherparent.call(py, (filename,), None).map_err(|e| callback_error(py, e))?;
            }
            Ok(())
        };
        dirstate
            .visit_tracked(&mut tracked_visitor)
            .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?;

        let mut removed_visitor = |filepath: &Vec<KeyRef>, _state: &mut FileState| {
            let filename = PyBytes::new(py, &filepath.concat()).into_object();
            nonnormal.call(py, (filename,), None).map_err(|e| callback_error(py, e))?;
            Ok(())
        };
        dirstate
            .visit_removed(&mut removed_visitor)
            .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?;

        Ok(py.None())
    }

    def getcasefoldedtracked(
        &self,
        filename: PyBytes,
        casefolder: PyObject,
        casefolderid: u64
    ) -> PyResult<Option<PyObject>> {
        let mut dirstate = self.dirstate(py).borrow_mut();
        let mut filter = |filename: KeyRef| -> errors::Result<Key> {
            let unfolded = PyBytes::new(py, filename);
            let folded = casefolder.call(py, (unfolded,), None)
                                   .map_err(|e| callback_error(py, e))?
                                   .extract::<PyBytes>(py)
                                   .map_err(|e| callback_error(py, e))?;
            Ok(folded.data(py).to_vec())
        };

        dirstate
            .get_tracked_filtered_key(filename.data(py), &mut filter, casefolderid)
            .map(|o| o.map(|k| PyBytes::new(py, &k).into_object()))
            .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))
    }

    def pathcomplete(
        &self,
        spec: PyBytes,
        acceptablestates: PyBytes,
        matchcallback: PyObject,
        fullpaths: bool
    ) -> PyResult<PyObject> {
        let mut dirstate = self.dirstate(py).borrow_mut();
        let acceptablestates = acceptablestates.data(py);

        let mut visitor = |filepath: &Vec<KeyRef>| {
            let filename = PyBytes::new(py, &filepath.concat()).into_object();
            matchcallback.call(py, (filename,), None).map_err(|e| callback_error(py, e))?;
            Ok(())
        };

        let acceptable = |state: &FileState| {
            acceptablestates.contains(&state.state)
        };

        // Files in state a, n or m are in the tracked tree.
        if b"anm".iter().any(|x| acceptablestates.contains(x)) {
            dirstate
                .path_complete_tracked(spec.data(py), fullpaths, &acceptable, &mut visitor)
                .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?;
        }

        // Files in state r are in the removed tree.
        if acceptablestates.contains(&b'r') {
            dirstate
                .path_complete_removed(spec.data(py), fullpaths, &acceptable, &mut visitor)
                .map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))?;
        }

        Ok(py.None())
    }

});
