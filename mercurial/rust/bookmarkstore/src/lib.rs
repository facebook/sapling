extern crate bookmarkstore as bmstore;
#[macro_use]
extern crate cpython;
extern crate encoding;
extern crate types;

use bmstore::BookmarkStore;
use cpython::{exc, PyBytes, PyErr, PyList, PyObject, PyResult, PyString, Python, PythonObject};
use cpython::exc::UnicodeDecodeError;
use encoding::local_bytes_to_path;
use std::cell::RefCell;
use types::node::Node;

py_module_initializer!(
    bookmarkstore,
    initbookmarkstore,
    PyInit_bookmarkstore,
    |py, m| {
        m.add_class::<bookmarkstore>(py)?;
        Ok(())
    }
);

py_class!(class bookmarkstore |py| {
    data bm_store: RefCell<BookmarkStore>;

    def __new__(_cls, path: Option<&PyBytes> = None) -> PyResult<bookmarkstore> {
        let bm_store = match path {
            Some(p) => {
                let path = local_bytes_to_path(p.data(py)).map_err(|_| encoding_error(py, p))?;

                BookmarkStore::from_file(&path)
                    .map_err(|e| PyErr::new::<exc::IOError, _>(py, format!("{}", e)))?
            }
            None => BookmarkStore::new(),
        };
        bookmarkstore::create_instance(py, RefCell::new(bm_store))
    }

    def add_bookmark(&self, bookmark: &str, node: PyBytes) -> PyResult<PyObject> {
        let mut bm_store = self.bm_store(py).borrow_mut();
        let node = Node::from_slice(node.data(py))
            .map_err(|e| PyErr::new::<exc::ValueError, _>(py, format!("{}", e)))?;

        bm_store.add_bookmark(bookmark, node);

        Ok(py.None())
    }

    def remove_bookmark(&self, bookmark: &str) -> PyResult<PyObject> {
        let mut bm_store = self.bm_store(py).borrow_mut();

        bm_store
            .remove_bookmark(bookmark)
            .map_err(|e| PyErr::new::<exc::KeyError, _>(py, format!("{}", e)))?;
        Ok(py.None())
    }

    def lookup_bookmark(&self, bookmark: &str) -> PyResult<Option<PyBytes>> {
        let bm_store = self.bm_store(py).borrow();

        match bm_store.lookup_bookmark(bookmark) {
            Some(node) => Ok(Some(PyBytes::new(py, node.as_ref()))),
            None => Ok(None),
        }
    }

    def lookup_node(&self, node: PyBytes) -> PyResult<Option<PyList>> {
        let bm_store = self.bm_store(py).borrow();
        let node = Node::from_slice(node.data(py))
            .map_err(|e| PyErr::new::<exc::ValueError, _>(py, format!("{}", e)))?;

        match bm_store.lookup_node(node) {
            Some(bms) => {
                let bms: Vec<_> = bms.iter()
                    .map(|bm| PyString::new(py, bm).into_object())
                    .collect();
                Ok(Some(PyList::new(py, bms.as_slice())))
            }
            None => Ok(None),
        }
    }

    def flush(&self, path: &PyBytes) -> PyResult<PyObject> {
        let mut bm_store = self.bm_store(py).borrow_mut();
        let path = local_bytes_to_path(path.data(py)).map_err(|_| encoding_error(py, path))?;

        bm_store
            .flush(&path)
            .map_err(|e| PyErr::new::<exc::IOError, _>(py, format!("{}", e)))?;
        Ok(py.None())
    }
});

// Taken from mercurial/rust/config crate
fn encoding_error(py: Python, input: &PyBytes) -> PyErr {
    use std::ffi::CStr;
    let utf8 = CStr::from_bytes_with_nul(b"utf8\0").unwrap();
    let reason = CStr::from_bytes_with_nul(b"invalid encoding\0").unwrap();
    let input = input.data(py);
    let err = UnicodeDecodeError::new(py, utf8, input, 0..input.len(), reason).unwrap();
    PyErr::from_instance(py, err)
}
