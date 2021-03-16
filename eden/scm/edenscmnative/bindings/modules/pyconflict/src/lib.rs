/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use conflict::CommitConflict as RustCommitConflict;
use conflict::FileConflict as RustFileConflict;
use conflict::FileContext as RustFileContext;
use cpython::*;
use cpython_ext::ser::to_object;
use cpython_ext::ResultPyErrExt;
use types::HgId;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "conflict"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<FileContext>(py)?;
    m.add_class::<FileConflict>(py)?;
    m.add_class::<CommitConflict>(py)?;
    Ok(m)
}

py_class!(class FileContext |py| {
    data model: RustFileContext;

    def __new__(_cls, id: Option<PyBytes>, flags: &str = "", copyfrom: Option<String> = None, commitid: Option<PyBytes> = None) -> PyResult<Self> {
        let commit_id = match commitid {
            Some(id) => Some(HgId::from_slice(id.data(py)).map_pyerr(py)?),
            None => None,
        };
        let id = match id {
            Some(id) => Some(HgId::from_slice(id.data(py)).map_pyerr(py)?),
            None => None,
        };
        let model = RustFileContext {
            id,
            flags: flags.to_string(),
            copy_from: copyfrom,
            commit_id,
        };
        Self::create_instance(py, model)
    }

    def toobject(&self) -> PyResult<PyObject> {
        to_object(py, self.model(py))
    }

    def tobytes(&self) -> PyResult<PyBytes> {
        let bytes = serde_cbor::to_vec(self.model(py)).unwrap();
        Ok(PyBytes::new(py, &bytes))
    }

    @staticmethod
    def frombytes(bytes: PyBytes) -> PyResult<Self> {
        let model = serde_cbor::from_slice(bytes.data(py)).map_pyerr(py)?;
        Self::create_instance(py, model)
    }

});

impl FileContext {
    fn to_rust(&self, py: Python) -> RustFileContext {
        self.model(py).clone()
    }
}

py_class!(class FileConflict |py| {
    data model: RustFileConflict;

    def __new__(_cls, adds: Vec<FileContext>, removes: Vec<FileContext>) -> PyResult<Self> {
        let model = RustFileConflict {
            adds: adds.into_iter().map(|a| a.to_rust(py)).collect(),
            removes: removes.into_iter().map(|a| a.to_rust(py)).collect(),
        };
        Self::create_instance(py, model)
    }

    def __add__(lhs, rhs) -> PyResult<Self> {
        let lhs = Self::extract(py, lhs)?.to_rust(py);
        let rhs = Self::extract(py, rhs)?.to_rust(py);
        let model = lhs + rhs;
        Self::create_instance(py, model)
    }

    def __sub__(lhs, rhs) -> PyResult<Self> {
        let lhs = Self::extract(py, lhs)?.to_rust(py);
        let rhs = Self::extract(py, rhs)?.to_rust(py);
        let model = lhs - rhs;
        Self::create_instance(py, model)
    }

    def isresolved(&self) -> PyResult<bool> {
        Ok(self.model(py).is_resolved())
    }

    @staticmethod
    def fromfile(file: FileContext) -> PyResult<Self> {
        let model = RustFileConflict::from_file(file.to_rust(py));
        Self::create_instance(py, model)
    }

    @staticmethod
    def from3way(base: FileContext, local: FileContext, other: FileContext) -> PyResult<Self> {
        let model = RustFileConflict::from_3way(base.to_rust(py), local.to_rust(py), other.to_rust(py));
        Self::create_instance(py, model)
    }

    def withresolution(&self, resolution: FileContext) -> PyResult<Self> {
        let model = self.to_rust(py).with_resolution(resolution.to_rust(py));
        Self::create_instance(py, model)
    }

    def complexity(&self) -> PyResult<usize> {
        Ok(self.model(py).complexity())
    }

    def toobject(&self) -> PyResult<PyObject> {
        to_object(py, self.model(py))
    }

    def tobytes(&self) -> PyResult<PyBytes> {
        let bytes = serde_cbor::to_vec(self.model(py)).unwrap();
        Ok(PyBytes::new(py, &bytes))
    }

    @staticmethod
    def frombytes(bytes: PyBytes) -> PyResult<Self> {
        let model = serde_cbor::from_slice(bytes.data(py)).map_pyerr(py)?;
        Self::create_instance(py, model)
    }

    def adds(&self) -> PyResult<Vec<FileContext>> {
        let model = self.model(py);
        let mut result = Vec::with_capacity(model.adds.len());
        for add in &model.adds {
            result.push(FileContext::create_instance(py, add.clone())?);
        }
        Ok(result)
    }

    def removes(&self) -> PyResult<Vec<FileContext>> {
        let model = self.model(py);
        let mut result = Vec::with_capacity(model.removes.len());
        for remove in &model.removes {
            result.push(FileContext::create_instance(py, remove.clone())?);
        }
        Ok(result)
    }
});

impl FileConflict {
    fn to_rust(&self, py: Python) -> RustFileConflict {
        self.model(py).clone()
    }
}

py_class!(class CommitConflict |py| {
    data model: RustCommitConflict;

    def __new__(_cls, pathconflicts: Vec<(String, FileConflict)>) -> PyResult<Self> {
        let model = RustCommitConflict {
            files: pathconflicts.into_iter().map(|(p, c)| (p, c.to_rust(py))).collect()
        };
        Self::create_instance(py, model)
    }

    /// Lookup by path. Returns None or FileConflict.
    def get(&self, path: &str) -> PyResult<Option<FileConflict>> {
        match self.model(py).files.get(path) {
            Some(conflict) => Ok(Some(FileConflict::create_instance(py, conflict.clone())?)),
            None => Ok(None),
        }
    }

    def toobject(&self) -> PyResult<PyObject> {
        to_object(py, self.model(py))
    }

    def tobytes(&self) -> PyResult<PyBytes> {
        let bytes = serde_cbor::to_vec(self.model(py)).unwrap();
        Ok(PyBytes::new(py, &bytes))
    }

    @staticmethod
    def frombytes(bytes: PyBytes) -> PyResult<Self> {
        let model = serde_cbor::from_slice(bytes.data(py)).map_pyerr(py)?;
        Self::create_instance(py, model)
    }
});

impl CommitConflict {
    fn to_rust(&self, py: Python) -> RustCommitConflict {
        self.model(py).clone()
    }
}
