// Copyright Facebook, Inc. 2017
//! Python bindings for treedirstate and treestate.

use cpython::*;
use pathencoding::local_bytes_to_path;
use std::cell::RefCell;
use std::path::PathBuf;
use treestate::errors::{self, ErrorKind};
use treestate::filestate::{FileState, FileStateV2, StateFlags};
use treestate::store::BlockId;
use treestate::tree::{Key, KeyRef};
use treestate::treedirstate::TreeDirstate;
use treestate::treestate::TreeState;

py_module_initializer!(treestate, inittreestate, PyInit_treestate, |py, m| {
    m.add_class::<treedirstatemap>(py)?;
    m.add_class::<treestate>(py)?;
    m.add(py, "EXIST_P1", StateFlags::EXIST_P1.to_bits())?;
    m.add(py, "EXIST_P2", StateFlags::EXIST_P2.to_bits())?;
    m.add(py, "EXIST_NEXT", StateFlags::EXIST_NEXT.to_bits())?;
    m.add(py, "IGNORED", StateFlags::IGNORED.to_bits())?;
    m.add(py, "NEED_CHECK", StateFlags::NEED_CHECK.to_bits())?;
    m.add(py, "COPIED", StateFlags::COPIED.to_bits())?;
    m.add(py, "tohgstate", py_fn!(py, flags_to_hg_state(flags: u16)))?;
    Ok(())
});

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
            Ok(folded.data(py).to_vec().into_boxed_slice())
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

