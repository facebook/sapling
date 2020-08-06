/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::*;
use cpython_ext::{error, ResultPyErrExt};

use taggederror::{intentional_bail, intentional_error, CommonMetadata, Fault, FilteredAnyhow};

py_exception!(error, IndexedLogError);
py_exception!(error, MetaLogError);
py_exception!(error, RustError);
py_exception!(error, RevisionstoreError);
py_exception!(error, NonUTF8Path);
py_exception!(error, CommitLookupError, exc::KeyError);

py_class!(pub class TaggedExceptionData |py| {
    data metadata: CommonMetadata;
    data error_message: String;
    def __new__(_cls) -> PyResult<TaggedExceptionData> {
        TaggedExceptionData::create_instance(py, CommonMetadata::default(), String::new())
    }

    def fault(&self) -> PyResult<Option<&'static str>> {
        Ok(match self.metadata(py).fault {
            Some(Fault::Request) => Some("request"),
            Some(Fault::Internal) => Some("internal"),
            Some(Fault::Dependency) => Some("dependency"),
            None => None,
        })
    }

    def typename(&self) -> PyResult<Option<&'static str>> {
        Ok(self.metadata(py).type_name.map(|v| v.0))
    }

    def has_metadata(&self) -> PyResult<bool> {
        Ok(!self.metadata(py).empty())
    }

    def metadata_display(&self) -> PyResult<String> {
        Ok(format!("{}", self.metadata(py)))
    }

    def message(&self) -> PyResult<String> {
        Ok(self.error_message(py).clone())
    }

    def __repr__(&self) -> PyResult<String> {
        Ok(self.error_message(py).clone())
    }
});

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "error"].join(".");
    let m = PyModule::new(py, &name)?;

    m.add(py, "CommitLookupError", py.get_type::<CommitLookupError>())?;
    m.add(py, "IndexedLogError", py.get_type::<IndexedLogError>())?;
    m.add(py, "MetaLogError", py.get_type::<MetaLogError>())?;
    m.add(py, "RustError", py.get_type::<RustError>())?;
    m.add(
        py,
        "RevisionstoreError",
        py.get_type::<RevisionstoreError>(),
    )?;
    m.add(py, "NonUTF8Path", py.get_type::<NonUTF8Path>())?;
    m.add(
        py,
        "TaggedExceptionData",
        py.get_type::<TaggedExceptionData>(),
    )?;
    m.add(py, "throwrustexception", py_fn!(py, py_intentional_error()))?;
    m.add(py, "throwrustbail", py_fn!(py, py_intentional_bail()))?;

    register_error_handlers();

    Ok(m)
}

fn register_error_handlers() {
    fn specific_error_handler(py: Python, e: &error::Error, _m: CommonMetadata) -> Option<PyErr> {
        // Extract inner io::Error out.
        // Why does Python need the low-level IOError? It doesn't have to.
        // Consider:
        // - Only expose high-level errors to Python with just enough
        //   information that Python can consume. Python no longer handles
        //   IOError directly.
        // - Gain more explicit control about error types exposed to
        //   Python. This means dropping anyhow.
        if let Some(revlogindex::Error::Corruption(e)) = e.downcast_ref::<revlogindex::Error>() {
            if let revlogindex::errors::CorruptionError::Io(e) = e.as_ref() {
                return Some(cpython_ext::error::translate_io_error(py, e));
            }
        }

        if e.is::<indexedlog::Error>() {
            Some(PyErr::new::<IndexedLogError, _>(
                py,
                cpython_ext::Str::from(format!("{:?}", e)),
            ))
        } else if e.is::<metalog::Error>() {
            Some(PyErr::new::<MetaLogError, _>(
                py,
                cpython_ext::Str::from(format!("{:?}", e)),
            ))
        } else if e.is::<revisionstore::Error>() {
            Some(PyErr::new::<RevisionstoreError, _>(
                py,
                cpython_ext::Str::from(format!("{:?}", e)),
            ))
        } else if matches!(
            e.downcast_ref::<dag::Error>(),
            Some(dag::Error::VertexNotFound(_)) | Some(dag::Error::IdNotFound(_))
        ) {
            Some(PyErr::new::<CommitLookupError, _>(
                py,
                cpython_ext::Str::from(e.to_string()),
            ))
        } else if e.is::<cpython_ext::Error>() {
            Some(PyErr::new::<NonUTF8Path, _>(
                py,
                cpython_ext::Str::from(format!("{:?}", e)),
            ))
        } else {
            None
        }
    }

    fn fallback_error_handler(py: Python, e: &error::Error, m: CommonMetadata) -> Option<PyErr> {
        TaggedExceptionData::create_instance(py, m, format!("{:?}", FilteredAnyhow::new(e)))
            .map(|data| PyErr::new::<RustError, _>(py, data))
            .ok()
    }

    error::register("010-specific", specific_error_handler);
    error::register("999-fallback", fallback_error_handler);
}

fn py_intentional_error(py: Python) -> PyResult<PyInt> {
    Ok(intentional_error()
        .map(|r| r.to_py_object(py))
        .map_pyerr(py)?)
}

fn py_intentional_bail(py: Python) -> PyResult<PyInt> {
    Ok(intentional_bail()
        .map(|r| r.to_py_object(py))
        .map_pyerr(py)?)
}
