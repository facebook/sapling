/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::FromPyObject;
use cpython::ObjectProtocol;
use cpython::PyBytes;
use cpython::PyObject;
use cpython::Python;
use cpython_ext::PyErr;
use cpython_ext::PyPathBuf;
use minibytes::Bytes;
use storemodel::BoxIterator;
use storemodel::FileStore;
use storemodel::KeyStore;
use types::Key;

pub struct PythonFileScmStore {
    read_file_contents: PyObject,
}

impl PythonFileScmStore {
    pub fn new(read_file_contents: PyObject) -> Self {
        let gil = Python::acquire_gil();
        let py = gil.python();
        if !read_file_contents.is_callable(py) {
            panic!("read_file_contents must be callable, e.g. a lambda");
        }

        PythonFileScmStore { read_file_contents }
    }
}

impl KeyStore for PythonFileScmStore {
    fn get_content_iter(
        &self,
        keys: Vec<Key>,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, Bytes)>>> {
        let iter = keys.into_iter().map(|k| {
            let gil = Python::acquire_gil();
            let py = gil.python();
            let py_name = PyPathBuf::from(k.path.as_repo_path());
            let py_node = PyBytes::new(py, k.hgid.as_ref());
            let result = match self.read_file_contents.call(py, (py_name, py_node), None) {
                Err(e) => return Err(PyErr::from(e).into()),
                Ok(v) => v,
            };
            let py_bytes = match PyBytes::extract(py, &result) {
                Err(e) => return Err(PyErr::from(e).into()),
                Ok(v) => v,
            };
            let bytes = Bytes::copy_from_slice(py_bytes.data(py));
            Ok((k, bytes))
        });
        Ok(Box::new(iter))
    }
}

impl FileStore for PythonFileScmStore {}