py_class!(class treestate |py| {
    data state: RefCell<TreeState>;

    def __new__(
        _cls,
        path: &PyBytes,
        root_id: u64
    ) -> PyResult<treestate> {
        let path = local_bytes_to_path(path.data(py)).map_err(|_|encoding_error(py))?;
        let root_id = if root_id == 0 {
            None
        } else {
            Some(BlockId(root_id))
        };
        let state = convert_result(py, TreeState::open(path, root_id))?;
        treestate::create_instance(py, RefCell::new(state))
    }

    def flush(&self) -> PyResult<u64> {
        // Save changes to the existing file.
        let mut state = self.state(py).borrow_mut();
        let root_id = convert_result(py, state.flush())?;
        Ok(root_id.0)
    }

    def saveas(&self, path: PyBytes) -> PyResult<u64> {
        // Save as a new file. Return `BlockId` that can be used in constructor.
        let path = local_bytes_to_path(path.data(py)).map_err(|_|encoding_error(py))?;
        let mut state = self.state(py).borrow_mut();
        let root_id = convert_result(py, state.write_as(path))?;
        Ok(root_id.0)
    }

    def __len__(&self) -> PyResult<usize> {
        let state = self.state(py).borrow();
        Ok(state.len())
    }

    def __contains__(&self, path: PyBytes) -> PyResult<bool> {
        let mut state = self.state(py).borrow_mut();
        let path = path.data(py);
        let file = convert_result(py, state.get(path))?;
        // A lot of places require "__contains__(path)" to be "False" if "path" is "?" state
        let visible_flags = StateFlags::EXIST_P1 | StateFlags::EXIST_P2 | StateFlags::EXIST_NEXT;
        Ok(match file {
            Some(file) => file.state.intersects(visible_flags),
            None => false,
        })
    }

    def get(&self, path: PyBytes, default: PyObject) -> PyResult<PyObject> {
        let mut state = self.state(py).borrow_mut();
        let path = path.data(py);
        let file = convert_result(py, state.get(path))?;
        Ok(match file {
            // (flags, mode, size, mtime, copied)
            Some(ref file) => {
                PyTuple::new(py, &[
                    file.state.to_bits().to_py_object(py).into_object(),
                    file.mode.to_py_object(py).into_object(),
                    file.size.to_py_object(py).into_object(),
                    file.mtime.to_py_object(py).into_object(),
                    match file.copied {
                        Some(ref path) => PyBytes::new(py, &path).into_object(),
                        None => py.None(),
                    }
                ]).into_object()
            }
            None => default,
        })
    }

    def insert(
        &self, path: PyBytes, bits: u16, mode: u32, size: i32, mtime: i32, copied: PyObject
    ) -> PyResult<PyObject> {
        let mut flags = StateFlags::from_bits_truncate(bits);
        // For special mtime or size, mark them as "NEED_CHECK" automatically.
        if mtime < 0 || size < 0 {
            flags |= StateFlags::NEED_CHECK;
        }

        // Also fix-up COPIED bit so they stay consistent.
        let copied = if copied.is_true(py)? {
            let path = copied.extract::<PyBytes>(py)?;
            flags |= StateFlags::COPIED;
            Some(path.data(py).to_vec().into_boxed_slice())
        } else {
            flags -= StateFlags::COPIED;
            None
        };

        let file = FileStateV2 { mode, size, mtime, copied, state: flags };
        let path = path.data(py);
        let mut state = self.state(py).borrow_mut();
        convert_result(py, state.insert(path, &file))?;
        Ok(py.None())
    }

    def remove(&self, path: PyBytes) -> PyResult<bool> {
        let mut state = self.state(py).borrow_mut();
        convert_result(py, state.remove(path.data(py)))
    }

    def hasdir(&self, path: PyBytes) -> PyResult<bool> {
        let mut state = self.state(py).borrow_mut();
        let path = path.data(py);
        Ok(convert_result(py, state.has_dir(path))?)
    }

    def walk(&self, setbits: u16, unsetbits: u16) -> PyResult<Vec<PyBytes>> {
        // Get all file paths with `setbits` set and `unsetbits` unset.
        assert_eq!(setbits & unsetbits, 0, "setbits cannot overlap with unsetbits");
        let setbits = StateFlags::from_bits_truncate(setbits);
        let unsetbits = StateFlags::from_bits_truncate(unsetbits);
        let mask = setbits | unsetbits;
        let mut state = self.state(py).borrow_mut();
        let mut result = Vec::new();
        convert_result(py, state.visit(
            &mut |components, _state| {
                let path = PyBytes::new(py, &components.concat());
                result.push(path);
                Ok(())
            },
            &|_, dir| match dir.get_aggregated_state() {
                None => true,
                Some(state) =>
                    state.union.contains(setbits) && !state.intersection.intersects(unsetbits),
            },
            &|_, file| file.state & mask == setbits,
        ))?;
        Ok(result)
    }

    def getfiltered(
        &self, path: PyBytes, filter: PyObject, filterid: u64
    ) -> PyResult<Option<PyBytes>> {
        let mut state = self.state(py).borrow_mut();
        let path = path.data(py);

        let result = convert_result(py, state.get_filtered_key(
            path,
            &mut |path| {
                let path = PyBytes::new(py, path);
                let filtered = filter
                    .call(py, (path,), None)
                    .map_err(|e| callback_error(py, e))?
                    .extract::<PyBytes>(py)
                    .map_err(|e| callback_error(py, e))?;
                Ok(filtered.data(py).to_vec().into_boxed_slice())
            },
            filterid,
        ))?;

        Ok(result.map(|o| PyBytes::new(py, &o[..])))
    }

    def pathcomplete(
        &self, prefix: PyBytes, setbits: u16, unsetbits: u16, matchcallback: PyObject,
        fullpaths: bool
    ) -> PyResult<PyObject> {
        let setbits = StateFlags::from_bits_truncate(setbits);
        let unsetbits = StateFlags::from_bits_truncate(unsetbits);
        let mask = setbits | unsetbits;
        let mut state = self.state(py).borrow_mut();

        convert_result(py, state.path_complete(
            prefix.data(py),
            fullpaths,
            &|file| file.state & mask == setbits,
            &mut |components| {
                let path = PyBytes::new(py, &components.concat()).into_object();
                matchcallback.call(py, (path,), None).map_err(|e| callback_error(py, e))?;
                Ok(())
            },
        ))?;

        Ok(py.None())
    }

    def invalidatemtime(&self, fsnow: i32) -> PyResult<PyObject> {
        // Distrust changed files with a mtime of `fsnow`. Rewrite their mtime to -1.
        // See mercurial/pure/parsers.py:pack_dirstate in core Mercurial for motivation.
        // Basically, this is required for the following case:
        //
        // $ hg update rev; write foo; hg commit -m update-foo
        //
        //   Time (second) | 0   | 1           |
        //   hg update       ...----|
        //   write foo               |--|
        //   hg commit                   |---...
        //
        // If "write foo" changes a file without changing its mtime and size, the file
        // change won't be detected. Therefore if mtime is `fsnow`, reset it to a different
        // value and mark it as NEED_CHECK, at the end of update to workaround the issue.
        // Here, hg assumes nobody else is touching the working copy when it holds wlock
        // (ex. during second 0).
        //
        // This is used before "flush" or "saveas".
        //
        // Note: In TreeState's case, NEED_CHECK might mean "perform a quick mtime check",
        // or "perform a content check" depending on the caller. Be careful when removing
        // "mtime = -1" statement.
        let mut state = self.state(py).borrow_mut();
        convert_result(py, state.visit(
            &mut |_, state| {
                if state.mtime >= fsnow {
                    state.mtime = -1;
                    state.state |= StateFlags::NEED_CHECK;
                }
                Ok(())
            },
            &|_, dir| if !dir.is_changed() {
                false
            } else {
                match dir.get_aggregated_state() {
                    Some(x) => x.union.intersects(StateFlags::EXIST_P1 | StateFlags::EXIST_P2),
                    None => true,
                }
            },
            &|_, file| file.state.intersects(StateFlags::EXIST_P1 | StateFlags::EXIST_P2),
        ))?;

        Ok(py.None())
    }

    def getmetadata(&self) -> PyResult<PyBytes> {
        let state = self.state(py).borrow();
        let metadata = PyBytes::new(py, state.get_metadata());
        Ok(metadata)
    }

    def setmetadata(&self, metadata: PyBytes) -> PyResult<PyObject> {
        let mut state = self.state(py).borrow_mut();
        let metadata = metadata.data(py);
        state.set_metadata(metadata);
        Ok(py.None())
    }
});

/// Convert StateFlags to Mercurial dirstate state
fn flags_to_hg_state(_py: Python, flags: u16) -> PyResult<&'static str> {
    let flags = StateFlags::from_bits_truncate(flags);
    Ok(match (
        flags.intersects(StateFlags::EXIST_P1 | StateFlags::EXIST_P2),
        flags.contains(StateFlags::EXIST_P2 | StateFlags::EXIST_P2),
        flags.contains(StateFlags::EXIST_NEXT),
    ) {
        (true, true, true) => "m",  // merge
        (true, false, true) => "n", // normal
        (true, _, false) => "r",    // remove
        (false, _, true) => "a",    // add
        (false, _, false) => "?",   // untracked
    })
}

/// Convert a Result to PyResult
fn convert_result<T>(py: Python, result: errors::Result<T>) -> PyResult<T> {
    result.map_err(|e| PyErr::new::<exc::IOError, _>(py, e.description()))
}

fn encoding_error(py: Python) -> PyErr {
    PyErr::new::<exc::RuntimeError, _>(py, "invalid encoding")
}
