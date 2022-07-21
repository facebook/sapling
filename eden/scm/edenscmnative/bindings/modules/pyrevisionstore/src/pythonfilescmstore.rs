/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;
use cpython::FromPyObject;
use cpython::ObjectProtocol;
use cpython::PyBytes;
use cpython::PyObject;
use cpython::Python;
use cpython_ext::PyErr;
use cpython_ext::PyPathBuf;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use minibytes::Bytes;

use storemodel::ReadFileContents;
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

#[async_trait]
impl ReadFileContents for PythonFileScmStore {
    type Error = anyhow::Error;

    async fn read_file_contents(
        &self,
        keys: Vec<Key>,
    ) -> BoxStream<Result<(Bytes, Key), Self::Error>> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let contents = keys
            .into_iter()
            .map(|k| {
                let py_name = PyPathBuf::from(k.path.as_repo_path());
                let py_node = PyBytes::new(py, k.hgid.as_ref());
                let result = self
                    .read_file_contents
                    .call(py, (py_name, py_node), None)
                    .map_err(PyErr::from)?;
                let py_bytes = PyBytes::extract(py, &result).map_err(PyErr::from)?;
                let bytes = py_bytes.data(py).to_vec();
                Ok((bytes.into(), k))
            })
            .collect::<Vec<_>>();

        futures::stream::iter(contents.into_iter()).boxed()
    }
}
